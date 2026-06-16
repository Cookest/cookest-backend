//! Onboarding service — handles the initial user preference setup flow

use chrono::Utc;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::entity::user::{ActiveModel as UserActiveModel, Entity as User, UserResponse};
use cookest_shared::errors::AppError;

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct OnboardingRequest {
    pub name: Option<String>,
    pub household_size: Option<i32>,
    pub dietary_restrictions: Option<Vec<String>>,
    pub allergies: Option<Vec<String>>,
    pub cooking_skill_level: Option<String>,
    pub preferred_cuisines: Option<Vec<String>>,
    pub health_goals: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weekly_budget: Option<f64>,
    pub preferred_time_per_meal_min: Option<i32>,
    pub meal_frequency: Option<i32>,
    /// Recipe IDs the user liked during the taste-calibration swipe step.
    /// Merged into `taste_profile` so the recommender can use them immediately.
    pub liked_recipe_ids: Option<Vec<i64>>,
}

pub struct OnboardingService {
    db: DatabaseConnection,
}

impl OnboardingService {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// Complete the onboarding flow — sets all profile preferences and marks onboarding done
    pub async fn complete_onboarding(
        &self,
        user_id: Uuid,
        req: OnboardingRequest,
    ) -> Result<UserResponse, AppError> {
        let user = User::find_by_id(user_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound("User".to_string()))?;

        // Capture before `user` is moved — ActiveModel fields are Unchanged, not Set.
        let existing_taste_profile = user.taste_profile.clone();

        let mut active: UserActiveModel = user.into();

        if let Some(name) = req.name {
            active.name = Set(Some(name));
        }
        if let Some(size) = req.household_size {
            active.household_size = Set(size.max(1));
        }
        if let Some(dr) = req.dietary_restrictions {
            active.dietary_restrictions = Set(Some(dr));
        }
        if let Some(al) = req.allergies {
            active.allergies = Set(Some(al));
        }
        if let Some(skill) = req.cooking_skill_level {
            active.cooking_skill_level = Set(Some(skill));
        }
        if let Some(cuisines) = req.preferred_cuisines {
            active.preferred_cuisines = Set(Some(cuisines));
        }
        if let Some(goals) = req.health_goals {
            active.health_goals = Set(Some(goals));
        }
        if let Some(budget) = req.weekly_budget {
            active.weekly_budget = Set(Some(rust_decimal::Decimal::try_from(budget).unwrap_or_default()));
        }
        if let Some(time) = req.preferred_time_per_meal_min {
            active.preferred_time_per_meal_min = Set(Some(time));
        }
        if let Some(freq) = req.meal_frequency {
            active.meal_frequency = Set(Some(freq.max(1).min(21)));
        }

        // Merge taste-calibration likes into taste_profile (same JSON shape the
        // /api/me/swipe endpoint produces) so the recommender can use them.
        if let Some(liked) = req.liked_recipe_ids {
            if !liked.is_empty() {
                let mut profile = existing_taste_profile;
                if !profile.get("liked_recipe_ids").map_or(false, |v| v.is_array()) {
                    profile["liked_recipe_ids"] = serde_json::json!([]);
                }
                if !profile.get("swipe_count").map_or(false, |v| v.is_number()) {
                    profile["swipe_count"] = serde_json::json!(0);
                }

                if let Some(arr) = profile["liked_recipe_ids"].as_array_mut() {
                    let mut existing: std::collections::HashSet<i64> =
                        arr.iter().filter_map(|v| v.as_i64()).collect();
                    let mut added = 0i64;
                    for id in liked {
                        if existing.insert(id) {
                            arr.push(serde_json::json!(id));
                            added += 1;
                        }
                    }
                    if let Some(count) = profile["swipe_count"].as_i64() {
                        profile["swipe_count"] = serde_json::json!(count + added);
                    }
                }

                active.taste_profile = Set(profile);
            }
        }

        active.onboarding_completed = Set(true);
        active.updated_at = Set(Utc::now().fixed_offset());

        let updated = active.update(&self.db).await?;
        Ok(UserResponse::from(updated))
    }
}
