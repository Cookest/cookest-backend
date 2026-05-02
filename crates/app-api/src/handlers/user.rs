//! Inventory, Profile, Interaction, and Meal Plan handlers

use actix_web::{web, HttpResponse};
use std::sync::Arc;
use uuid::Uuid;

use cookest_shared::errors::AppError;
use crate::models::inventory::{AddInventoryItem, UpdateInventoryItem};
use crate::models::profile::UpdateProfileRequest;
use crate::models::interaction::RateRecipeRequest;
use crate::models::meal_plan::GenerateMealPlanRequest;
use crate::services::{InventoryService, ProfileService, InteractionService, MealPlanService, PushTokenService, PreferenceService};
use crate::middleware::Claims;
use crate::handlers::onboarding::{complete_onboarding, change_password, delete_account};

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

#[derive(serde::Deserialize)]
pub struct ListPlansQuery {
    pub page: Option<u64>,
    pub per_page: Option<u64>,
}

pub async fn list_meal_plans(
    meal_svc: web::Data<Arc<MealPlanService>>,
    claims: web::ReqData<Claims>,
    query: web::Query<ListPlansQuery>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let page = query.page.unwrap_or(1);
    let per_page = query.per_page.unwrap_or(10).min(50);
    let result = meal_svc.list_plans(user_id, page, per_page).await?;
    Ok(HttpResponse::Ok().json(result))
}

pub async fn get_meal_plan(
    meal_svc: web::Data<Arc<MealPlanService>>,
    claims: web::ReqData<Claims>,
    path: web::Path<i64>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let result = meal_svc.get_plan(user_id, path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(result))
}

pub async fn delete_meal_plan(
    meal_svc: web::Data<Arc<MealPlanService>>,
    claims: web::ReqData<Claims>,
    path: web::Path<i64>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    meal_svc.delete_plan(user_id, path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "message": "Meal plan deleted" })))
}

#[derive(serde::Deserialize)]
pub struct SwapSlotBody {
    pub recipe_id: Option<i64>,
    pub flex_type: Option<String>,
    pub energy_level: Option<String>,
}

pub async fn swap_slot(
    meal_svc: web::Data<Arc<MealPlanService>>,
    claims: web::ReqData<Claims>,
    path: web::Path<(i64, i64)>,
    body: web::Json<SwapSlotBody>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let (plan_id, slot_id) = path.into_inner();
    let body = body.into_inner();
    let result = meal_svc
        .swap_slot(user_id, plan_id, slot_id, body.recipe_id, body.flex_type, body.energy_level)
        .await?;
    Ok(HttpResponse::Ok().json(result))
}

#[derive(serde::Deserialize)]
pub struct MarkFlexBody {
    pub flex_type: String,
    pub energy_level: Option<String>,
}

pub async fn mark_slot_flex(
    meal_svc: web::Data<Arc<MealPlanService>>,
    claims: web::ReqData<Claims>,
    path: web::Path<(i64, i64)>,
    body: web::Json<MarkFlexBody>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let (plan_id, slot_id) = path.into_inner();
    let body = body.into_inner();
    meal_svc
        .mark_slot_flex(user_id, plan_id, slot_id, body.flex_type, body.energy_level)
        .await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "message": "Slot marked as flex day" })))
}

pub async fn get_nutrition_summary(
    meal_svc: web::Data<Arc<MealPlanService>>,
    claims: web::ReqData<Claims>,
    path: web::Path<i64>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let result = meal_svc.get_nutrition_summary(user_id, path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(result))
}

// ── Preference Weights ────────────────────────────────────────────────────────

pub async fn get_preferences(
    pref_svc: web::Data<Arc<PreferenceService>>,
    claims: web::ReqData<Claims>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let prefs = pref_svc.get_preferences(user_id).await?;
    Ok(HttpResponse::Ok().json(prefs))
}

pub async fn reset_preferences(
    pref_svc: web::Data<Arc<PreferenceService>>,
    claims: web::ReqData<Claims>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    pref_svc.reset_preferences(user_id).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "message": "Preference weights reset" })))
}

// ── Push Tokens ───────────────────────────────────────────────────────────────
#[derive(serde::Deserialize)]
pub struct RegisterPushTokenBody {
    pub token: String,
    pub platform: String,
}

pub async fn register_push_token(
    push_svc: web::Data<Arc<PushTokenService>>,
    claims: web::ReqData<Claims>,
    body: web::Json<RegisterPushTokenBody>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let body = body.into_inner();
    let result = push_svc.upsert(user_id, body.token, body.platform).await?;
    Ok(HttpResponse::Created().json(result))
}

pub async fn list_push_tokens(
    push_svc: web::Data<Arc<PushTokenService>>,
    claims: web::ReqData<Claims>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let tokens = push_svc.list(user_id).await?;
    Ok(HttpResponse::Ok().json(tokens))
}

pub async fn delete_push_token(
    push_svc: web::Data<Arc<PushTokenService>>,
    claims: web::ReqData<Claims>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    push_svc.delete(user_id, path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "message": "Push token removed" })))
}

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
        // Profile + history + favourites + push tokens + preferences
        .service(
            web::scope("/api/me")
                .route("", web::get().to(get_profile))
                .route("", web::put().to(update_profile))
                .route("", web::delete().to(delete_account))
                .route("/history", web::get().to(get_cooking_history))
                .route("/favourites", web::get().to(get_favourites))
                .route("/push-tokens", web::get().to(list_push_tokens))
                .route("/push-tokens", web::post().to(register_push_token))
                .route("/push-tokens/{id}", web::delete().to(delete_push_token))
                .route("/preferences", web::get().to(get_preferences))
                .route("/preferences", web::delete().to(reset_preferences))
                .route("/onboarding", web::post().to(complete_onboarding))
                .route("/change-password", web::post().to(change_password)),
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
                .route("", web::get().to(list_meal_plans))
                .route("/generate", web::post().to(generate_meal_plan))
                .route("/current", web::get().to(get_current_meal_plan))
                .route("/current/shopping-list", web::get().to(get_shopping_list))
                .route("/{id}", web::get().to(get_meal_plan))
                .route("/{id}", web::delete().to(delete_meal_plan))
                .route("/{id}/nutrition", web::get().to(get_nutrition_summary))
                .route("/{plan_id}/slots/{slot_id}", web::put().to(swap_slot))
                .route("/{plan_id}/slots/{slot_id}/complete", web::put().to(mark_slot_complete))
                .route("/{plan_id}/slots/{slot_id}/flex", web::put().to(mark_slot_flex)),
        );
}
