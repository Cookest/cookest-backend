//! Application configuration contract.
//!
//! All runtime settings are read from environment variables (or `.env`).
//! Required variables cause a hard startup failure so misconfiguration is
//! caught at launch time rather than at the first request.

use secrecy::{ExposeSecret, SecretString};
use cookest_shared::config::ConfigError;
use std::env;

/// Validated, immutable snapshot of every env-var this service needs.
///
/// Constructed once at startup via [`Config::from_env`] and then shared as
/// `Arc<Config>` / `web::Data`.  All secret values are wrapped in
/// [`SecretString`] so they are never accidentally printed in logs.
#[derive(Clone)]
pub struct Config {
    pub database_url: SecretString,
    pub jwt_secret: SecretString,
    pub jwt_access_expiry_seconds: i64,
    pub jwt_refresh_expiry_seconds: i64,
    pub host: String,
    pub port: u16,
    pub cors_origin: String,
    pub ollama_url: String,
    pub ollama_model: String,
    pub ollama_embed_model: String,
    pub pdf_upload_dir: String,
    pub stripe_webhook_secret: Option<String>,
    pub food_api_url: String,
    pub food_api_key: Option<String>,
    pub resend_api_key: Option<SecretString>,
    pub resend_from_email: String,
    pub overpass_url: String,
    pub rag_top_k: u64,
    pub self_hosted: bool,
}

impl Config {
    /// Load configuration from environment variables.
    ///
    /// **Required vars** (missing → `ConfigError::Missing` → startup panic):
    /// - `APP_DATABASE_URL` or `DATABASE_URL` — PostgreSQL connection string
    /// - `JWT_SECRET` — HMAC signing key; must be ≥ 32 chars (256 bits)
    ///
    /// **Optional vars** (sensible defaults in parentheses):
    /// - `JWT_ACCESS_EXPIRY_SECONDS` (900 = 15 min)
    /// - `JWT_REFRESH_EXPIRY_SECONDS` (604800 = 7 days)
    /// - `HOST` (127.0.0.1), `PORT` (8080)
    /// - `CORS_ORIGIN`, `OLLAMA_URL`, `OLLAMA_MODEL`
    /// - `PDF_UPLOAD_DIR`, `FOOD_API_URL`, `FOOD_API_KEY`
    /// - `RESEND_API_KEY`, `RESEND_FROM_EMAIL`
    /// - `STRIPE_WEBHOOK_SECRET`
    /// - `OLLAMA_EMBED_MODEL` (nomic-embed-text) — RAG embeddings model
    /// - `OVERPASS_URL` — OpenStreetMap Overpass endpoint for nearby stores
    /// - `RAG_TOP_K` (5) — number of knowledge chunks retrieved per query
    pub fn from_env() -> Result<Self, ConfigError> {
        dotenvy::dotenv().ok();

        // Support both APP_DATABASE_URL (microservice) and DATABASE_URL (monolith)
        let database_url = env::var("APP_DATABASE_URL")
            .or_else(|_| env::var("DATABASE_URL"))
            .map_err(|_| ConfigError::Missing("DATABASE_URL"))?;

        let jwt_secret = env::var("JWT_SECRET")
            .map_err(|_| ConfigError::Missing("JWT_SECRET"))?;

        if jwt_secret.len() < 32 {
            return Err(ConfigError::InvalidValue(
                "JWT_SECRET must be at least 32 characters (256 bits)",
            ));
        }

        let jwt_access_expiry_seconds: i64 = env::var("JWT_ACCESS_EXPIRY_SECONDS")
            .unwrap_or_else(|_| "900".to_string())
            .parse()
            .map_err(|_| ConfigError::InvalidValue("JWT_ACCESS_EXPIRY_SECONDS must be a number"))?;

        let jwt_refresh_expiry_seconds: i64 = env::var("JWT_REFRESH_EXPIRY_SECONDS")
            .unwrap_or_else(|_| "604800".to_string())
            .parse()
            .map_err(|_| ConfigError::InvalidValue("JWT_REFRESH_EXPIRY_SECONDS must be a number"))?;

        let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());

        let port: u16 = env::var("PORT")
            .unwrap_or_else(|_| "8080".to_string())
            .parse()
            .map_err(|_| ConfigError::InvalidValue("PORT must be a valid port number"))?;

        let cors_origin = env::var("CORS_ORIGIN")
            .unwrap_or_else(|_| "http://localhost:3000".to_string());

        let ollama_url = env::var("OLLAMA_URL")
            .unwrap_or_else(|_| "http://localhost:11434".to_string());

        let ollama_model = env::var("OLLAMA_MODEL")
            .unwrap_or_else(|_| "llava".to_string());

        let ollama_embed_model = env::var("OLLAMA_EMBED_MODEL")
            .unwrap_or_else(|_| "nomic-embed-text".to_string());

        let pdf_upload_dir = env::var("PDF_UPLOAD_DIR")
            .unwrap_or_else(|_| "./cookest_pdfs".to_string());

        let stripe_webhook_secret = env::var("STRIPE_WEBHOOK_SECRET").ok();

        let food_api_url = env::var("FOOD_API_URL")
            .unwrap_or_else(|_| "http://localhost:8081".to_string());

        let food_api_key = env::var("FOOD_API_KEY").ok();

        let resend_api_key = env::var("RESEND_API_KEY")
            .map(SecretString::from)
            .ok();

        let resend_from_email = env::var("RESEND_FROM_EMAIL")
            .unwrap_or_else(|_| "noreply@m.cookest.app".to_string());

        let overpass_url = env::var("OVERPASS_URL")
            .unwrap_or_else(|_| "https://overpass-api.de/api/interpreter".to_string());

        let rag_top_k: u64 = env::var("RAG_TOP_K")
            .unwrap_or_else(|_| "5".to_string())
            .parse()
            .map_err(|_| ConfigError::InvalidValue("RAG_TOP_K must be a number"))?;

        let self_hosted = env::var("SELF_HOSTED")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(false);

        Ok(Self {
            database_url: SecretString::from(database_url),
            jwt_secret: SecretString::from(jwt_secret),
            jwt_access_expiry_seconds,
            jwt_refresh_expiry_seconds,
            host,
            port,
            cors_origin,
            ollama_url,
            ollama_model,
            ollama_embed_model,
            pdf_upload_dir,
            stripe_webhook_secret,
            food_api_url,
            food_api_key,
            resend_api_key,
            resend_from_email,
            overpass_url,
            rag_top_k,
            self_hosted,
        })
    }

    /// Expose the database URL as a plain `&str` for SeaORM connection setup.
    ///
    /// The value is kept behind [`SecretString`] at rest; only call this where
    /// strictly necessary (i.e. when building the DB pool).
    pub fn database_url(&self) -> &str {
        self.database_url.expose_secret()
    }

    /// Expose the JWT HMAC secret for token signing/verification.
    ///
    /// Keep the returned `&str` in scope only for the duration of the
    /// sign/verify operation — do not clone or log it.
    pub fn jwt_secret(&self) -> &str {
        self.jwt_secret.expose_secret()
    }
}
