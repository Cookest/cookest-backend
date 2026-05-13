//! Cookest Food API — Standalone food, recipe, and nutrition database API
//!
//! A sellable product providing:
//! - Ingredient catalog with nutrition data
//! - Recipe database with images, steps, and nutrition
//! - Allergen tracking
//! - API key authentication

mod config;
mod db;
mod entity;
mod errors;
mod handlers;
mod models;
mod services;
mod middleware;

use actix_cors::Cors;
use actix_web::{web, App, HttpServer, middleware::Logger};
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::Config;
use crate::handlers::{configure_ingredients, configure_recipes};
use crate::services::{IngredientService, RecipeService};
use crate::middleware::security_headers::SecurityHeaders;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,cookest_food_api=debug".to_string()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting Cookest Food API");

    let config = Config::from_env().expect("Failed to load configuration");
    let bind_address = format!("{}:{}", config.host, config.port);
    let cors_origin = config.cors_origin.clone();

    let db = db::establish_connection(config.database_url())
        .await
        .expect("Failed to connect to database");

    // Run migrations
    tracing::info!("Running food database migrations...");
    use sea_orm::{ConnectionTrait, Statement};

    let migrations: &[&str] = &[
        r#"CREATE EXTENSION IF NOT EXISTS "uuid-ossp""#,
        r#"CREATE EXTENSION IF NOT EXISTS pg_trgm"#,

        // Ingredients
        r#"
        CREATE TABLE IF NOT EXISTS ingredients (
            id          BIGSERIAL PRIMARY KEY,
            name        TEXT UNIQUE NOT NULL,
            category    TEXT,
            fdc_id      INTEGER,
            off_id      TEXT,
            created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
        "#,
        r#"CREATE INDEX IF NOT EXISTS idx_ingredients_name_trgm
            ON ingredients USING GIN (name gin_trgm_ops)"#,
        r#"CREATE INDEX IF NOT EXISTS idx_ingredients_category
            ON ingredients(category)"#,
        r#"CREATE INDEX IF NOT EXISTS idx_ingredients_fdc_id
            ON ingredients(fdc_id) WHERE fdc_id IS NOT NULL"#,

        // Ingredient Nutrients
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
        )
        "#,
        r#"CREATE INDEX IF NOT EXISTS idx_ingredient_nutrients_ingredient
            ON ingredient_nutrients(ingredient_id)"#,

        // Ingredient Allergens
        r#"
        CREATE TABLE IF NOT EXISTS ingredient_allergens (
            id              BIGSERIAL PRIMARY KEY,
            ingredient_id   BIGINT NOT NULL REFERENCES ingredients(id) ON DELETE CASCADE,
            allergen        TEXT NOT NULL,
            severity        TEXT NOT NULL DEFAULT 'contains',
            UNIQUE(ingredient_id, allergen)
        )
        "#,
        r#"CREATE INDEX IF NOT EXISTS idx_ingredient_allergens_ingredient
            ON ingredient_allergens(ingredient_id)"#,
        r#"CREATE INDEX IF NOT EXISTS idx_ingredient_allergens_allergen
            ON ingredient_allergens(allergen)"#,

        // Portion Sizes
        r#"
        CREATE TABLE IF NOT EXISTS portion_sizes (
            id              BIGSERIAL PRIMARY KEY,
            ingredient_id   BIGINT NOT NULL REFERENCES ingredients(id) ON DELETE CASCADE,
            description     TEXT NOT NULL,
            weight_grams    NUMERIC(10,3) NOT NULL,
            unit            TEXT
        )
        "#,
        r#"CREATE INDEX IF NOT EXISTS idx_portion_sizes_ingredient
            ON portion_sizes(ingredient_id)"#,

        // Recipes
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
            author_id       UUID,
            is_public       BOOLEAN NOT NULL DEFAULT TRUE,
            created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
        "#,
        r#"CREATE INDEX IF NOT EXISTS idx_recipes_name_trgm
            ON recipes USING GIN (name gin_trgm_ops)"#,
        r#"CREATE INDEX IF NOT EXISTS idx_recipes_cuisine    ON recipes(cuisine)"#,
        r#"CREATE INDEX IF NOT EXISTS idx_recipes_category   ON recipes(category)"#,
        r#"CREATE INDEX IF NOT EXISTS idx_recipes_difficulty ON recipes(difficulty)"#,
        r#"CREATE INDEX IF NOT EXISTS idx_recipes_dietary
            ON recipes(is_vegetarian, is_vegan, is_gluten_free, is_dairy_free, is_nut_free)"#,

        // Recipe Ingredients
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
        )
        "#,
        r#"CREATE INDEX IF NOT EXISTS idx_recipe_ingredients_recipe
            ON recipe_ingredients(recipe_id)"#,
        r#"CREATE INDEX IF NOT EXISTS idx_recipe_ingredients_ingredient
            ON recipe_ingredients(ingredient_id)"#,

        // Recipe Steps
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
        )
        "#,
        r#"CREATE INDEX IF NOT EXISTS idx_recipe_steps_recipe
            ON recipe_steps(recipe_id)"#,

        // Recipe Images
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
        )
        "#,
        r#"CREATE INDEX IF NOT EXISTS idx_recipe_images_recipe
            ON recipe_images(recipe_id)"#,
        r#"CREATE INDEX IF NOT EXISTS idx_recipe_images_primary
            ON recipe_images(recipe_id, is_primary) WHERE is_primary = TRUE"#,

        // Recipe Nutrition
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
        )
        "#,
        r#"CREATE INDEX IF NOT EXISTS idx_recipe_nutrition_recipe
            ON recipe_nutrition(recipe_id)"#,

        // API Keys
        r#"
        CREATE TABLE IF NOT EXISTS api_keys (
            id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
            name            TEXT NOT NULL,
            key_hash        TEXT NOT NULL,
            tier            TEXT NOT NULL DEFAULT 'free',
            rate_limit_rpm  INTEGER NOT NULL DEFAULT 60,
            monthly_usage   BIGINT NOT NULL DEFAULT 0,
            monthly_limit   BIGINT NOT NULL DEFAULT 10000,
            is_active       BOOLEAN NOT NULL DEFAULT TRUE,
            created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            last_used_at    TIMESTAMPTZ
        )
        "#,
        r#"CREATE INDEX IF NOT EXISTS idx_api_keys_hash ON api_keys(key_hash)"#,
    ];

    for sql in migrations {
        db.execute(Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            sql.to_string(),
        ))
        .await
        .expect("Failed to run food migration");
    }

    tracing::info!("All food migrations complete");

    // Initialize services
    let recipe_service = Arc::new(RecipeService::new(db.clone()));
    let ingredient_service = Arc::new(IngredientService::new(db.clone()));

    tracing::info!("Food API starting on {}", bind_address);

    HttpServer::new(move || {
        let cors_base = if cors_origin == "*" {
            Cors::default().send_wildcard()
        } else {
            Cors::default()
                .allowed_origin(&cors_origin)
                .allowed_origin_fn(|origin, _| {
                    if let Ok(s) = std::str::from_utf8(origin.as_bytes()) {
                        s.starts_with("http://localhost:") || s.starts_with("http://127.0.0.1:")
                    } else {
                        false
                    }
                })
        };
        let cors = cors_base
            .allowed_methods(vec!["GET", "POST", "PUT", "DELETE", "OPTIONS"])
            .allowed_headers(vec![
                actix_web::http::header::AUTHORIZATION,
                actix_web::http::header::ACCEPT,
                actix_web::http::header::CONTENT_TYPE,
                actix_web::http::header::HeaderName::from_static("x-api-key"),
            ])
            .max_age(3600);

        App::new()
            .app_data(web::JsonConfig::default().limit(10 * 1024 * 1024))
            .wrap(Logger::default())
            .wrap(cors)
            .wrap(SecurityHeaders)
            .app_data(web::Data::new(recipe_service.clone()))
            .app_data(web::Data::new(ingredient_service.clone()))
            .app_data(web::Data::new(db.clone()))
            // Health check
            .service(
                web::resource("/health")
                    .route(web::get().to(|| async {
                        actix_web::HttpResponse::Ok().json(serde_json::json!({
                            "status": "healthy",
                            "service": "cookest-food-api",
                            "version": "0.1.0"
                        }))
                    }))
            )
            // API v1 routes
            .configure(configure_ingredients)
            .configure(configure_recipes)
    })
    .bind(&bind_address)?
    .run()
    .await
}
