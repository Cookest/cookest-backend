//! API key authentication middleware
//! Validates X-API-Key header against hashed keys in the database

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use sha2::{Digest, Sha256};

use crate::entity::api_key;
use crate::errors::AppError;

/// Extracted API key info available in handlers
#[derive(Debug, Clone)]
pub struct ApiKeyInfo {
    pub id: uuid::Uuid,
    pub name: String,
    pub tier: String,
}

/// Validate an API key from the X-API-Key header
pub async fn validate_api_key(
    db: &DatabaseConnection,
    raw_key: &str,
) -> Result<ApiKeyInfo, AppError> {
    let hash = format!("{:x}", Sha256::digest(raw_key.as_bytes()));

    let key = api_key::Entity::find()
        .filter(api_key::Column::KeyHash.eq(&hash))
        .filter(api_key::Column::IsActive.eq(true))
        .one(db)
        .await
        .map_err(|e| AppError::Internal(format!("DB error: {}", e)))?
        .ok_or(AppError::ApiKeyInvalid)?;

    Ok(ApiKeyInfo {
        id: key.id,
        name: key.name,
        tier: key.tier,
    })
}
