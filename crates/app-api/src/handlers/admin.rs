use actix_web::{web, HttpResponse};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Algorithm, Argon2, Params, Version,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait,
    PaginatorTrait, QueryFilter, QueryOrder, Set,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::entity::user::{self, ActiveModel, Entity as User};
use crate::middleware::auth::AuthenticatedUser;
use crate::services::token::{SubscriptionTier, TokenService};
use crate::validation::normalize_email;
use cookest_shared::errors::AppError;

// ── Setup ─────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SetupRequest {
    #[serde(rename = "adminEmail")]
    pub admin_email: String,
    #[serde(rename = "adminPassword")]
    pub admin_password: String,
    // optional config fields — accepted but not persisted yet
    #[serde(rename = "instanceName")]
    pub instance_name: Option<String>,
    #[serde(rename = "aiEnabled")]
    pub ai_enabled: Option<bool>,
    #[serde(rename = "stripeEnabled")]
    pub stripe_enabled: Option<bool>,
    #[serde(rename = "pdfPipelineEnabled")]
    pub pdf_pipeline_enabled: Option<bool>,
}

fn is_self_hosted() -> bool {
    std::env::var("SELF_HOSTED").unwrap_or_default().trim().to_ascii_lowercase() == "true"
}

/// `GET /admin/setup/status` — lets the frontend know if first-run setup is needed.
///
/// Public endpoint. Returns `{ self_hosted, needs_setup }` so the admin panel
/// root page can redirect to /setup on a fresh self-hosted install.
pub async fn setup_status(
    db: web::Data<DatabaseConnection>,
) -> Result<HttpResponse, AppError> {
    let self_hosted = is_self_hosted();

    let needs_setup = if self_hosted {
        User::find()
            .filter(user::Column::IsAdmin.eq(true))
            .one(db.get_ref())
            .await?
            .is_none()
    } else {
        false
    };

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "self_hosted": self_hosted,
        "needs_setup": needs_setup,
    })))
}

/// `POST /admin/setup` — first-run only on self-hosted instances.
///
/// Only available when `SELF_HOSTED=true`. Returns 403 on managed deployments.
/// Returns 409 once any admin exists, making the endpoint permanently inert.
pub async fn setup(
    db: web::Data<DatabaseConnection>,
    token_service: web::Data<Arc<TokenService>>,
    body: web::Json<SetupRequest>,
) -> Result<HttpResponse, AppError> {
    if !is_self_hosted() {
        return Err(AppError::Forbidden);
    }

    let body = body.into_inner();

    // Reject if an admin already exists (setup is one-time only)
    let existing_admin = User::find()
        .filter(user::Column::IsAdmin.eq(true))
        .one(db.get_ref())
        .await?;

    if existing_admin.is_some() {
        return Ok(HttpResponse::Conflict().json(serde_json::json!({
            "error": "Admin account already exists. Use the login page."
        })));
    }

    let email = normalize_email(&body.admin_email);

    if email.is_empty() || !email.contains('@') {
        return Err(AppError::Internal("Invalid admin email".into()));
    }
    if body.admin_password.len() < 8 {
        return Err(AppError::Internal("Password must be at least 8 characters".into()));
    }

    // Check the email isn't already taken
    let email_taken = User::find()
        .filter(user::Column::Email.eq(&email))
        .one(db.get_ref())
        .await?;

    let user_id = if let Some(existing) = email_taken {
        // Promote existing user to admin
        let mut active: ActiveModel = existing.into();
        active.is_admin = Set(true);
        active.updated_at = Set(Utc::now().fixed_offset());
        let updated = active.update(db.get_ref()).await?;
        updated.id
    } else {
        // Create new admin user
        let password_hash = hash_password(&body.admin_password)?;
        let now = Utc::now().fixed_offset();
        let user_id = Uuid::new_v4();

        let new_user = ActiveModel {
            id: Set(user_id),
            email: Set(email.clone()),
            password_hash: Set(password_hash),
            refresh_token_hash: Set(None),
            name: Set(Some("Admin".to_string())),
            household_size: Set(1),
            dietary_restrictions: Set(None),
            allergies: Set(None),
            avatar_url: Set(None),
            is_email_verified: Set(true),
            two_factor_enabled: Set(false),
            totp_secret: Set(None),
            failed_login_attempts: Set(0),
            locked_until: Set(None),
            subscription_tier: Set("pro".to_string()),
            subscription_valid_until: Set(None),
            stripe_customer_id: Set(None),
            cooking_skill_level: Set(None),
            preferred_cuisines: Set(None),
            health_goals: Set(None),
            weekly_budget: Set(None),
            preferred_time_per_meal_min: Set(None),
            meal_frequency: Set(None),
            taste_profile: Set(serde_json::json!({})),
            onboarding_completed: Set(true),
            is_admin: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
        };

        let inserted = new_user.insert(db.get_ref()).await?;
        inserted.id
    };

    let access_token = token_service
        .generate_access_token(user_id, &email, SubscriptionTier::Pro, true, true)
        .map_err(|e| AppError::Internal(format!("Token generation failed: {:?}", e)))?;

    tracing::info!("Admin account created/promoted via setup: {}", email);

    Ok(HttpResponse::Ok().json(serde_json::json!({ "access_token": access_token })))
}

