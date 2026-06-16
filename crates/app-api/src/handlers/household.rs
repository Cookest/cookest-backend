//! Household (family group) handlers.
//!
//! - `POST /api/households`            — create a household (creator becomes owner)
//! - `GET  /api/households/me`         — the caller's household + members
//! - `POST /api/households/{id}/invites` — owner generates a shareable invite token
//! - `POST /api/households/join`       — join a household with an invite token

use actix_web::{web, HttpResponse};
use std::sync::Arc;
use uuid::Uuid;

use crate::middleware::auth::AuthenticatedUser;
use crate::services::household::{CreateHouseholdRequest, HouseholdService, JoinRequest};
use cookest_shared::errors::AppError;

pub fn configure_households(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/households")
            .route("", web::post().to(create))
            .route("/me", web::get().to(me))
            .route("/join", web::post().to(join))
            .route("/{id}/invites", web::post().to(create_invite)),
    );
}

async fn create(
    user: AuthenticatedUser,
    service: web::Data<Arc<HouseholdService>>,
    body: web::Json<CreateHouseholdRequest>,
) -> Result<HttpResponse, AppError> {
    let view = service.create(user.id, body.into_inner().name).await?;
    Ok(HttpResponse::Created().json(view))
}

async fn me(
    user: AuthenticatedUser,
    service: web::Data<Arc<HouseholdService>>,
) -> Result<HttpResponse, AppError> {
    match service.my_household(user.id).await? {
        Some(view) => Ok(HttpResponse::Ok().json(view)),
        None => Ok(HttpResponse::Ok().json(serde_json::json!(null))),
    }
}

async fn create_invite(
    user: AuthenticatedUser,
    service: web::Data<Arc<HouseholdService>>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let token = service.create_invite(user.id, path.into_inner()).await?;
    Ok(HttpResponse::Created().json(serde_json::json!({ "token": token })))
}

async fn join(
    user: AuthenticatedUser,
    service: web::Data<Arc<HouseholdService>>,
    body: web::Json<JoinRequest>,
) -> Result<HttpResponse, AppError> {
    let view = service.join(user.id, &body.into_inner().token).await?;
    Ok(HttpResponse::Ok().json(view))
}
