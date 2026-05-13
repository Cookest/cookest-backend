//! Input validation schemas for authentication endpoints
//! 
//! Security features:
//! - Email normalization (lowercase, trim)
//! - Strong password requirements (OWASP guidelines)
//! - Length limits to prevent DoS

use serde::Deserialize;
use validator::Validate;

/// Registration request with strict validation
#[derive(Debug, Deserialize, Validate)]
pub struct RegisterRequest {
    #[validate(
        email(message = "Invalid email format"),
        length(max = 255, message = "Email too long")
    )]
    pub email: String,

    #[validate(
        length(min = 8, max = 128, message = "Password must be 8-128 characters"),
        custom(function = "validate_password_strength")
    )]
    pub password: String,
}

/// Login request
#[derive(Debug, Deserialize, Validate)]
pub struct LoginRequest {
    #[validate(
        email(message = "Invalid email format"),
        length(max = 255, message = "Email too long")
    )]
    pub email: String,

    #[validate(length(max = 128, message = "Password too long"))]
    pub password: String,
}

/// Refresh token request (token comes from HttpOnly cookie)
#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    // Token extracted from cookie, not from body
}

/// Custom password strength validator
/// Requires: uppercase, lowercase, digit, special character
fn validate_password_strength(password: &str) -> Result<(), validator::ValidationError> {
    let has_uppercase = password.chars().any(|c| c.is_uppercase());
    let has_lowercase = password.chars().any(|c| c.is_lowercase());
    let has_digit = password.chars().any(|c| c.is_ascii_digit());
    let has_special = password.chars().any(|c| !c.is_alphanumeric());

    if !has_uppercase {
        let mut err = validator::ValidationError::new("password_strength");
        err.message = Some("Password must contain at least one uppercase letter".into());
        return Err(err);
    }

    if !has_lowercase {
        let mut err = validator::ValidationError::new("password_strength");
        err.message = Some("Password must contain at least one lowercase letter".into());
        return Err(err);
    }

    if !has_digit {
        let mut err = validator::ValidationError::new("password_strength");
        err.message = Some("Password must contain at least one digit".into());
        return Err(err);
    }

    if !has_special {
        let mut err = validator::ValidationError::new("password_strength");
        err.message = Some("Password must contain at least one special character".into());
        return Err(err);
    }

    Ok(())
}

/// Normalize email for consistent storage and lookup
pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_password_validation() {
        // Valid password
        assert!(validate_password_strength("SecurePass123!").is_ok());
        
        // Missing uppercase
        assert!(validate_password_strength("securepass123!").is_err());
        
        // Missing lowercase
        assert!(validate_password_strength("SECUREPASS123!").is_err());
        
        // Missing digit
        assert!(validate_password_strength("SecurePass!!!").is_err());
        
        // Missing special char
        assert!(validate_password_strength("SecurePass123").is_err());
    }

    #[test]
    fn test_email_normalization() {
        assert_eq!(normalize_email("  Test@Example.COM  "), "test@example.com");
    }
}
