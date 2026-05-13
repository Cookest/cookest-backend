//! Profile Service — fetch and update user profile

use chrono::Utc;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set};
use serde_json::json;
use uuid::Uuid;

use crate::entity::user;
use crate::errors::AppError;
use crate::models::profile::*;

pub struct ProfileService {
    db: DatabaseConnection,
}

impl ProfileService {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// Get user profile by ID
    pub async fn get_profile(&self, user_id: Uuid) -> Result<ProfileResponse, AppError> {
        let user = user::Entity::find_by_id(user_id)
            .one(&self.db)
            .await?
            .ok_or(AppError::NotFound("User".into()))?;

        Ok(ProfileResponse {
            id: user.id.to_string(),
            email: user.email,
            name: user.name,
            household_size: user.household_size,
            dietary_restrictions: user.dietary_restrictions,
            allergies: user.allergies,
            avatar_url: user.avatar_url,
            is_email_verified: user.is_email_verified,
            two_factor_enabled: user.two_factor_enabled,
            created_at: user.created_at.to_rfc3339(),
        })
    }

    /// Update user profile fields
    pub async fn update_profile(
        &self,
        user_id: Uuid,
        req: UpdateProfileRequest,
    ) -> Result<ProfileResponse, AppError> {
        let user = user::Entity::find_by_id(user_id)
            .one(&self.db)
            .await?
            .ok_or(AppError::NotFound("User".into()))?;

        let now = Utc::now().fixed_offset();
        let mut active: user::ActiveModel = user.into();

        if let Some(name) = req.name {
            active.name = Set(Some(name));
        }
        if let Some(size) = req.household_size {
            active.household_size = Set(size);
        }
        if let Some(restrictions) = req.dietary_restrictions {
            active.dietary_restrictions = Set(Some(restrictions));
        }
        if let Some(allergies) = req.allergies {
            active.allergies = Set(Some(allergies));
        }
        if let Some(avatar) = req.avatar_url {
            active.avatar_url = Set(Some(avatar));
        }
        active.updated_at = Set(now);

        let saved = active.update(&self.db).await?;

        Ok(ProfileResponse {
            id: saved.id.to_string(),
            email: saved.email,
            name: saved.name,
            household_size: saved.household_size,
            dietary_restrictions: saved.dietary_restrictions,
            allergies: saved.allergies,
            avatar_url: saved.avatar_url,
            is_email_verified: saved.is_email_verified,
            two_factor_enabled: saved.two_factor_enabled,
            created_at: saved.created_at.to_rfc3339(),
        })
    }
}
