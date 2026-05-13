//! Recipe HTTP handlers
//!
//! All endpoints are API-key-gated (handled by middleware).
//! No user-specific features — no JWT, no AuthenticatedUser.
//!
//! Routes:
//!   GET    /api/v1/recipes                     — list with filters + pagination
//!   GET    /api/v1/recipes/random              — random recipe picker
//!   GET    /api/v1/recipes/by-ingredient       — recipes containing an ingredient
//!   GET    /api/v1/recipes/slug/:slug          — full detail by slug
//!   GET    /api/v1/recipes/:id                 — full detail by ID
//!   GET    /api/v1/recipes/:id/scale           — scale ingredient quantities
//!   POST   /api/v1/recipes                     — create (write-tier API keys)
//!   PUT    /api/v1/recipes/:id                 — update
//!   DELETE /api/v1/recipes/:id                 — delete
//!   GET    /api/v1/stats                       — database statistics
//!   GET    /api/v1/cuisines                    — distinct cuisine list
//!   GET    /api/v1/categories                  — distinct category list

use actix_web::{web, HttpResponse};
use std::sync::Arc;

use crate::errors::AppError;
use crate::models::recipe::{
    RecipeQuery, CreateRecipeRequest, UpdateRecipeRequest,
    ScaleRequest, RandomQuery, ByIngredientQuery,
};
use crate::services::RecipeService;

/// GET /api/v1/recipes
pub async fn list_recipes(
    recipe_service: web::Data<Arc<RecipeService>>,
    query: web::Query<RecipeQuery>,
) -> Result<HttpResponse, AppError> {
    let result = recipe_service.list_recipes(query.into_inner()).await?;
    Ok(HttpResponse::Ok().json(result))
}

/// GET /api/v1/recipes/random
pub async fn random_recipes(
    recipe_service: web::Data<Arc<RecipeService>>,
    query: web::Query<RandomQuery>,
) -> Result<HttpResponse, AppError> {
    let result = recipe_service.random_recipes(query.into_inner()).await?;
    Ok(HttpResponse::Ok().json(result))
}

/// GET /api/v1/recipes/by-ingredient?name=chicken
pub async fn recipes_by_ingredient(
    recipe_service: web::Data<Arc<RecipeService>>,
    query: web::Query<ByIngredientQuery>,
) -> Result<HttpResponse, AppError> {
    let result = recipe_service.recipes_by_ingredient(query.into_inner()).await?;
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

/// GET /api/v1/recipes/:id/scale?servings=4
pub async fn scale_recipe(
    recipe_service: web::Data<Arc<RecipeService>>,
    path: web::Path<i64>,
    query: web::Query<ScaleRequest>,
) -> Result<HttpResponse, AppError> {
    let result = recipe_service.scale_recipe(path.into_inner(), query.servings).await?;
    Ok(HttpResponse::Ok().json(result))
}

/// GET /api/v1/stats
pub async fn get_stats(
    recipe_service: web::Data<Arc<RecipeService>>,
) -> Result<HttpResponse, AppError> {
    let stats = recipe_service.get_stats().await?;
    Ok(HttpResponse::Ok().json(stats))
}

/// GET /api/v1/cuisines
pub async fn list_cuisines(
    recipe_service: web::Data<Arc<RecipeService>>,
) -> Result<HttpResponse, AppError> {
    let cuisines = recipe_service.get_cuisines().await?;
    Ok(HttpResponse::Ok().json(cuisines))
}

/// GET /api/v1/categories
pub async fn list_categories(
    recipe_service: web::Data<Arc<RecipeService>>,
) -> Result<HttpResponse, AppError> {
    let categories = recipe_service.get_categories().await?;
    Ok(HttpResponse::Ok().json(categories))
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

/// Configure all recipe and stats routes
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg
        // Stats and lookup endpoints (no :id conflict)
        .service(
            web::resource("/api/v1/stats")
                .route(web::get().to(get_stats))
        )
        .service(
            web::resource("/api/v1/cuisines")
                .route(web::get().to(list_cuisines))
        )
        .service(
            web::resource("/api/v1/categories")
                .route(web::get().to(list_categories))
        )
        // Recipe scope
        .service(
            web::scope("/api/v1/recipes")
                .route("",                    web::get().to(list_recipes))
                .route("",                    web::post().to(create_recipe))
                .route("/random",             web::get().to(random_recipes))
                .route("/by-ingredient",      web::get().to(recipes_by_ingredient))
                .route("/slug/{slug}",        web::get().to(get_recipe_by_slug))
                .route("/{id}",               web::get().to(get_recipe))
                .route("/{id}/scale",         web::get().to(scale_recipe))
                .route("/{id}",               web::put().to(update_recipe))
                .route("/{id}",               web::delete().to(delete_recipe)),
        );
}
