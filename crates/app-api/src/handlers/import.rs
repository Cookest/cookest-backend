//! Import proxy — forwards admin import requests to food-api after JWT verification
//!
//! Routes (admin-only, behind JwtAuth middleware):
//!   GET  /api/admin/database/import/scan?folder=...
//!   POST /api/admin/database/import/execute

use actix_web::{web, HttpResponse};
use serde::Deserialize;
use uuid::Uuid;
use cookest_shared::errors::AppError;
use crate::handlers::browse::FoodApiClient;
use crate::middleware::Claims;
use crate::entity::user::Entity as User;
use sea_orm::EntityTrait;

#[derive(Deserialize)]
pub struct ScanQuery {
    pub folder: Option<String>,
}

#[derive(Deserialize)]
pub struct ExecuteBody {
    pub folder: Option<String>,
    pub filename: String,
    pub format: Option<String>,
}

/// Verify that the authenticated user is an admin by checking DB.
async fn require_admin(
    db: &sea_orm::DatabaseConnection,
    claims: &Claims,
) -> Result<(), AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Forbidden)?;
    let user = User::find_by_id(user_id)
        .one(db)
        .await?
        .ok_or_else(|| AppError::NotFound("User".to_string()))?;
    if !user.is_admin {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

/// GET /api/admin/database/import/scan
pub async fn scan(
    food: web::Data<FoodApiClient>,
    db: web::Data<sea_orm::DatabaseConnection>,
    query: web::Query<ScanQuery>,
    claims: web::ReqData<Claims>,
) -> Result<HttpResponse, AppError> {
    require_admin(&db, &claims).await?;
    let folder = query.folder.as_deref().unwrap_or("/data/imports");
    let url = format!(
        "/api/v1/admin/import/scan?folder={}",
        urlencoding::encode(folder)
    );
    let resp = food
        .get(&url)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("food-api scan error: {}", e)))?;
    let status = actix_web::http::StatusCode::from_u16(resp.status().as_u16())
        .unwrap_or(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR);
    let body = resp
        .bytes()
        .await
        .map_err(|e| AppError::Internal(format!("food-api read error: {}", e)))?;
    Ok(HttpResponse::build(status)
        .content_type("application/json")
        .body(body))
}

/// POST /api/admin/database/import/execute
pub async fn execute(
    food: web::Data<FoodApiClient>,
    db: web::Data<sea_orm::DatabaseConnection>,
    claims: web::ReqData<Claims>,
    body: web::Json<ExecuteBody>,
) -> Result<HttpResponse, AppError> {
    require_admin(&db, &claims).await?;
    let payload = serde_json::json!({
        "folder": body.folder.as_deref().unwrap_or("/data/imports"),
        "filename": body.filename,
        "format": body.format,
    });
    let resp = food
        .client
        .post(format!("{}/api/v1/admin/import/execute", food.base_url))
        .header("Content-Type", "application/json")
        .header("X-API-Key", food.api_key.as_deref().unwrap_or(""))
        .json(&payload)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("food-api execute error: {}", e)))?;
    let status = actix_web::http::StatusCode::from_u16(resp.status().as_u16())
        .unwrap_or(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR);
    let body_bytes = resp
        .bytes()
        .await
        .map_err(|e| AppError::Internal(format!("food-api read error: {}", e)))?;
    Ok(HttpResponse::build(status)
        .content_type("application/json")
        .body(body_bytes))
}

pub fn configure_import_proxy(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/admin/database/import")
            .route("/scan", web::get().to(scan))
            .route("/execute", web::post().to(execute)),
    );
}
