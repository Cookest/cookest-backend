use actix_web::{HttpResponse, ResponseError};
use sea_orm::DbErr;
use serde::Serialize;
use std::fmt;
use validator::ValidationErrors;

/// Application error types with security-conscious external messages
#[derive(Debug)]
pub enum AppError {
    /// Database errors - log internally, return generic message
    Database(DbErr),
    /// Validation errors - safe to return details
    Validation(ValidationErrors),
    /// Bad request with a descriptive message
    BadRequest(String),
    /// Authentication failed - generic message
    AuthenticationFailed,
    /// Invalid API key
    ApiKeyInvalid,
    /// Resource not found
    NotFound(String),
    /// Internal server error
    Internal(String),
    /// Rate limit exceeded
    RateLimitExceeded,
    /// Forbidden — authenticated but not allowed
    Forbidden,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<serde_json::Value>,
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Database(_) => write!(f, "Database error"),
            AppError::Validation(e) => write!(f, "Validation error: {}", e),
            AppError::BadRequest(msg) => write!(f, "Bad request: {}", msg),
            AppError::AuthenticationFailed => write!(f, "Authentication failed"),
            AppError::ApiKeyInvalid => write!(f, "Invalid API key"),
            AppError::NotFound(r) => write!(f, "{} not found", r),
            AppError::Internal(_) => write!(f, "Internal server error"),
            AppError::RateLimitExceeded => write!(f, "Too many requests"),
            AppError::Forbidden => write!(f, "Forbidden"),
        }
    }
}

impl ResponseError for AppError {
    fn error_response(&self) -> HttpResponse {
        let (status, error_response) = match self {
            AppError::Database(e) => {
                tracing::error!("Database error: {:?}", e);
                (
                    actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorResponse {
                        error: "An internal error occurred".to_string(),
                        details: None,
                    },
                )
            }
            AppError::Validation(errors) => (
                actix_web::http::StatusCode::BAD_REQUEST,
                ErrorResponse {
                    error: "Validation failed".to_string(),
                    details: Some(serde_json::to_value(errors).unwrap_or_default()),
                },
            ),
            AppError::BadRequest(msg) => (
                actix_web::http::StatusCode::BAD_REQUEST,
                ErrorResponse {
                    error: msg.clone(),
                    details: None,
                },
            ),
            AppError::AuthenticationFailed => (
                actix_web::http::StatusCode::UNAUTHORIZED,
                ErrorResponse {
                    error: "Authentication failed".to_string(),
                    details: None,
                },
            ),
            AppError::ApiKeyInvalid => (
                actix_web::http::StatusCode::UNAUTHORIZED,
                ErrorResponse {
                    error: "Invalid or missing API key".to_string(),
                    details: None,
                },
            ),
            AppError::Internal(e) => {
                tracing::error!("Internal error: {}", e);
                (
                    actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorResponse {
                        error: "An internal error occurred".to_string(),
                        details: None,
                    },
                )
            }
            AppError::NotFound(resource) => (
                actix_web::http::StatusCode::NOT_FOUND,
                ErrorResponse {
                    error: format!("{} not found", resource),
                    details: None,
                },
            ),
            AppError::RateLimitExceeded => (
                actix_web::http::StatusCode::TOO_MANY_REQUESTS,
                ErrorResponse {
                    error: "Too many requests. Please try again later.".to_string(),
                    details: None,
                },
            ),
            AppError::Forbidden => (
                actix_web::http::StatusCode::FORBIDDEN,
                ErrorResponse {
                    error: "You do not have permission to perform this action.".to_string(),
                    details: None,
                },
            ),
        };

        HttpResponse::build(status).json(error_response)
    }
}

impl From<DbErr> for AppError {
    fn from(err: DbErr) -> Self {
        AppError::Database(err)
    }
}

impl From<ValidationErrors> for AppError {
    fn from(err: ValidationErrors) -> Self {
        AppError::Validation(err)
    }
}
