//! Inventory, Profile, Interaction, and Meal Plan handlers

use actix_web::{web, HttpResponse};
use std::sync::Arc;
use uuid::Uuid;

use crate::errors::AppError;
use crate::models::inventory::{AddInventoryItem, UpdateInventoryItem};
use crate::models::profile::UpdateProfileRequest;
use crate::models::interaction::RateRecipeRequest;
use crate::models::meal_plan::GenerateMealPlanRequest;
use crate::services::{InventoryService, ProfileService, InteractionService, MealPlanService};
use crate::middleware::Claims;

// ── Inventory ────────────────────────────────────────────────────────────────

pub async fn list_inventory(
    inv: web::Data<Arc<InventoryService>>,
    claims: web::ReqData<Claims>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let items = inv.list(user_id).await?;
    Ok(HttpResponse::Ok().json(items))
}

pub async fn add_inventory_item(
    inv: web::Data<Arc<InventoryService>>,
    claims: web::ReqData<Claims>,
    body: web::Json<AddInventoryItem>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let item = inv.add(user_id, body.into_inner()).await?;
    Ok(HttpResponse::Created().json(item))
}

pub async fn update_inventory_item(
    inv: web::Data<Arc<InventoryService>>,
    claims: web::ReqData<Claims>,
    path: web::Path<i64>,
    body: web::Json<UpdateInventoryItem>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let item = inv.update(user_id, path.into_inner(), body.into_inner()).await?;
    Ok(HttpResponse::Ok().json(item))
}

pub async fn delete_inventory_item(
    inv: web::Data<Arc<InventoryService>>,
    claims: web::ReqData<Claims>,
    path: web::Path<i64>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    inv.delete(user_id, path.into_inner()).await?;
    Ok(HttpResponse::NoContent().finish())
}

pub async fn expiring_soon(
    inv: web::Data<Arc<InventoryService>>,
    claims: web::ReqData<Claims>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let items = inv.expiring_soon(user_id, 5).await?;
    Ok(HttpResponse::Ok().json(items))
}

// ── Profile ──────────────────────────────────────────────────────────────────

pub async fn get_profile(
    profile: web::Data<Arc<ProfileService>>,
    claims: web::ReqData<Claims>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let p = profile.get_profile(user_id).await?;
    Ok(HttpResponse::Ok().json(p))
}

pub async fn update_profile(
    profile: web::Data<Arc<ProfileService>>,
    claims: web::ReqData<Claims>,
    body: web::Json<UpdateProfileRequest>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let p = profile.update_profile(user_id, body.into_inner()).await?;
    Ok(HttpResponse::Ok().json(p))
}

// ── Interactions ─────────────────────────────────────────────────────────────

pub async fn rate_recipe(
    interaction: web::Data<Arc<InteractionService>>,
    claims: web::ReqData<Claims>,
    path: web::Path<i64>,
    body: web::Json<RateRecipeRequest>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let req = body.into_inner();
    let res = interaction.rate_recipe(user_id, path.into_inner(), req.rating, req.comment).await?;
    Ok(HttpResponse::Ok().json(res))
}

pub async fn toggle_favourite(
    interaction: web::Data<Arc<InteractionService>>,
    claims: web::ReqData<Claims>,
    path: web::Path<i64>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let res = interaction.toggle_favourite(user_id, path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(res))
}

pub async fn get_favourites(
    interaction: web::Data<Arc<InteractionService>>,
    claims: web::ReqData<Claims>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let favs = interaction.get_favourites(user_id).await?;
    Ok(HttpResponse::Ok().json(favs))
}

pub async fn mark_cooked(
    interaction: web::Data<Arc<InteractionService>>,
    profile: web::Data<Arc<ProfileService>>,
    claims: web::ReqData<Claims>,
    path: web::Path<i64>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let household_size = profile.get_profile(user_id).await?.household_size;
    let res = interaction.mark_cooked(user_id, path.into_inner(), household_size).await?;
    Ok(HttpResponse::Ok().json(res))
}

pub async fn get_cooking_history(
    interaction: web::Data<Arc<InteractionService>>,
    claims: web::ReqData<Claims>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let history = interaction.get_cooking_history(user_id).await?;
    Ok(HttpResponse::Ok().json(history))
}

// ── Meal Planning ─────────────────────────────────────────────────────────────

pub async fn generate_meal_plan(
    meal_svc: web::Data<Arc<MealPlanService>>,
    profile_svc: web::Data<Arc<ProfileService>>,
    claims: web::ReqData<Claims>,
    body: web::Json<GenerateMealPlanRequest>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let profile = profile_svc.get_profile(user_id).await?;
    let plan = meal_svc
        .generate_week_plan(user_id, profile.household_size, body.week_start)
        .await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "id": plan.id,
        "week_start": plan.week_start,
        "is_ai_generated": plan.is_ai_generated,
        "message": "Meal plan generated successfully"
    })))
}

pub async fn get_current_meal_plan(
    meal_svc: web::Data<Arc<MealPlanService>>,
    claims: web::ReqData<Claims>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    match meal_svc.get_current_plan(user_id).await? {
        Some(plan) => Ok(HttpResponse::Ok().json(plan)),
        None => Ok(HttpResponse::Ok().json(serde_json::json!({
            "message": "No meal plan for this week. Generate one at POST /api/meal-plans/generate"
        }))),
    }
}

pub async fn get_shopping_list(
    meal_svc: web::Data<Arc<MealPlanService>>,
    claims: web::ReqData<Claims>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let list = meal_svc.get_shopping_list(user_id).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "count": list.len(),
        "items": list
    })))
}

pub async fn mark_slot_complete(
    meal_svc: web::Data<Arc<MealPlanService>>,
    claims: web::ReqData<Claims>,
    path: web::Path<(i64, i64)>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let (plan_id, slot_id) = path.into_inner();
    meal_svc.mark_slot_complete(user_id, plan_id, slot_id).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "message": "Slot marked as completed" })))
}

// ── Route configuration ───────────────────────────────────────────────────────

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg
        // Inventory
        .service(
            web::scope("/api/inventory")
                .route("", web::get().to(list_inventory))
                .route("", web::post().to(add_inventory_item))
                .route("/expiring", web::get().to(expiring_soon))
                .route("/{id}", web::put().to(update_inventory_item))
                .route("/{id}", web::delete().to(delete_inventory_item)),
        )
        // Profile + history + favourites
        .service(
            web::scope("/api/me")
                .route("", web::get().to(get_profile))
                .route("", web::put().to(update_profile))
                .route("/history", web::get().to(get_cooking_history))
                .route("/favourites", web::get().to(get_favourites)),
        )
        // Recipe interactions
        .service(
            web::scope("/api/recipes")
                .route("/{id}/rate", web::post().to(rate_recipe))
                .route("/{id}/favourite", web::post().to(toggle_favourite))
                .route("/{id}/cook", web::post().to(mark_cooked)),
        )
        // Meal planning
        .service(
            web::scope("/api/meal-plans")
                .route("/generate", web::post().to(generate_meal_plan))
                .route("/current", web::get().to(get_current_meal_plan))
                .route("/current/shopping-list", web::get().to(get_shopping_list))
                .route("/{plan_id}/slots/{slot_id}/complete", web::put().to(mark_slot_complete)),
        );
}
