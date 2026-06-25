//! Admin ingredient proxy — forwards catalog CRUD to food-api after a JWT + DB
//! admin check. The master catalog lives in food-api; the admin dashboard manages
//! it through these endpoints.
//!
//! Routes (admin-only, behind JwtAuth):
//!   GET    /admin/ingredients?q=&category=&page=&per_page=
//!   GET    /admin/ingredients/categories
//!   GET    /admin/ingredients/{id}
//!   POST   /admin/ingredients
//!   PUT    /admin/ingredients/{id}
//!   DELETE /admin/ingredients/{id}

use actix_web::{web, HttpRequest, HttpResponse};
use sea_orm::EntityTrait;
use uuid::Uuid;

use crate::entity::user::Entity as User;
use crate::handlers::browse::FoodApiClient;
use crate::middleware::Claims;
use cookest_shared::errors::AppError;

async fn require_admin(db: &sea_orm::DatabaseConnection, claims: &Claims) -> Result<(), AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::Forbidden)?;
    let user = User::find_by_id(user_id)
        .one(db)
        .await?
        .ok_or_else(|| AppError::NotFound("User".to_string()))?;
    if !user.is_admin {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

/// Relay a food-api response straight back to the caller, preserving status code.
async fn relay(resp: reqwest::Response) -> Result<HttpResponse, AppError> {
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

/// GET /admin/ingredients — list/search the catalog (forwards the query string).
pub async fn list(
    food: web::Data<FoodApiClient>,
    db: web::Data<sea_orm::DatabaseConnection>,
    claims: web::ReqData<Claims>,
    req: HttpRequest,
) -> Result<HttpResponse, AppError> {
    require_admin(&db, &claims).await?;
    let qs = req.query_string();
    let url = if qs.is_empty() {
        format!("{}/api/v1/ingredients", food.base_url)
    } else {
        format!("{}/api/v1/ingredients?{}", food.base_url, qs)
    };
    let resp = food
        .client
        .get(url)
        .header("X-API-Key", food.api_key.as_deref().unwrap_or(""))
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("food-api list error: {}", e)))?;
    relay(resp).await
}

/// GET /admin/ingredients/categories
pub async fn categories(
    food: web::Data<FoodApiClient>,
    db: web::Data<sea_orm::DatabaseConnection>,
    claims: web::ReqData<Claims>,
) -> Result<HttpResponse, AppError> {
    require_admin(&db, &claims).await?;
    let resp = food
        .client
        .get(format!("{}/api/v1/ingredients/categories", food.base_url))
        .header("X-API-Key", food.api_key.as_deref().unwrap_or(""))
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("food-api categories error: {}", e)))?;
    relay(resp).await
}

/// GET /admin/ingredients/{id} — full detail (for the edit modal).
pub async fn detail(
    food: web::Data<FoodApiClient>,
    db: web::Data<sea_orm::DatabaseConnection>,
    claims: web::ReqData<Claims>,
    path: web::Path<i64>,
) -> Result<HttpResponse, AppError> {
    require_admin(&db, &claims).await?;
    let id = path.into_inner();
    let resp = food
        .client
        .get(format!("{}/api/v1/ingredients/{}", food.base_url, id))
        .header("X-API-Key", food.api_key.as_deref().unwrap_or(""))
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("food-api detail error: {}", e)))?;
    relay(resp).await
}

/// GET /admin/ingredients/import/scan?folder=... — list importable files in a folder.
pub async fn import_scan(
    food: web::Data<FoodApiClient>,
    db: web::Data<sea_orm::DatabaseConnection>,
    claims: web::ReqData<Claims>,
    req: HttpRequest,
) -> Result<HttpResponse, AppError> {
    require_admin(&db, &claims).await?;
    let qs = req.query_string();
    let url = if qs.is_empty() {
        format!("{}/api/v1/admin/import/scan", food.base_url)
    } else {
        format!("{}/api/v1/admin/import/scan?{}", food.base_url, qs)
    };
    let resp = food
        .client
        .get(url)
        .header("X-API-Key", food.api_key.as_deref().unwrap_or(""))
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("food-api import scan error: {}", e)))?;
    relay(resp).await
}

/// POST /admin/ingredients/import/execute — import an ingredient-list file from a folder.
pub async fn import_execute(
    food: web::Data<FoodApiClient>,
    db: web::Data<sea_orm::DatabaseConnection>,
    claims: web::ReqData<Claims>,
    body: web::Json<serde_json::Value>,
) -> Result<HttpResponse, AppError> {
    require_admin(&db, &claims).await?;
    let resp = food
        .client
        .post(format!("{}/api/v1/admin/import/ingredients", food.base_url))
        .header("Content-Type", "application/json")
        .header("X-API-Key", food.api_key.as_deref().unwrap_or(""))
        .json(&body.into_inner())
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("food-api import error: {}", e)))?;
    relay(resp).await
}

/// POST /admin/ingredients
pub async fn create(
    food: web::Data<FoodApiClient>,
    db: web::Data<sea_orm::DatabaseConnection>,
    claims: web::ReqData<Claims>,
    body: web::Json<serde_json::Value>,
) -> Result<HttpResponse, AppError> {
    require_admin(&db, &claims).await?;
    let resp = food
        .client
        .post(format!("{}/api/v1/admin/ingredients", food.base_url))
        .header("Content-Type", "application/json")
        .header("X-API-Key", food.api_key.as_deref().unwrap_or(""))
        .json(&body.into_inner())
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("food-api create error: {}", e)))?;
    relay(resp).await
}

/// PUT /admin/ingredients/{id}
pub async fn update(
    food: web::Data<FoodApiClient>,
    db: web::Data<sea_orm::DatabaseConnection>,
    claims: web::ReqData<Claims>,
    path: web::Path<i64>,
    body: web::Json<serde_json::Value>,
) -> Result<HttpResponse, AppError> {
    require_admin(&db, &claims).await?;
    let id = path.into_inner();
    let resp = food
        .client
        .put(format!("{}/api/v1/admin/ingredients/{}", food.base_url, id))
        .header("Content-Type", "application/json")
        .header("X-API-Key", food.api_key.as_deref().unwrap_or(""))
        .json(&body.into_inner())
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("food-api update error: {}", e)))?;
    relay(resp).await
}

/// DELETE /admin/ingredients/{id}
pub async fn delete(
    food: web::Data<FoodApiClient>,
    db: web::Data<sea_orm::DatabaseConnection>,
    claims: web::ReqData<Claims>,
    path: web::Path<i64>,
) -> Result<HttpResponse, AppError> {
    require_admin(&db, &claims).await?;
    let id = path.into_inner();
    let resp = food
        .client
        .delete(format!("{}/api/v1/admin/ingredients/{}", food.base_url, id))
        .header("X-API-Key", food.api_key.as_deref().unwrap_or(""))
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("food-api delete error: {}", e)))?;
    relay(resp).await
}

/// `/categories` is registered before `/{id}` so it isn't captured as an id.
pub fn configure_admin_ingredients(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/admin/ingredients")
            .route("", web::get().to(list))
            .route("", web::post().to(create))
            .route("/categories", web::get().to(categories))
            .route("/import/scan", web::get().to(import_scan))
            .route("/import/execute", web::post().to(import_execute))
            .route("/{id}", web::get().to(detail))
            .route("/{id}", web::put().to(update))
            .route("/{id}", web::delete().to(delete)),
    );
}
