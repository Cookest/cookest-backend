pub mod auth;

pub use auth::JwtAuth;

// Re-export shared middleware
pub use cookest_shared::middleware::rate_limit::{RateLimit, RateLimitConfig};
pub use cookest_shared::middleware::security_headers::SecurityHeaders;

// Re-export Claims so handlers can import it from crate::middleware::Claims
pub use crate::services::token::Claims;
