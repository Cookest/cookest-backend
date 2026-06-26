//! Nutrition AI handlers — RAG-grounded suggestions.
//!
//! Routes (JWT required):
//!   POST /api/ai/what-to-buy          — nutrition-aware shopping suggestions
//!   POST /api/ai/recipe-suggestions   — nutrition-aware recipe ideas

use actix_web::{web, HttpResponse};
use std::sync::Arc;
use uuid::Uuid;

use cookest_shared::errors::AppError;
use crate::middleware::Claims;
use crate::services::nutrition::{NutritionService, RecipeSuggestRequest, WhatToBuyRequest};

pub async fn what_to_buy(
    svc: web::Data<Arc<NutritionService>>,
    sub_service: web::Data<Arc<crate::services::subscription::SubscriptionService>>,
    claims: web::ReqData<Claims>,
    body: web::Json<WhatToBuyRequest>,
) -> Result<HttpResponse, AppError> {
    sub_service.require_pro_for_ai_feature(&claims, "ai_grocery_suggestions")?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let res = svc.what_to_buy(user_id, body.into_inner().goal).await?;
    Ok(HttpResponse::Ok().json(res))
}

pub async fn recipe_suggestions(
    svc: web::Data<Arc<NutritionService>>,
    sub_service: web::Data<Arc<crate::services::subscription::SubscriptionService>>,
    claims: web::ReqData<Claims>,
    body: web::Json<RecipeSuggestRequest>,
) -> Result<HttpResponse, AppError> {
    sub_service.require_pro_for_ai_feature(&claims, "ai_recipe_suggestions")?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let res = svc.recipe_suggestions(user_id, body.into_inner().count.unwrap_or(5)).await?;
    Ok(HttpResponse::Ok().json(res))
}

pub fn configure_nutrition(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/ai")
            .route("/what-to-buy", web::post().to(what_to_buy))
            .route("/recipe-suggestions", web::post().to(recipe_suggestions)),
    );
}
