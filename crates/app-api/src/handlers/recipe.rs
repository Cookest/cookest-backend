//! Recipe HTTP handlers
//!
//! Public routes (no auth):
//!   GET  /api/recipes            — list with filters + pagination
//!   GET  /api/recipes/slug/:slug — full detail by slug
//!   GET  /api/recipes/:id        — full detail by ID
//!
//! Auth-gated routes (JWT required):
//!   GET  /api/recipes?match_inventory=true — list with inventory match % (any tier)
//!   GET  /api/recipes/mine                — user's own recipes
//!   POST /api/recipes                     — create recipe (Pro tier)
//!   PUT  /api/recipes/:id                 — update own recipe (Pro tier)
//!   DELETE /api/recipes/:id               — delete own recipe

use actix_web::{web, HttpResponse};
use std::sync::Arc;
use uuid::Uuid;

use cookest_shared::errors::AppError;
use crate::middleware::auth::AuthenticatedUser;
use crate::models::recipe::{RecipeQuery, CreateRecipeRequest, UpdateRecipeRequest};
use crate::services::{RecipeService, SubscriptionService};

/// GET /api/recipes
/// Public listing; if match_inventory=true AND auth header present, adds match percentages
pub async fn list_recipes(
    recipe_service: web::Data<Arc<RecipeService>>,
    query: web::Query<RecipeQuery>,
    user: Option<AuthenticatedUser>,
) -> Result<HttpResponse, AppError> {
    let q = query.into_inner();
    if q.match_inventory == Some(true) {
        let user_id = user.map(|u| u.id).ok_or(AppError::AuthenticationFailed)?;
        let result = recipe_service.list_recipes_with_inventory(user_id, q).await?;
        return Ok(HttpResponse::Ok().json(result));
    }
    let result = recipe_service.list_recipes(q).await?;
    Ok(HttpResponse::Ok().json(result))
}

/// GET /api/recipes/:id
pub async fn get_recipe(
    recipe_service: web::Data<Arc<RecipeService>>,
    path: web::Path<i64>,
) -> Result<HttpResponse, AppError> {
    let recipe = recipe_service.get_recipe(path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(recipe))
}

/// GET /api/recipes/slug/:slug
pub async fn get_recipe_by_slug(
    recipe_service: web::Data<Arc<RecipeService>>,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let recipe = recipe_service.get_recipe_by_slug(&path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(recipe))
}

/// GET /api/recipes/mine — list authenticated user's own recipes
pub async fn list_my_recipes(
    recipe_service: web::Data<Arc<RecipeService>>,
    user: AuthenticatedUser,
    query: web::Query<crate::models::recipe::PaginationQuery>,
) -> Result<HttpResponse, AppError> {
    let user_id = user.id;
    let page = query.page.unwrap_or(1);
    let per_page = query.per_page.unwrap_or(20);
    let result = recipe_service.list_my_recipes(user_id, page, per_page).await?;
    Ok(HttpResponse::Ok().json(result))
}

/// POST /api/recipes — Pro tier: create a recipe
pub async fn create_recipe(
    recipe_service: web::Data<Arc<RecipeService>>,
    sub_service: web::Data<Arc<SubscriptionService>>,
    user: AuthenticatedUser,
    body: web::Json<CreateRecipeRequest>,
) -> Result<HttpResponse, AppError> {
    let user_id = user.id;
    sub_service.require_pro(&user.claims).await?;
    let result = recipe_service.create_recipe(user_id, body.into_inner()).await?;
    Ok(HttpResponse::Created().json(result))
}

/// PUT /api/recipes/:id — update own recipe (Pro tier, author only)
pub async fn update_recipe(
    recipe_service: web::Data<Arc<RecipeService>>,
    sub_service: web::Data<Arc<SubscriptionService>>,
    user: AuthenticatedUser,
    path: web::Path<i64>,
    body: web::Json<UpdateRecipeRequest>,
) -> Result<HttpResponse, AppError> {
    let user_id = user.id;
    sub_service.require_pro(&user.claims).await?;
    let result = recipe_service.update_recipe(user_id, path.into_inner(), body.into_inner()).await?;
    Ok(HttpResponse::Ok().json(result))
}

/// DELETE /api/recipes/:id — delete own recipe (author only)
pub async fn delete_recipe(
    recipe_service: web::Data<Arc<RecipeService>>,
    user: AuthenticatedUser,
    path: web::Path<i64>,
) -> Result<HttpResponse, AppError> {
    let user_id = user.id;
    recipe_service.delete_recipe(user_id, path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "message": "Recipe deleted" })))
}

/// Configure all recipe routes in a single scope.
/// Public GET routes (list, slug, by-id) work without auth.
/// Write routes and /mine use AuthenticatedUser which self-validates the JWT.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/recipes")
            .route("", web::get().to(list_recipes))
            .route("", web::post().to(create_recipe))
            // /mine must be before /{id} to avoid wildcard capture
            .route("/mine", web::get().to(list_my_recipes))
            .route("/slug/{slug}", web::get().to(get_recipe_by_slug))
            .route("/{id}", web::get().to(get_recipe))
            .route("/{id}", web::put().to(update_recipe))
            .route("/{id}", web::delete().to(delete_recipe)),
    );
}

/// No-op: all recipe routes consolidated into configure() above.
pub fn configure_protected(_cfg: &mut web::ServiceConfig) {}
