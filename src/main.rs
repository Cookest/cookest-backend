//! Cookest API — Rust + Actix-Web + SeaORM + PostgreSQL
//!
//! Features:
//! - Argon2id password hashing
//! - JWT with refresh token rotation
//! - Rate limiting
//! - HttpOnly cookies
//! - Security headers
//! - Full recipe, ingredient, nutrition, inventory, meal plan, and AI chat database

mod config;
mod db;
mod entity;
mod errors;
mod handlers;
mod middleware;
mod models;
mod services;
mod validation;

use actix_cors::Cors;
use actix_web::{http::header, middleware::Logger, web, App, HttpServer};
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::Config;
use crate::handlers::{
    configure_auth, configure_recipes, configure_ingredients, configure_user, configure_chat,
    configure_onboarding, configure_shopping_list, configure_subscription, configure_stores,
};
use crate::middleware::{JwtAuth, RateLimit};
use crate::services::{
    AuthService, TokenService, RecipeService, IngredientService,
    MealPlanService, InventoryService, ProfileService, InteractionService, ChatService,
    OnboardingService, ShoppingListService, SubscriptionService, StoreService,
};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,auth_api=debug".to_string()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting Cookest API");

    // Load configuration
    let config = Config::from_env().expect("Failed to load configuration");

    let bind_address = format!("{}:{}", config.host, config.port);
    let cors_origin = config.cors_origin.clone();

    // Connect to database
    let db = db::establish_connection(&config)
        .await
        .expect("Failed to connect to database");

    // Run all migrations in dependency order
    tracing::info!("Running database migrations...");
    use sea_orm::{ConnectionTrait, Statement};

    let migrations: &[&str] = &[
        // ── Extensions ──────────────────────────────────────────────────────────
        r#"CREATE EXTENSION IF NOT EXISTS "uuid-ossp";"#,
        r#"CREATE EXTENSION IF NOT EXISTS pg_trgm;"#,

        // ── Users (extended with profile fields) ────────────────────────────────
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id                      UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
            email                   VARCHAR(255) UNIQUE NOT NULL,
            name                    VARCHAR(255),
            password_hash           TEXT NOT NULL,
            refresh_token_hash      TEXT,
            household_size          INTEGER NOT NULL DEFAULT 1,
            dietary_restrictions    TEXT[] DEFAULT '{}',
            allergies               TEXT[] DEFAULT '{}',
            avatar_url              TEXT,
            is_email_verified       BOOLEAN NOT NULL DEFAULT FALSE,
            two_factor_enabled      BOOLEAN NOT NULL DEFAULT FALSE,
            totp_secret             TEXT,
            failed_login_attempts   INTEGER NOT NULL DEFAULT 0,
            locked_until            TIMESTAMPTZ,
            created_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at              TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);
        "#,

        // ── Ingredients ──────────────────────────────────────────────────────────
        r#"
        CREATE TABLE IF NOT EXISTS ingredients (
            id          BIGSERIAL PRIMARY KEY,
            name        TEXT UNIQUE NOT NULL,
            category    TEXT,
            fdc_id      INTEGER,
            off_id      TEXT,
            created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE INDEX IF NOT EXISTS idx_ingredients_name_trgm
            ON ingredients USING GIN (name gin_trgm_ops);
        CREATE INDEX IF NOT EXISTS idx_ingredients_category
            ON ingredients(category);
        CREATE INDEX IF NOT EXISTS idx_ingredients_fdc_id
            ON ingredients(fdc_id) WHERE fdc_id IS NOT NULL;
        "#,

        // ── Ingredient Nutrients ─────────────────────────────────────────────────
        r#"
        CREATE TABLE IF NOT EXISTS ingredient_nutrients (
            id                  BIGSERIAL PRIMARY KEY,
            ingredient_id       BIGINT NOT NULL REFERENCES ingredients(id) ON DELETE CASCADE,
            calories            NUMERIC(10,4),
            protein_g           NUMERIC(10,4),
            carbs_g             NUMERIC(10,4),
            fat_g               NUMERIC(10,4),
            fiber_g             NUMERIC(10,4),
            sugar_g             NUMERIC(10,4),
            sodium_mg           NUMERIC(10,4),
            saturated_fat_g     NUMERIC(10,4),
            cholesterol_mg      NUMERIC(10,4),
            micronutrients      JSONB,
            UNIQUE(ingredient_id)
        );
        CREATE INDEX IF NOT EXISTS idx_ingredient_nutrients_ingredient
            ON ingredient_nutrients(ingredient_id);
        CREATE INDEX IF NOT EXISTS idx_ingredient_nutrients_micros
            ON ingredient_nutrients USING GIN (micronutrients);
        "#,

        // ── Portion Sizes ────────────────────────────────────────────────────────
        r#"
        CREATE TABLE IF NOT EXISTS portion_sizes (
            id              BIGSERIAL PRIMARY KEY,
            ingredient_id   BIGINT NOT NULL REFERENCES ingredients(id) ON DELETE CASCADE,
            description     TEXT NOT NULL,
            weight_grams    NUMERIC(10,3) NOT NULL,
            unit            TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_portion_sizes_ingredient
            ON portion_sizes(ingredient_id);
        "#,

        // ── Recipes ──────────────────────────────────────────────────────────────
        r#"
        CREATE TABLE IF NOT EXISTS recipes (
            id              BIGSERIAL PRIMARY KEY,
            name            TEXT NOT NULL,
            slug            TEXT UNIQUE NOT NULL,
            description     TEXT,
            cuisine         TEXT,
            category        TEXT,
            difficulty      TEXT,
            servings        INTEGER NOT NULL DEFAULT 2,
            prep_time_min   INTEGER,
            cook_time_min   INTEGER,
            total_time_min  INTEGER,
            is_vegetarian   BOOLEAN NOT NULL DEFAULT FALSE,
            is_vegan        BOOLEAN NOT NULL DEFAULT FALSE,
            is_gluten_free  BOOLEAN NOT NULL DEFAULT FALSE,
            is_dairy_free   BOOLEAN NOT NULL DEFAULT FALSE,
            is_nut_free     BOOLEAN NOT NULL DEFAULT FALSE,
            source_url      TEXT,
            average_rating  NUMERIC(3,2),
            rating_count    INTEGER NOT NULL DEFAULT 0,
            created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE INDEX IF NOT EXISTS idx_recipes_name_trgm
            ON recipes USING GIN (name gin_trgm_ops);
        CREATE INDEX IF NOT EXISTS idx_recipes_cuisine    ON recipes(cuisine);
        CREATE INDEX IF NOT EXISTS idx_recipes_category   ON recipes(category);
        CREATE INDEX IF NOT EXISTS idx_recipes_difficulty ON recipes(difficulty);
        CREATE INDEX IF NOT EXISTS idx_recipes_dietary
            ON recipes(is_vegetarian, is_vegan, is_gluten_free, is_dairy_free, is_nut_free);
        "#,

        // ── Recipe Ingredients ───────────────────────────────────────────────────
        r#"
        CREATE TABLE IF NOT EXISTS recipe_ingredients (
            id              BIGSERIAL PRIMARY KEY,
            recipe_id       BIGINT NOT NULL REFERENCES recipes(id) ON DELETE CASCADE,
            ingredient_id   BIGINT NOT NULL REFERENCES ingredients(id) ON DELETE RESTRICT,
            quantity        NUMERIC(10,3),
            unit            TEXT,
            quantity_grams  NUMERIC(10,3),
            notes           TEXT,
            display_order   INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_recipe_ingredients_recipe
            ON recipe_ingredients(recipe_id);
        CREATE INDEX IF NOT EXISTS idx_recipe_ingredients_ingredient
            ON recipe_ingredients(ingredient_id);
        "#,

        // ── Recipe Steps ─────────────────────────────────────────────────────────
        r#"
        CREATE TABLE IF NOT EXISTS recipe_steps (
            id              BIGSERIAL PRIMARY KEY,
            recipe_id       BIGINT NOT NULL REFERENCES recipes(id) ON DELETE CASCADE,
            step_number     INTEGER NOT NULL,
            instruction     TEXT NOT NULL,
            duration_min    INTEGER,
            image_url       TEXT,
            tip             TEXT,
            UNIQUE(recipe_id, step_number)
        );
        CREATE INDEX IF NOT EXISTS idx_recipe_steps_recipe
            ON recipe_steps(recipe_id);
        "#,

        // ── Recipe Images ────────────────────────────────────────────────────────
        r#"
        CREATE TABLE IF NOT EXISTS recipe_images (
            id          BIGSERIAL PRIMARY KEY,
            recipe_id   BIGINT NOT NULL REFERENCES recipes(id) ON DELETE CASCADE,
            url         TEXT NOT NULL,
            image_type  TEXT,
            is_primary  BOOLEAN NOT NULL DEFAULT FALSE,
            width       INTEGER,
            height      INTEGER,
            source      TEXT,
            created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE INDEX IF NOT EXISTS idx_recipe_images_recipe
            ON recipe_images(recipe_id);
        CREATE INDEX IF NOT EXISTS idx_recipe_images_primary
            ON recipe_images(recipe_id, is_primary) WHERE is_primary = TRUE;
        "#,

        // ── Recipe Nutrition (precomputed) ───────────────────────────────────────
        r#"
        CREATE TABLE IF NOT EXISTS recipe_nutrition (
            id                  BIGSERIAL PRIMARY KEY,
            recipe_id           BIGINT NOT NULL REFERENCES recipes(id) ON DELETE CASCADE,
            per_serving         BOOLEAN NOT NULL DEFAULT TRUE,
            calories            NUMERIC(10,4),
            protein_g           NUMERIC(10,4),
            carbs_g             NUMERIC(10,4),
            fat_g               NUMERIC(10,4),
            fiber_g             NUMERIC(10,4),
            sugar_g             NUMERIC(10,4),
            sodium_mg           NUMERIC(10,4),
            saturated_fat_g     NUMERIC(10,4),
            cholesterol_mg      NUMERIC(10,4),
            micronutrients      JSONB,
            calculated_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            UNIQUE(recipe_id)
        );
        CREATE INDEX IF NOT EXISTS idx_recipe_nutrition_recipe
            ON recipe_nutrition(recipe_id);
        "#,

        // ── User Favorites ───────────────────────────────────────────────────────
        r#"
        CREATE TABLE IF NOT EXISTS user_favorites (
            id          BIGSERIAL PRIMARY KEY,
            user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            recipe_id   BIGINT NOT NULL REFERENCES recipes(id) ON DELETE CASCADE,
            saved_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            UNIQUE(user_id, recipe_id)
        );
        CREATE INDEX IF NOT EXISTS idx_user_favorites_user   ON user_favorites(user_id);
        CREATE INDEX IF NOT EXISTS idx_user_favorites_recipe ON user_favorites(recipe_id);
        "#,

        // ── Recipe Ratings ───────────────────────────────────────────────────────
        r#"
        CREATE TABLE IF NOT EXISTS recipe_ratings (
            id          BIGSERIAL PRIMARY KEY,
            user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            recipe_id   BIGINT NOT NULL REFERENCES recipes(id) ON DELETE CASCADE,
            rating      SMALLINT NOT NULL CHECK (rating BETWEEN 1 AND 5),
            comment     TEXT,
            created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            UNIQUE(user_id, recipe_id)
        );
        CREATE INDEX IF NOT EXISTS idx_recipe_ratings_recipe ON recipe_ratings(recipe_id);
        CREATE INDEX IF NOT EXISTS idx_recipe_ratings_user   ON recipe_ratings(user_id);
        "#,

        // ── Cooking History ──────────────────────────────────────────────────────
        r#"
        CREATE TABLE IF NOT EXISTS cooking_history (
            id                  BIGSERIAL PRIMARY KEY,
            user_id             UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            recipe_id           BIGINT NOT NULL REFERENCES recipes(id) ON DELETE CASCADE,
            servings_made       INTEGER NOT NULL DEFAULT 1,
            inventory_deducted  BOOLEAN NOT NULL DEFAULT FALSE,
            cooked_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE INDEX IF NOT EXISTS idx_cooking_history_user   ON cooking_history(user_id);
        CREATE INDEX IF NOT EXISTS idx_cooking_history_recipe ON cooking_history(recipe_id);
        CREATE INDEX IF NOT EXISTS idx_cooking_history_date   ON cooking_history(user_id, cooked_at DESC);
        "#,

        // ── Inventory Items ──────────────────────────────────────────────────────
        r#"
        CREATE TABLE IF NOT EXISTS inventory_items (
            id                  BIGSERIAL PRIMARY KEY,
            user_id             UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            ingredient_id       BIGINT NOT NULL REFERENCES ingredients(id) ON DELETE RESTRICT,
            custom_name         TEXT,
            quantity            NUMERIC(10,3) NOT NULL,
            unit                TEXT NOT NULL,
            expiry_date         DATE,
            storage_location    TEXT,
            added_at            TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE INDEX IF NOT EXISTS idx_inventory_user        ON inventory_items(user_id);
        CREATE INDEX IF NOT EXISTS idx_inventory_ingredient  ON inventory_items(ingredient_id);
        CREATE INDEX IF NOT EXISTS idx_inventory_expiry
            ON inventory_items(user_id, expiry_date) WHERE expiry_date IS NOT NULL;
        "#,

        // ── Meal Plans ───────────────────────────────────────────────────────────
        r#"
        CREATE TABLE IF NOT EXISTS meal_plans (
            id                  BIGSERIAL PRIMARY KEY,
            user_id             UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            week_start          DATE NOT NULL,
            is_ai_generated     BOOLEAN NOT NULL DEFAULT FALSE,
            created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            UNIQUE(user_id, week_start)
        );
        CREATE INDEX IF NOT EXISTS idx_meal_plans_user ON meal_plans(user_id);
        "#,

        // ── Meal Plan Slots ──────────────────────────────────────────────────────
        r#"
        CREATE TABLE IF NOT EXISTS meal_plan_slots (
            id                  BIGSERIAL PRIMARY KEY,
            meal_plan_id        BIGINT NOT NULL REFERENCES meal_plans(id) ON DELETE CASCADE,
            recipe_id           BIGINT NOT NULL REFERENCES recipes(id) ON DELETE RESTRICT,
            day_of_week         SMALLINT NOT NULL CHECK (day_of_week BETWEEN 0 AND 6),
            meal_type           TEXT NOT NULL,
            servings_override   INTEGER,
            is_completed        BOOLEAN NOT NULL DEFAULT FALSE,
            UNIQUE(meal_plan_id, day_of_week, meal_type)
        );
        CREATE INDEX IF NOT EXISTS idx_meal_plan_slots_plan   ON meal_plan_slots(meal_plan_id);
        CREATE INDEX IF NOT EXISTS idx_meal_plan_slots_recipe ON meal_plan_slots(recipe_id);
        "#,

        // ── Chat Sessions ────────────────────────────────────────────────────────
        r#"
        CREATE TABLE IF NOT EXISTS chat_sessions (
            id                  BIGSERIAL PRIMARY KEY,
            user_id             UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            current_recipe_id   BIGINT REFERENCES recipes(id) ON DELETE SET NULL,
            title               TEXT,
            created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE INDEX IF NOT EXISTS idx_chat_sessions_user ON chat_sessions(user_id);
        "#,

        // ── Chat Messages ────────────────────────────────────────────────────────
        r#"
        CREATE TABLE IF NOT EXISTS chat_messages (
            id          BIGSERIAL PRIMARY KEY,
            session_id  BIGINT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
            role        TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'system')),
            content     TEXT NOT NULL,
            tokens_used INTEGER,
            created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE INDEX IF NOT EXISTS idx_chat_messages_session
            ON chat_messages(session_id, created_at ASC);
        "#,

        // ── User Preferences (ML vector) ─────────────────────────────────────
        r#"
        CREATE TABLE IF NOT EXISTS user_preferences (
            user_id             UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
            cuisine_weights     JSONB NOT NULL DEFAULT '{}',
            ingredient_weights  JSONB NOT NULL DEFAULT '{}',
            macro_bias          JSONB NOT NULL DEFAULT '{"protein":0.0,"carbs":0.0,"fat":0.0}',
            difficulty_weights  JSONB NOT NULL DEFAULT '{"easy":0.0,"medium":0.0,"hard":0.0}',
            preferred_time_min  INTEGER NOT NULL DEFAULT 30,
            interaction_count   INTEGER NOT NULL DEFAULT 0,
            updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE INDEX IF NOT EXISTS idx_user_preferences_updated
            ON user_preferences(updated_at);
        "#,

        // ── Schema v2: Subscription + onboarding fields on users ────────────
        r#"ALTER TABLE users ADD COLUMN IF NOT EXISTS subscription_tier TEXT NOT NULL DEFAULT 'free';"#,
        r#"ALTER TABLE users ADD COLUMN IF NOT EXISTS subscription_valid_until TIMESTAMPTZ;"#,
        r#"ALTER TABLE users ADD COLUMN IF NOT EXISTS stripe_customer_id TEXT;"#,
        r#"ALTER TABLE users ADD COLUMN IF NOT EXISTS cooking_skill_level TEXT;"#,
        r#"ALTER TABLE users ADD COLUMN IF NOT EXISTS preferred_cuisines TEXT[] DEFAULT '{}';"#,
        r#"ALTER TABLE users ADD COLUMN IF NOT EXISTS health_goals TEXT[] DEFAULT '{}';"#,
        r#"ALTER TABLE users ADD COLUMN IF NOT EXISTS weekly_budget NUMERIC(10,2);"#,
        r#"ALTER TABLE users ADD COLUMN IF NOT EXISTS preferred_time_per_meal_min INTEGER;"#,
        r#"ALTER TABLE users ADD COLUMN IF NOT EXISTS onboarding_completed BOOLEAN NOT NULL DEFAULT FALSE;"#,
        r#"ALTER TABLE users ADD COLUMN IF NOT EXISTS is_admin BOOLEAN NOT NULL DEFAULT FALSE;"#,
        r#"CREATE INDEX IF NOT EXISTS idx_users_stripe ON users(stripe_customer_id) WHERE stripe_customer_id IS NOT NULL;"#,

        // ── Schema v2: Flex / energy fields on meal_plan_slots ──────────────
        // recipe_id becomes nullable to support flex days (no recipe assigned)
        r#"ALTER TABLE meal_plan_slots ALTER COLUMN recipe_id DROP NOT NULL;"#,
        r#"ALTER TABLE meal_plan_slots ADD COLUMN IF NOT EXISTS is_flex BOOLEAN NOT NULL DEFAULT FALSE;"#,
        r#"ALTER TABLE meal_plan_slots ADD COLUMN IF NOT EXISTS flex_type TEXT;"#,
        r#"ALTER TABLE meal_plan_slots ADD COLUMN IF NOT EXISTS energy_level TEXT;"#,

        // ── Schema v2: author_id on recipes ──────────────────────────────────
        r#"ALTER TABLE recipes ADD COLUMN IF NOT EXISTS author_id UUID REFERENCES users(id) ON DELETE SET NULL;"#,
        r#"ALTER TABLE recipes ADD COLUMN IF NOT EXISTS is_public BOOLEAN NOT NULL DEFAULT TRUE;"#,
        r#"CREATE INDEX IF NOT EXISTS idx_recipes_author ON recipes(author_id) WHERE author_id IS NOT NULL;"#,

        // ── Stores ─────────────────────────────────────────────────────────────
        // Must come before pdf_processing_jobs (jobs FK → stores)
        r#"
        CREATE TABLE IF NOT EXISTS stores (
            id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
            name        TEXT NOT NULL,
            slug        TEXT UNIQUE NOT NULL,
            website     TEXT,
            logo_url    TEXT,
            country     TEXT,
            city        TEXT,
            lat         DOUBLE PRECISION,
            lng         DOUBLE PRECISION,
            created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE INDEX IF NOT EXISTS idx_stores_slug ON stores(slug);
        "#,

        // ── PDF Processing Jobs ───────────────────────────────────────────────
        // Must come before candidates (candidates FK → jobs)
        r#"
        CREATE TABLE IF NOT EXISTS pdf_processing_jobs (
            id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
            store_id        UUID NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
            file_path       TEXT NOT NULL,
            status          TEXT NOT NULL DEFAULT 'pending',
            error           TEXT,
            retry_count     INTEGER NOT NULL DEFAULT 0,
            started_at      TIMESTAMPTZ,
            heartbeat_at    TIMESTAMPTZ,
            processed_at    TIMESTAMPTZ,
            created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE INDEX IF NOT EXISTS idx_pdf_jobs_store  ON pdf_processing_jobs(store_id);
        CREATE INDEX IF NOT EXISTS idx_pdf_jobs_status ON pdf_processing_jobs(status);
        "#,

        // ── Store Promotions (published, admin-approved) ──────────────────────
        r#"
        CREATE TABLE IF NOT EXISTS store_promotions (
            id                  UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
            store_id            UUID NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
            product_name        TEXT NOT NULL,
            brand               TEXT,
            original_price      NUMERIC(10,2),
            discounted_price    NUMERIC(10,2) NOT NULL,
            discount_pct        NUMERIC(5,2),
            unit                TEXT,
            valid_from          TIMESTAMPTZ,
            valid_until         TIMESTAMPTZ,
            is_active           BOOLEAN NOT NULL DEFAULT TRUE,
            source_pdf_url      TEXT,
            confidence          NUMERIC(4,3),
            created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE INDEX IF NOT EXISTS idx_store_promotions_store  ON store_promotions(store_id);
        CREATE INDEX IF NOT EXISTS idx_store_promotions_active ON store_promotions(store_id, is_active, valid_until);
        CREATE INDEX IF NOT EXISTS idx_store_promotions_name_trgm
            ON store_promotions USING GIN (product_name gin_trgm_ops);
        "#,

        // ── Store Promotion Ingredients (cross-reference) ─────────────────────
        r#"
        CREATE TABLE IF NOT EXISTS store_promotion_ingredients (
            promotion_id    UUID NOT NULL REFERENCES store_promotions(id) ON DELETE CASCADE,
            ingredient_id   BIGINT NOT NULL REFERENCES ingredients(id) ON DELETE CASCADE,
            similarity_score NUMERIC(4,3),
            PRIMARY KEY (promotion_id, ingredient_id)
        );
        CREATE INDEX IF NOT EXISTS idx_promo_ingredients_ingredient
            ON store_promotion_ingredients(ingredient_id);
        "#,

        // ── Store Promotion Candidates (AI staging, awaiting admin review) ────
        r#"
        CREATE TABLE IF NOT EXISTS store_promotion_candidates (
            id                  UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
            store_id            UUID NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
            job_id              UUID NOT NULL REFERENCES pdf_processing_jobs(id) ON DELETE CASCADE,
            product_name        TEXT NOT NULL,
            brand               TEXT,
            original_price      NUMERIC(10,2),
            discounted_price    NUMERIC(10,2) NOT NULL,
            discount_pct        NUMERIC(5,2),
            unit                TEXT,
            valid_from          TIMESTAMPTZ,
            valid_until         TIMESTAMPTZ,
            confidence          NUMERIC(4,3),
            review_status       TEXT NOT NULL DEFAULT 'pending',
            reviewed_by         UUID REFERENCES users(id) ON DELETE SET NULL,
            reviewed_at         TIMESTAMPTZ,
            created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE INDEX IF NOT EXISTS idx_candidates_job    ON store_promotion_candidates(job_id);
        CREATE INDEX IF NOT EXISTS idx_candidates_review ON store_promotion_candidates(review_status);
        "#,

        // ── Shopping List ─────────────────────────────────────────────────────
        r#"
        CREATE TABLE IF NOT EXISTS shopping_list_items (
            id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
            user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            ingredient_id   BIGINT REFERENCES ingredients(id) ON DELETE SET NULL,
            name            TEXT NOT NULL,
            quantity        NUMERIC(10,3),
            unit            TEXT,
            is_checked      BOOLEAN NOT NULL DEFAULT FALSE,
            is_manual       BOOLEAN NOT NULL DEFAULT FALSE,
            meal_plan_id    BIGINT REFERENCES meal_plans(id) ON DELETE SET NULL,
            created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE INDEX IF NOT EXISTS idx_shopping_list_user       ON shopping_list_items(user_id);
        CREATE INDEX IF NOT EXISTS idx_shopping_list_ingredient ON shopping_list_items(ingredient_id) WHERE ingredient_id IS NOT NULL;
        "#,

        // ── User Push Tokens ─────────────────────────────────────────────────
        r#"
        CREATE TABLE IF NOT EXISTS user_push_tokens (
            id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
            user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            token       TEXT UNIQUE NOT NULL,
            platform    TEXT NOT NULL,
            created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE INDEX IF NOT EXISTS idx_push_tokens_user ON user_push_tokens(user_id);
        "#,

        // ── Stripe Processed Events (idempotency) ─────────────────────────────
        r#"
        CREATE TABLE IF NOT EXISTS stripe_processed_events (
            event_id        TEXT PRIMARY KEY,
            processed_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        "#,
    ];

    for sql in migrations {
        for statement in sql.split(';') {
            let stmt = statement.trim();
            if stmt.is_empty() {
                continue;
            }
            db.execute(Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                stmt.to_string(),
            ))
            .await
            .expect("Failed to run migration");
        }
    }

    tracing::info!("All {} migrations complete", migrations.len());

    // Initialize services
    let token_service = Arc::new(TokenService::new(&config));
    let auth_service = Arc::new(AuthService::new(db.clone(), TokenService::new(&config)));
    let recipe_service = Arc::new(RecipeService::new(db.clone()));
    let ingredient_service = Arc::new(IngredientService::new(db.clone()));
    let meal_plan_service = Arc::new(MealPlanService::new(db.clone()));
    let inventory_service = Arc::new(InventoryService::new(db.clone()));
    let profile_service = Arc::new(ProfileService::new(db.clone()));
    let interaction_service = Arc::new(InteractionService::new(db.clone()));
    let chat_service = Arc::new(ChatService::new(db.clone()));
    let onboarding_service = Arc::new(OnboardingService::new(db.clone()));
    let shopping_list_service = Arc::new(ShoppingListService::new(db.clone()));
    let subscription_service = Arc::new(SubscriptionService::new(
        db.clone(),
        config.stripe_webhook_secret.clone(),
    ));

    // Ensure PDF upload directory exists
    std::fs::create_dir_all(&config.pdf_upload_dir)
        .expect("Failed to create PDF upload directory");

    let store_service = Arc::new(StoreService::new(
        db.clone(),
        std::path::PathBuf::from(&config.pdf_upload_dir),
        config.ollama_url.clone(),
        config.ollama_model.clone(),
    ));

    tracing::info!("Server starting on {}", bind_address);

    HttpServer::new(move || {
        // Configure CORS
        let cors = Cors::default()
            .allowed_origin(&cors_origin)
            .allowed_methods(vec!["GET", "POST", "PUT", "DELETE", "OPTIONS"])
            .allowed_headers(vec![
                header::AUTHORIZATION,
                header::ACCEPT,
                header::CONTENT_TYPE,
            ])
            .supports_credentials()
            .max_age(3600);

        App::new()
            // Security: Request body size limit (10 MB - larger for recipe images)
            .app_data(web::JsonConfig::default().limit(10 * 1024 * 1024))
            // Logging
            .wrap(Logger::default())
            // CORS
            .wrap(cors)
            // Rate limiting for auth endpoints
            .wrap(RateLimit::strict())
            // Services
            .app_data(web::Data::new(auth_service.clone()))
            .app_data(web::Data::new(token_service.clone()))
            .app_data(web::Data::new(recipe_service.clone()))
            .app_data(web::Data::new(ingredient_service.clone()))
            .app_data(web::Data::new(meal_plan_service.clone()))
            .app_data(web::Data::new(inventory_service.clone()))
            .app_data(web::Data::new(profile_service.clone()))
            .app_data(web::Data::new(interaction_service.clone()))
            .app_data(web::Data::new(chat_service.clone()))
            .app_data(web::Data::new(onboarding_service.clone()))
            .app_data(web::Data::new(shopping_list_service.clone()))
            .app_data(web::Data::new(subscription_service.clone()))
            .app_data(web::Data::new(store_service.clone()))
            .app_data(web::Data::new(db.clone()))
            // ── Public routes (no JWT required) ──────────────────────────────
            .configure(configure_auth)        // /api/auth/*
            .configure(configure_recipes)     // /api/recipes/* (read-only browsing)
            .configure(configure_ingredients) // /api/ingredients/* (search)
            .configure(configure_subscription) // /api/webhooks/stripe (raw body, no JWT)
            // ── Protected routes (JWT required) ──────────────────────────────
            .service(
                web::scope("")
                    .wrap(JwtAuth::new(token_service.clone()))
                    .configure(configure_user)          // /api/me/*, /api/inventory/*, /api/meal-plans/*
                    .configure(configure_chat)          // /api/chat/*
                    .configure(configure_onboarding)    // /api/onboarding, /api/me/change-password, /api/me
                    .configure(configure_shopping_list) // /api/shopping-list/*
                    .configure(configure_stores)        // /api/stores/*, /api/admin/stores/*
            )
            // Health check (public, no auth)
            .route("/health", web::get().to(|| async {
                actix_web::HttpResponse::Ok().json(serde_json::json!({
                    "status": "healthy",
                    "service": "cookest-api"
                }))
            }))
    })
    .bind(&bind_address)?
    .run()
    .await
}
