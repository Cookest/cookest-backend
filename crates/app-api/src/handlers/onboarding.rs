//! Onboarding and account-management handlers.
//!
//! These endpoints handle post-registration setup (dietary prefs, skill level)
//! as well as sensitive account actions (password change, account deletion)
//! that sit outside the main auth flow.

use actix_web::{web, HttpResponse};
use serde::Deserialize;
use std::sync::Arc;

use cookest_shared::errors::AppError;
use crate::middleware::auth::AuthenticatedUser;
use crate::services::auth::AuthService;
use crate::services::onboarding::{OnboardingRequest, OnboardingService};

/// Register onboarding routes onto `cfg`.
///
/// - `POST /api/auth/onboarding` — complete user onboarding (JWT required)
pub fn configure_onboarding(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/auth")
            .route("/onboarding", web::post().to(complete_onboarding)),
    );
}

/// `POST /api/auth/onboarding` — save first-time user preferences.
///
/// JWT required.  Sets dietary restrictions, cooking skill level, household
/// size, and other preference fields collected during app onboarding.
/// Idempotent — can be called again if the user revisits onboarding.
pub async fn complete_onboarding(
    user: AuthenticatedUser,
    service: web::Data<Arc<OnboardingService>>,
    body: web::Json<OnboardingRequest>,
) -> Result<HttpResponse, AppError> {
    let updated = service.complete_onboarding(user.id, body.into_inner()).await?;
    Ok(HttpResponse::Ok().json(updated))
}

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    current_password: String,
    new_password: String,
}

/// `POST /api/auth/change-password` — change the authenticated user’s password.
///
/// JWT required.  **SECURITY**: requires the current password in the request
/// body to prevent account takeover via a stolen access token.  The new
/// password is validated and re-hashed by the auth service.
pub async fn change_password(
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
pub struct DeleteAccountRequest {
    password: String,
}

/// `DELETE /api/auth/account` — permanently delete the authenticated user’s account.
///
/// JWT required.  **SECURITY**: requires password confirmation in the request
/// body as a safeguard against accidental or CSRF-triggered deletion.
/// The auth service performs a hard delete of the user row and all
/// cascading data (inventory, meal plans, chat history).
pub async fn delete_account(
    user: AuthenticatedUser,
    auth_service: web::Data<Arc<AuthService>>,
    body: web::Json<DeleteAccountRequest>,
) -> Result<HttpResponse, AppError> {
    auth_service.delete_account(user.id, &body.password).await?;
    Ok(HttpResponse::NoContent().finish())
}