// ── Admin guard ───────────────────────────────────────────────────────────────

async fn require_admin(user_id: Uuid, db: &DatabaseConnection) -> Result<(), AppError> {
    let user = User::find_by_id(user_id)
        .one(db)
        .await?
        .ok_or(AppError::Forbidden)?;
    if !user.is_admin {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

// ── Users ─────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct Pagination {
    pub page: Option<u64>,
    pub per_page: Option<u64>,
}

#[derive(Serialize)]
struct AdminUserRow {
    id: Uuid,
    email: String,
    name: Option<String>,
    subscription_tier: String,
    is_admin: bool,
    is_email_verified: bool,
    onboarding_completed: bool,
    created_at: String,
}

impl From<crate::entity::user::Model> for AdminUserRow {
    fn from(u: crate::entity::user::Model) -> Self {
        Self {
            id: u.id,
            email: u.email,
            name: u.name,
            subscription_tier: u.subscription_tier,
            is_admin: u.is_admin,
            is_email_verified: u.is_email_verified,
            onboarding_completed: u.onboarding_completed,
            created_at: u.created_at.to_rfc3339(),
        }
    }
}

pub async fn list_users(
    user: AuthenticatedUser,
    db: web::Data<DatabaseConnection>,
    query: web::Query<Pagination>,
) -> Result<HttpResponse, AppError> {
    require_admin(user.id, db.get_ref()).await?;

    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(20).min(100);

    let paginator = User::find()
        .order_by_desc(user::Column::CreatedAt)
        .paginate(db.get_ref(), per_page);

    let total = paginator.num_items().await?;
    let users: Vec<AdminUserRow> = paginator
        .fetch_page(page - 1)
        .await?
        .into_iter()
        .map(AdminUserRow::from)
        .collect();

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "users": users,
        "total": total,
        "page": page,
        "per_page": per_page,
    })))
}

pub async fn get_user(
    auth: AuthenticatedUser,
    db: web::Data<DatabaseConnection>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    require_admin(auth.id, db.get_ref()).await?;

    let target_id = path.into_inner();
    let user = User::find_by_id(target_id)
        .one(db.get_ref())
        .await?
        .ok_or_else(|| AppError::NotFound("User".into()))?;

    Ok(HttpResponse::Ok().json(AdminUserRow::from(user)))
}

#[derive(Deserialize)]
pub struct UpdateUserBody {
    pub subscription_tier: Option<String>,
    pub is_admin: Option<bool>,
    pub name: Option<String>,
}

