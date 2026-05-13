//! Push token service — register and remove device push tokens

use chrono::Utc;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
    ActiveModelTrait, Set,
};
use uuid::Uuid;

use crate::entity::user_push_token;
use crate::errors::AppError;

pub struct PushTokenService {
    db: DatabaseConnection,
}

impl PushTokenService {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// Register or update a device push token for the user.
    /// If the same token already exists (for any user) it is updated to this user.
    pub async fn upsert(
        &self,
        user_id: Uuid,
        token: String,
        platform: String,
    ) -> Result<serde_json::Value, AppError> {
        // Delete any existing row with this token (ownership transfer / re-registration)
        user_push_token::Entity::delete_many()
            .filter(user_push_token::Column::Token.eq(&token))
            .exec(&self.db)
            .await?;

        let id = Uuid::new_v4();
        let model = user_push_token::ActiveModel {
            id: Set(id),
            user_id: Set(user_id),
            token: Set(token.clone()),
            platform: Set(platform.clone()),
            created_at: Set(Utc::now().fixed_offset()),
        };
        model.insert(&self.db).await?;

        Ok(serde_json::json!({
            "id": id,
            "platform": platform,
            "message": "Push token registered"
        }))
    }

    /// List all push tokens for a user
    pub async fn list(&self, user_id: Uuid) -> Result<Vec<serde_json::Value>, AppError> {
        let tokens = user_push_token::Entity::find()
            .filter(user_push_token::Column::UserId.eq(user_id))
            .all(&self.db)
            .await?;

        Ok(tokens
            .into_iter()
            .map(|t| serde_json::json!({
                "id": t.id,
                "platform": t.platform,
                "created_at": t.created_at,
            }))
            .collect())
    }

    /// Delete a specific push token (must belong to user)
    pub async fn delete(&self, user_id: Uuid, token_id: Uuid) -> Result<(), AppError> {
        let token = user_push_token::Entity::find_by_id(token_id)
            .one(&self.db)
            .await?
            .filter(|t| t.user_id == user_id)
            .ok_or(AppError::NotFound("Push token".into()))?;

        user_push_token::Entity::delete_by_id(token.id)
            .exec(&self.db)
            .await?;

        Ok(())
    }
}
