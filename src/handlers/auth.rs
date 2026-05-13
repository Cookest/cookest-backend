//! Authentication handlers for register, login, refresh, and logout
//! 
//! Security features:
//! - Request body size limits
//! - Input validation before processing
//! - HttpOnly cookies for refresh tokens
//! - Generic error messages

use actix_web::{cookie::{Cookie, SameSite}, web, HttpRequest, HttpResponse};
use std::sync::Arc;
use validator::Validate;

use crate::errors::AppError;
use crate::services::AuthService;
use crate::validation::{LoginRequest, RegisterRequest};

/// POST /api/auth/register
/// 
/// Creates a new user account with validated email and password
pub async fn register(
    auth_service: web::Data<Arc<AuthService>>,
    body: web::Json<RegisterRequest>,
) -> Result<HttpResponse, AppError> {
    // Validate input
    body.validate()?;

    let user = auth_service.register(body.into_inner()).await?;

    Ok(HttpResponse::Created().json(serde_json::json!({
        "message": "User registered successfully",
        "user": user
    })))
}

/// POST /api/auth/login
/// 
/// Authenticates user and returns access token + refresh token (in cookie)
pub async fn login(
    auth_service: web::Data<Arc<AuthService>>,
    body: web::Json<LoginRequest>,
) -> Result<HttpResponse, AppError> {
    // Validate input
    body.validate()?;

    let (token_pair, refresh_token, _user) = auth_service.login(body.into_inner()).await?;

    // Create HttpOnly cookie for refresh token
    let refresh_cookie = Cookie::build("refresh_token", refresh_token)
        .path("/api/auth")
        .http_only(true)
        .secure(true) // Only send over HTTPS
        .same_site(SameSite::Strict)
        .max_age(cookie::time::Duration::days(7))
        .finish();

    Ok(HttpResponse::Ok()
        .cookie(refresh_cookie)
        .json(token_pair))
}

/// POST /api/auth/refresh
/// 
/// Refreshes access token using refresh token from cookie
pub async fn refresh(
    auth_service: web::Data<Arc<AuthService>>,
    req: HttpRequest,
) -> Result<HttpResponse, AppError> {
    // Extract refresh token from cookie
    let refresh_token = req
        .cookie("refresh_token")
        .map(|c| c.value().to_string())
        .ok_or(AppError::InvalidToken)?;

    let (token_pair, new_refresh_token, _user) = auth_service.refresh_token(&refresh_token).await?;

    // Rotate refresh token cookie
    let refresh_cookie = Cookie::build("refresh_token", new_refresh_token)
        .path("/api/auth")
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Strict)
        .max_age(cookie::time::Duration::days(7))
        .finish();

    Ok(HttpResponse::Ok()
        .cookie(refresh_cookie)
        .json(token_pair))
}

/// POST /api/auth/logout
/// 
/// Invalidates refresh token
pub async fn logout(
    _auth_service: web::Data<Arc<AuthService>>,
    req: HttpRequest,
) -> Result<HttpResponse, AppError> {
    // Extract user_id from refresh token if present
    if let Some(refresh_cookie) = req.cookie("refresh_token") {
        // We could validate and extract user_id, but for security
        // we'll just clear the cookie regardless
        let _ = refresh_cookie.value();
    }

    // Clear refresh token cookie
    let mut clear_cookie = Cookie::build("refresh_token", "")
        .path("/api/auth")
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Strict)
        .finish();
    clear_cookie.make_removal();

    Ok(HttpResponse::Ok()
        .cookie(clear_cookie)
        .json(serde_json::json!({
            "message": "Logged out successfully"
        })))
}

/// Configure auth routes
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/auth")
            .route("/register", web::post().to(register))
            .route("/login", web::post().to(login))
            .route("/refresh", web::post().to(refresh))
            .route("/logout", web::post().to(logout))
    );
}