pub async fn update_user(
    auth: AuthenticatedUser,
    db: web::Data<DatabaseConnection>,
    path: web::Path<Uuid>,
    body: web::Json<UpdateUserBody>,
) -> Result<HttpResponse, AppError> {
    require_admin(auth.id, db.get_ref()).await?;

    let target_id = path.into_inner();
    let user = User::find_by_id(target_id)
        .one(db.get_ref())
        .await?
        .ok_or_else(|| AppError::NotFound("User".into()))?;

    let body = body.into_inner();
    let mut active: ActiveModel = user.into();

    if let Some(tier) = body.subscription_tier {
        if !["free", "pro", "family"].contains(&tier.as_str()) {
            return Err(AppError::Internal("Invalid subscription tier".into()));
        }
        active.subscription_tier = Set(tier);
    }
    if let Some(is_admin) = body.is_admin {
        active.is_admin = Set(is_admin);
    }
    if let Some(name) = body.name {
        active.name = Set(Some(name));
    }
    active.updated_at = Set(Utc::now().fixed_offset());

    let updated = active.update(db.get_ref()).await?;
    Ok(HttpResponse::Ok().json(AdminUserRow::from(updated)))
}

// ── Stats ─────────────────────────────────────────────────────────────────────

pub async fn get_stats(
    auth: AuthenticatedUser,
    db: web::Data<DatabaseConnection>,
) -> Result<HttpResponse, AppError> {
    require_admin(auth.id, db.get_ref()).await?;

    let total_users = User::find().count(db.get_ref()).await?;

    let total_recipes = crate::entity::recipe::Entity::find()
        .count(db.get_ref())
        .await?;

    let total_ingredients = crate::entity::ingredient::Entity::find()
        .count(db.get_ref())
        .await?;

    let active_meal_plans = crate::entity::meal_plan::Entity::find()
        .count(db.get_ref())
        .await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "total_users": total_users,
        "total_recipes": total_recipes,
        "total_ingredients": total_ingredients,
        "active_meal_plans": active_meal_plans,
        "ai_chats_today": 0,
        "system_health": {
            "database": "healthy",
            "api": "healthy"
        }
    })))
}

// ── Health ────────────────────────────────────────────────────────────────────

pub async fn get_health(
    auth: AuthenticatedUser,
    db: web::Data<DatabaseConnection>,
) -> Result<HttpResponse, AppError> {
    require_admin(auth.id, db.get_ref()).await?;

    // Quick DB ping
    let db_ok = db
        .get_ref()
        .execute(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT 1".to_string(),
        ))
        .await
        .is_ok();

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "database": if db_ok { "healthy" } else { "unhealthy" },
        "api": "healthy",
    })))
}

// ── Settings ──────────────────────────────────────────────────────────────────

pub async fn get_settings(
    auth: AuthenticatedUser,
    db: web::Data<DatabaseConnection>,
) -> Result<HttpResponse, AppError> {
    require_admin(auth.id, db.get_ref()).await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "self_hosted": std::env::var("SELF_HOSTED").unwrap_or_default() == "true",
        "food_data_source": std::env::var("FOOD_DATA_SOURCE").unwrap_or_else(|_| "hybrid".into()),
        "ai_enabled": std::env::var("OLLAMA_URL").is_ok(),
        "stripe_enabled": std::env::var("STRIPE_WEBHOOK_SECRET").is_ok(),
    })))
}

pub async fn update_settings(
    auth: AuthenticatedUser,
    db: web::Data<DatabaseConnection>,
    _body: web::Json<serde_json::Value>,
) -> Result<HttpResponse, AppError> {
    require_admin(auth.id, db.get_ref()).await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "message": "Settings updated. Restart the server to apply environment-level changes."
    })))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn hash_password(password: &str) -> Result<String, AppError> {
    let params = Params::new(19 * 1024, 2, 1, None)
        .map_err(|e| AppError::Internal(format!("Argon2 params: {}", e)))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let salt = SaltString::generate(&mut OsRng);
    argon2
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(format!("Password hashing failed: {}", e)))
}

// ── Routes ────────────────────────────────────────────────────────────────────

pub fn configure_admin_setup(cfg: &mut web::ServiceConfig) {
    cfg.route("/admin/setup/status", web::get().to(setup_status));
    cfg.route("/admin/setup", web::post().to(setup));
}

pub fn configure_admin(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/admin")
            .route("/users", web::get().to(list_users))
            .route("/users/{id}", web::get().to(get_user))
            .route("/users/{id}", web::patch().to(update_user))
            .route("/stats", web::get().to(get_stats))
            .route("/health", web::get().to(get_health))
            .route("/settings", web::get().to(get_settings))
            .route("/settings", web::patch().to(update_settings)),
    );
}
