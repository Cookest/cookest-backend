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
use actix_multipart::Multipart;
use futures::StreamExt;
use std::sync::Arc;

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
    query: web::Query<crate::models::recipe::RecipeQuery>,
) -> Result<HttpResponse, AppError> {
    let user_id = user.id;
    let result = recipe_service.list_my_recipes(user_id, query.into_inner()).await?;
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
    if body.is_public.unwrap_or(false) {
        sub_service.check_published_recipes_limit(&user.claims, user.id).await?;
    }
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
    if body.is_public.unwrap_or(false) {
        sub_service.check_published_recipes_limit(&user.claims, user.id).await?;
    }
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

/// POST /api/recipes/:id/image — upload an image for the recipe
pub async fn upload_recipe_image(
    recipe_service: web::Data<Arc<RecipeService>>,
    user: AuthenticatedUser,
    path: web::Path<i64>,
    mut payload: Multipart,
) -> Result<HttpResponse, AppError> {
    let user_id = user.id;
    let recipe_id = path.into_inner();

    let mut image_bytes: Option<Vec<u8>> = None;
    let mut file_ext = "jpg".to_string();

    while let Some(field) = payload.next().await {
        let mut field = field.map_err(|e| AppError::Internal(format!("Multipart error: {}", e)))?;
        let content_disposition = field.content_disposition();
        let field_name = content_disposition.and_then(|cd| cd.get_name()).unwrap_or("");

        if field_name == "image" {
            if let Some(cd) = content_disposition {
                if let Some(filename) = cd.get_filename() {
                    if let Some(ext) = filename.split('.').last() {
                        file_ext = ext.to_string();
                    }
                }
            }

            let mut bytes = Vec::new();
            while let Some(chunk) = field.next().await {
                let chunk = chunk.map_err(|e| AppError::Internal(format!("Chunk error: {}", e)))?;
                bytes.extend_from_slice(&chunk);
                // Max 10 MB
                if bytes.len() > 10 * 1024 * 1024 {
                    return Err(AppError::Validation({
                        let mut errors = validator::ValidationErrors::new();
                        let mut e = validator::ValidationError::new("too_large");
                        e.message = Some("Image must be under 10 MB".into());
                        errors.add("image", e);
                        errors
                    }));
                }
            }
            if !bytes.is_empty() {
                image_bytes = Some(bytes);
            }
        }
    }

    let bytes = image_bytes.ok_or_else(|| AppError::Validation({
        let mut errors = validator::ValidationErrors::new();
        let mut e = validator::ValidationError::new("missing");
        e.message = Some("Image field is required".into());
        errors.add("image", e);
        errors
    }))?;

    let result = recipe_service.upload_recipe_image(user_id, recipe_id, bytes, &file_ext).await?;
    Ok(HttpResponse::Ok().json(result))
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
            .route("/{id}", web::delete().to(delete_recipe))
            .route("/{id}/image", web::post().to(upload_recipe_image)),
    );
}

/// No-op: all recipe routes consolidated into configure() above.
pub fn configure_protected(_cfg: &mut web::ServiceConfig) {}
