//! Meal poll handlers.
//!
//! Public (no auth — for people without the app):
//!   - `GET  /api/polls/{token}`       — view a poll + live results
//!   - `POST /api/polls/{token}/vote`  — cast/change a vote
//!
//! Protected (JWT):
//!   - `POST /api/polls`               — create a poll from candidate dishes

use actix_web::{web, HttpResponse};
use std::sync::Arc;

use crate::middleware::auth::AuthenticatedUser;
use crate::services::meal_poll::{CreatePollRequest, MealPollService, VoteRequest};
use cookest_shared::errors::AppError;

/// Public, unauthenticated poll routes (mounted outside the JWT scope).
pub fn configure_polls_public(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/polls")
            .route("/{token}", web::get().to(view))
            .route("/{token}/vote", web::post().to(vote)),
    );
}

/// Authenticated poll routes.
pub fn configure_polls_protected(cfg: &mut web::ServiceConfig) {
    cfg.service(web::scope("/api/my-polls").route("", web::post().to(create)));
}

async fn create(
    user: AuthenticatedUser,
    service: web::Data<Arc<MealPollService>>,
    body: web::Json<CreatePollRequest>,
) -> Result<HttpResponse, AppError> {
    let view = service.create(user.id, body.into_inner()).await?;
    Ok(HttpResponse::Created().json(view))
}

async fn view(
    service: web::Data<Arc<MealPollService>>,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let view = service.view_by_token(&path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(view))
}

async fn vote(
    service: web::Data<Arc<MealPollService>>,
    path: web::Path<String>,
    body: web::Json<VoteRequest>,
) -> Result<HttpResponse, AppError> {
    let view = service.vote(&path.into_inner(), body.into_inner()).await?;
    Ok(HttpResponse::Ok().json(view))
}
