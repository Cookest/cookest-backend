//! Recipe HTTP handlers
//!
//! All endpoints are API-key-gated (handled by middleware).
//! No user-specific features — no JWT, no AuthenticatedUser.
//!
//! Routes:
//!   GET    /api/v1/recipes            — list with filters + pagination
//!   GET    /api/v1/recipes/slug/:slug — full detail by slug
//!   GET    /api/v1/recipes/:id        — full detail by ID
//!   POST   /api/v1/recipes            — create (write-tier API keys)
//!   PUT    /api/v1/recipes/:id        — update
//!   DELETE /api/v1/recipes/:id        — delete

use actix_web::{web, HttpResponse};
use std::sync::Arc;

use crate::errors::AppError;
use crate::models::recipe::{RecipeQuery, CreateRecipeRequest, UpdateRecipeRequest};
use crate::services::RecipeService;

/// GET /api/v1/recipes
pub async fn list_recipes(
    recipe_service: web::Data<Arc<RecipeService>>,
    query: web::Query<RecipeQuery>,
) -> Result<HttpResponse, AppError> {
    let result = recipe_service.list_recipes(query.into_inner()).await?;
    Ok(HttpResponse::Ok().json(result))
}

/// GET /api/v1/recipes/:id
pub async fn get_recipe(
    recipe_service: web::Data<Arc<RecipeService>>,
    path: web::Path<i64>,
) -> Result<HttpResponse, AppError> {
    let recipe = recipe_service.get_recipe(path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(recipe))
}

/// GET /api/v1/recipes/slug/:slug
pub async fn get_recipe_by_slug(
    recipe_service: web::Data<Arc<RecipeService>>,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let recipe = recipe_service.get_recipe_by_slug(&path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(recipe))
}

/// POST /api/v1/recipes
pub async fn create_recipe(
    recipe_service: web::Data<Arc<RecipeService>>,
    body: web::Json<CreateRecipeRequest>,
) -> Result<HttpResponse, AppError> {
    let result = recipe_service.create_recipe(body.into_inner()).await?;
    Ok(HttpResponse::Created().json(result))
}

/// PUT /api/v1/recipes/:id
pub async fn update_recipe(
    recipe_service: web::Data<Arc<RecipeService>>,
    path: web::Path<i64>,
    body: web::Json<UpdateRecipeRequest>,
) -> Result<HttpResponse, AppError> {
    let result = recipe_service.update_recipe(path.into_inner(), body.into_inner()).await?;
    Ok(HttpResponse::Ok().json(result))
}

/// DELETE /api/v1/recipes/:id
pub async fn delete_recipe(
    recipe_service: web::Data<Arc<RecipeService>>,
    path: web::Path<i64>,
) -> Result<HttpResponse, AppError> {
    recipe_service.delete_recipe(path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "message": "Recipe deleted" })))
}

/// Configure all recipe routes
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/v1/recipes")
            .route("", web::get().to(list_recipes))
            .route("", web::post().to(create_recipe))
            .route("/slug/{slug}", web::get().to(get_recipe_by_slug))
            .route("/{id}", web::get().to(get_recipe))
            .route("/{id}", web::put().to(update_recipe))
            .route("/{id}", web::delete().to(delete_recipe)),
    );
}
