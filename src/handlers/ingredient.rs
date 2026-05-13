//! Ingredient HTTP handlers
//!
//! Routes:
//! GET /api/ingredients         — list/search ingredients
//! GET /api/ingredients/:id     — full detail with nutrients and portions

use actix_web::{web, HttpResponse};
use std::sync::Arc;

use crate::errors::AppError;
use crate::models::ingredient::IngredientQuery;
use crate::services::IngredientService;

/// GET /api/ingredients?q=chicken&category=protein
/// Search ingredients — used for inventory autocomplete
pub async fn search_ingredients(
    ingredient_service: web::Data<Arc<IngredientService>>,
    query: web::Query<IngredientQuery>,
) -> Result<HttpResponse, AppError> {
    let result = ingredient_service.search(query.into_inner()).await?;
    Ok(HttpResponse::Ok().json(result))
}

/// GET /api/ingredients/:id
/// Get full ingredient detail with nutrients and portion sizes
pub async fn get_ingredient(
    ingredient_service: web::Data<Arc<IngredientService>>,
    path: web::Path<i64>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();
    let ingredient = ingredient_service.get_ingredient(id).await?;
    Ok(HttpResponse::Ok().json(ingredient))
}

/// Configure ingredient routes
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/ingredients")
            .route("", web::get().to(search_ingredients))
            .route("/{id}", web::get().to(get_ingredient)),
    );
}
