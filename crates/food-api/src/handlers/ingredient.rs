//! Ingredient HTTP handlers
//!
//! Public routes:
//!   GET /api/v1/ingredients             — list/search ingredients
//!   GET /api/v1/ingredients/categories  — distinct category list
//!   GET /api/v1/ingredients/:id         — full detail with nutrients and portions
//!
//! Admin routes (no JWT here; admin verified by the app-api proxy, like import.rs):
//!   POST   /api/v1/admin/ingredients      — create a catalog ingredient
//!   PUT    /api/v1/admin/ingredients/:id  — update a catalog ingredient
//!   DELETE /api/v1/admin/ingredients/:id  — delete (409 if used by a recipe)

use actix_web::{web, HttpResponse};
use std::sync::Arc;

use crate::errors::AppError;
use crate::models::ingredient::{CreateIngredientRequest, IngredientQuery, UpdateIngredientRequest};
use crate::services::IngredientService;

/// GET /api/v1/ingredients?q=chicken&category=protein
pub async fn search_ingredients(
    ingredient_service: web::Data<Arc<IngredientService>>,
    query: web::Query<IngredientQuery>,
) -> Result<HttpResponse, AppError> {
    let result = ingredient_service.search(query.into_inner()).await?;
    Ok(HttpResponse::Ok().json(result))
}

/// GET /api/v1/ingredients/:id
pub async fn get_ingredient(
    ingredient_service: web::Data<Arc<IngredientService>>,
    path: web::Path<i64>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();
    let ingredient = ingredient_service.get_ingredient(id).await?;
    Ok(HttpResponse::Ok().json(ingredient))
}

/// GET /api/v1/ingredients/categories
pub async fn list_categories(
    ingredient_service: web::Data<Arc<IngredientService>>,
) -> Result<HttpResponse, AppError> {
    let categories = ingredient_service.list_categories().await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "categories": categories })))
}

/// GET /api/v1/ingredients/barcode/:code
pub async fn get_ingredient_by_barcode(
    ingredient_service: web::Data<Arc<IngredientService>>,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let code = path.into_inner();
    let ingredient = ingredient_service.get_by_barcode(&code).await?;
    Ok(HttpResponse::Ok().json(ingredient))
}

/// POST /api/v1/admin/ingredients
pub async fn create_ingredient(
    ingredient_service: web::Data<Arc<IngredientService>>,
    body: web::Json<CreateIngredientRequest>,
) -> Result<HttpResponse, AppError> {
    let created = ingredient_service.create_ingredient(body.into_inner()).await?;
    Ok(HttpResponse::Created().json(created))
}

/// PUT /api/v1/admin/ingredients/:id
pub async fn update_ingredient(
    ingredient_service: web::Data<Arc<IngredientService>>,
    path: web::Path<i64>,
    body: web::Json<UpdateIngredientRequest>,
) -> Result<HttpResponse, AppError> {
    let updated = ingredient_service
        .update_ingredient(path.into_inner(), body.into_inner())
        .await?;
    Ok(HttpResponse::Ok().json(updated))
}

/// DELETE /api/v1/admin/ingredients/:id
pub async fn delete_ingredient(
    ingredient_service: web::Data<Arc<IngredientService>>,
    path: web::Path<i64>,
) -> Result<HttpResponse, AppError> {
    ingredient_service.delete_ingredient(path.into_inner()).await?;
    Ok(HttpResponse::NoContent().finish())
}

/// Configure public ingredient routes.
/// `/categories` is registered before `/{id}` so it isn't captured as an id.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/v1/ingredients")
            .route("", web::get().to(search_ingredients))
            .route("/categories", web::get().to(list_categories))
            .route("/barcode/{code}", web::get().to(get_ingredient_by_barcode))
            .route("/{id}", web::get().to(get_ingredient)),
    );
}

/// Configure admin ingredient routes (catalog management).
pub fn configure_admin(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/v1/admin/ingredients")
            .route("", web::post().to(create_ingredient))
            .route("/{id}", web::put().to(update_ingredient))
            .route("/{id}", web::delete().to(delete_ingredient)),
    );
}
