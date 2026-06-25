//! Taste profile handlers — swipe recording.

use actix_web::{web, HttpResponse};
use std::sync::Arc;

use crate::middleware::auth::AuthenticatedUser;
use crate::services::taste_profile::{SwipeRequest, TasteProfileService};
use cookest_shared::errors::AppError;

pub fn configure_taste_profile(cfg: &mut web::ServiceConfig) {
    cfg.service(web::scope("/api/me").route("/swipe", web::post().to(record_swipe)));
}

/// `POST /api/me/swipe` — record a recipe swipe event.
///
/// Body: `{ "recipe_id": 42, "direction": "like" | "dislike" }`
async fn record_swipe(
    user: AuthenticatedUser,
    service: web::Data<Arc<TasteProfileService>>,
    body: web::Json<SwipeRequest>,
) -> Result<HttpResponse, AppError> {
    service.record_swipe(user.id, body.into_inner()).await?;
    Ok(HttpResponse::NoContent().finish())
}
