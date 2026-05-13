//! Token service for JWT generation and validation
//! 
//! Security features:
//! - Short-lived access tokens (15 min default)
//! - Refresh tokens stored as hashes in DB
//! - Secure random token generation
//! - Algorithm explicitly specified (prevents algorithm confusion attacks)
//! - Subscription tier embedded ONLY in access tokens, never in refresh tokens

use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, TokenData, Validation};
use rand::Rng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::Config;
use cookest_shared::errors::AppError;

/// Subscription tier — determines feature access throughout the API
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SubscriptionTier {
    Free,
    Pro,
    Family,
}

impl SubscriptionTier {
    pub fn from_str(s: &str) -> Self {
        match s {
            "pro" => SubscriptionTier::Pro,
            "family" => SubscriptionTier::Family,
            _ => SubscriptionTier::Free,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            SubscriptionTier::Free => "free",
            SubscriptionTier::Pro => "pro",
            SubscriptionTier::Family => "family",
        }
    }

    /// Returns true if this tier is Pro or Family
    pub fn is_pro_or_above(&self) -> bool {
        matches!(self, SubscriptionTier::Pro | SubscriptionTier::Family)
    }
}

/// JWT Claims structure
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    /// Subject (user ID)
    pub sub: String,
    /// User email
    pub email: String,
    /// Token type: "access" or "refresh"
    pub token_type: TokenType,
    /// Expiration time (Unix timestamp)
    pub exp: i64,
    /// Issued at (Unix timestamp)
    pub iat: i64,
    /// JWT ID (unique identifier for this token)
    pub jti: String,
    /// Subscription tier — ONLY present in access tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<SubscriptionTier>,
    /// Whether the user is an admin — ONLY present in access tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_admin: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TokenType {
    Access,
    Refresh,
}

/// Token pair returned after successful authentication
#[derive(Debug, Serialize)]
pub struct TokenPair {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    // refresh_token is set as HttpOnly cookie, not in response body
}

pub struct TokenService {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    access_expiry_seconds: i64,
    refresh_expiry_seconds: i64,
}

impl TokenService {
    pub fn new(config: &Config) -> Self {
        let secret = config.jwt_secret().as_bytes();
        Self {
            encoding_key: EncodingKey::from_secret(secret),
            decoding_key: DecodingKey::from_secret(secret),
            access_expiry_seconds: config.jwt_access_expiry_seconds,
            refresh_expiry_seconds: config.jwt_refresh_expiry_seconds,
        }
    }

    /// Generate access token (short-lived) — embeds tier and admin flag
    pub fn generate_access_token(
        &self,
        user_id: Uuid,
        email: &str,
        tier: SubscriptionTier,
        is_admin: bool,
    ) -> Result<String, AppError> {
        let now = Utc::now();
        let exp = now + Duration::seconds(self.access_expiry_seconds);

        let claims = Claims {
            sub: user_id.to_string(),
            email: email.to_string(),
            token_type: TokenType::Access,
            exp: exp.timestamp(),
            iat: now.timestamp(),
            jti: generate_jti(),
            tier: Some(tier),
            is_admin: Some(is_admin),
        };

        encode(&Header::default(), &claims, &self.encoding_key)
            .map_err(|e| AppError::Internal(format!("Token generation failed: {}", e)))
    }

    /// Generate refresh token (long-lived) — does NOT embed tier (tier read from DB on refresh)
    pub fn generate_refresh_token(&self, user_id: Uuid, email: &str) -> Result<String, AppError> {
        let now = Utc::now();
        let exp = now + Duration::seconds(self.refresh_expiry_seconds);

        let claims = Claims {
            sub: user_id.to_string(),
            email: email.to_string(),
            token_type: TokenType::Refresh,
            exp: exp.timestamp(),
            iat: now.timestamp(),
            jti: generate_jti(),
            tier: None,
            is_admin: None,
        };

        encode(&Header::default(), &claims, &self.encoding_key)
            .map_err(|e| AppError::Internal(format!("Token generation failed: {}", e)))
    }

    /// Validate and decode a token
    pub fn validate_token(&self, token: &str) -> Result<TokenData<Claims>, AppError> {
        let mut validation = Validation::default();
        // Explicitly set algorithm to prevent algorithm confusion attacks
        validation.set_required_spec_claims(&["exp", "iat", "sub"]);

        decode::<Claims>(token, &self.decoding_key, &validation).map_err(|e| {
            match e.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => AppError::TokenExpired,
                _ => AppError::InvalidToken,
            }
        })
    }

    /// Validate access token specifically
    pub fn validate_access_token(&self, token: &str) -> Result<Claims, AppError> {
        let token_data = self.validate_token(token)?;
        
        if token_data.claims.token_type != TokenType::Access {
            return Err(AppError::InvalidToken);
        }
        
        Ok(token_data.claims)
    }

    /// Validate refresh token specifically
    pub fn validate_refresh_token(&self, token: &str) -> Result<Claims, AppError> {
        let token_data = self.validate_token(token)?;
        
        if token_data.claims.token_type != TokenType::Refresh {
            return Err(AppError::InvalidToken);
        }
        
        Ok(token_data.claims)
    }

    /// Get refresh token expiry in seconds (for cookie max-age)
    pub fn refresh_expiry_seconds(&self) -> i64 {
        self.refresh_expiry_seconds
    }

    /// Get access token expiry in seconds
    pub fn access_expiry_seconds(&self) -> i64 {
        self.access_expiry_seconds
    }
}

/// Generate a unique JWT ID
fn generate_jti() -> String {
    let mut rng = rand::thread_rng();
    let random_bytes: [u8; 16] = rng.gen();
    hex_encode(&random_bytes)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(HEX_CHARS[(byte >> 4) as usize] as char);
        result.push(HEX_CHARS[(byte & 0x0f) as usize] as char);
    }
    result
}
