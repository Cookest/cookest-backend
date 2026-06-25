use actix_web::{web, HttpResponse};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::middleware::Claims;
use crate::services::meal_plan_suggestion::MealPlanSuggestionService;
use cookest_shared::errors::AppError;

#[derive(Deserialize)]
pub struct CreateSuggestionRequest {
    pub slot_id: i64,
    pub recipe_id: i64,
}

#[derive(Deserialize)]
pub struct UpdateSuggestionStatusRequest {
    pub status: String, // 'approved' or 'rejected'
}

pub async fn get_plan_suggestions(
    suggestion_service: web::Data<Arc<MealPlanSuggestionService>>,
    _claims: web::ReqData<Claims>,
    path: web::Path<i64>,
) -> Result<HttpResponse, AppError> {
    let plan_id = path.into_inner();

    let suggestions = suggestion_service
        .get_suggestions_for_plan(plan_id)
        .await
        .map_err(|e| AppError::Internal(e))?;

    Ok(HttpResponse::Ok().json(suggestions))
}

pub async fn create_suggestion(
    suggestion_service: web::Data<Arc<MealPlanSuggestionService>>,
    claims: web::ReqData<Claims>,
    path: web::Path<i64>,
    body: web::Json<CreateSuggestionRequest>,
) -> Result<HttpResponse, AppError> {
    let suggested_by = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let plan_id = path.into_inner();

    // In a real implementation, you would look up the family_owner_id for the plan_id
    // For now, we use suggested_by as a fallback
    let family_owner_id = suggested_by;

    let suggestion = suggestion_service
        .create_suggestion(
            plan_id,
            body.slot_id,
            body.recipe_id,
            suggested_by,
            family_owner_id,
        )
        .await
        .map_err(|e| AppError::Internal(e))?;

    Ok(HttpResponse::Created().json(suggestion))
}

pub async fn update_suggestion(
    suggestion_service: web::Data<Arc<MealPlanSuggestionService>>,
    _claims: web::ReqData<Claims>,
    path: web::Path<(i64, i64)>,
    body: web::Json<UpdateSuggestionStatusRequest>,
) -> Result<HttpResponse, AppError> {
    let (_plan_id, suggestion_id) = path.into_inner();

    let suggestion = suggestion_service
        .update_suggestion_status(suggestion_id, &body.status)
        .await
        .map_err(|e| AppError::Internal(e))?;

    Ok(HttpResponse::Ok().json(suggestion))
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/meal-plans/{plan_id}/suggestions")
            .route("", web::get().to(get_plan_suggestions))
            .route("", web::post().to(create_suggestion))
            .route("/{id}", web::put().to(update_suggestion)),
    );
}
