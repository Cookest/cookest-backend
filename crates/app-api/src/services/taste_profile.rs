//! Taste profile service — records recipe swipe events and maintains a user's taste profile.

use chrono::Utc;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::user::{ActiveModel as UserActiveModel, Entity as User};
use cookest_shared::errors::AppError;

#[derive(Debug, Deserialize, Serialize)]
pub struct SwipeRequest {
    pub recipe_id: i32,
    pub direction: SwipeDirection,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SwipeDirection {
    Like,
    Dislike,
}

pub struct TasteProfileService {
    db: DatabaseConnection,
}

impl TasteProfileService {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// Record a recipe swipe and update the user's taste_profile JSON.
    ///
    /// The taste_profile is a JSON object:
    /// ```json
    /// {
    ///   "liked_recipe_ids": [1, 2, 3],
    ///   "disliked_recipe_ids": [4, 5],
    ///   "swipe_count": 10
    /// }
    /// ```
    pub async fn record_swipe(
        &self,
        user_id: Uuid,
        req: SwipeRequest,
    ) -> Result<(), AppError> {
        let user = User::find_by_id(user_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound("User".to_string()))?;

        let mut profile = user.taste_profile.clone();

        // Ensure arrays exist
        if profile.get("liked_recipe_ids").is_none() {
            profile["liked_recipe_ids"] = serde_json::json!([]);
        }
        if profile.get("disliked_recipe_ids").is_none() {
            profile["disliked_recipe_ids"] = serde_json::json!([]);
        }
        if profile.get("swipe_count").is_none() {
            profile["swipe_count"] = serde_json::json!(0);
        }

        let key = match req.direction {
            SwipeDirection::Like => "liked_recipe_ids",
            SwipeDirection::Dislike => "disliked_recipe_ids",
        };

        if let Some(arr) = profile[key].as_array_mut() {
            arr.push(serde_json::json!(req.recipe_id));
        }

        if let Some(count) = profile["swipe_count"].as_i64() {
            profile["swipe_count"] = serde_json::json!(count + 1);
        }

        let mut active: UserActiveModel = user.into();
        active.taste_profile = Set(profile);
        active.updated_at = Set(Utc::now().fixed_offset());
        active.update(&self.db).await?;

        Ok(())
    }
}
