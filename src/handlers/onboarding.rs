//! Onboarding + auth improvement handlers

use actix_web::{web, HttpResponse};
use serde::Deserialize;
use std::sync::Arc;

use crate::errors::AppError;
use crate::middleware::auth::AuthenticatedUser;
use crate::services::auth::AuthService;
use crate::services::onboarding::{OnboardingRequest, OnboardingService};

pub fn configure_onboarding(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/auth")
            .route("/onboarding", web::post().to(complete_onboarding)),
    );
    cfg.service(
        web::scope("/api/me")
            .route("/change-password", web::post().to(change_password))
            .route("", web::delete().to(delete_account)),
    );
}

async fn complete_onboarding(
    user: AuthenticatedUser,
    service: web::Data<Arc<OnboardingService>>,
    body: web::Json<OnboardingRequest>,
) -> Result<HttpResponse, AppError> {
    let updated = service.complete_onboarding(user.id, body.into_inner()).await?;
    Ok(HttpResponse::Ok().json(updated))
}

#[derive(Deserialize)]
struct ChangePasswordRequest {
    current_password: String,
    new_password: String,
}

async fn change_password(
    user: AuthenticatedUser,
    auth_service: web::Data<Arc<AuthService>>,
    body: web::Json<ChangePasswordRequest>,
) -> Result<HttpResponse, AppError> {
    auth_service
        .change_password(user.id, &body.current_password, &body.new_password)
        .await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "message": "Password changed successfully" })))
}

#[derive(Deserialize)]
struct DeleteAccountRequest {
    password: String,
}

async fn delete_account(
    user: AuthenticatedUser,
    auth_service: web::Data<Arc<AuthService>>,
    body: web::Json<DeleteAccountRequest>,
) -> Result<HttpResponse, AppError> {
    auth_service.delete_account(user.id, &body.password).await?;
    Ok(HttpResponse::NoContent().finish())
}
