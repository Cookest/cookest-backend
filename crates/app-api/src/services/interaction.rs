//! Interaction Service — handles ratings, favourites, cooking history
//! Also triggers PreferenceService to update ML weights on every interaction

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set,
};
use uuid::Uuid;

use crate::entity::{recipe, recipe_rating, user_favorite, cooking_history};
use cookest_shared::errors::AppError;
use crate::models::interaction::*;
use crate::services::{InventoryService, PreferenceService, RecipeService};
use crate::services::preference::PreferenceSignal;
use crate::handlers::browse::FoodApiClient;

pub struct InteractionService {
    db: DatabaseConnection,
    preference_service: PreferenceService,
    inventory_service: InventoryService,
    food_api_client: FoodApiClient,
}

impl InteractionService {
    pub fn new(db: DatabaseConnection, food_api_client: FoodApiClient) -> Self {
        let db2 = db.clone();
        let db3 = db.clone();
        Self {
            db,
            preference_service: PreferenceService::new(db2),
            inventory_service: InventoryService::new(db3, food_api_client.clone()),
            food_api_client,
        }
    }

    /// Rate a recipe — saves rating and triggers ML preference update
    pub async fn rate_recipe(
        &self,
        user_id: Uuid,
        recipe_id: i64,
        rating: i16,
        comment: Option<String>,
    ) -> Result<InteractionResponse, AppError> {
        // Verify recipe exists, importing it if missing
        if recipe::Entity::find_by_id(recipe_id).one(&self.db).await?.is_none() {
            let recipe_service = RecipeService::new(self.db.clone(), self.food_api_client.clone());
            recipe_service.get_recipe_or_import(recipe_id).await?;
        }

        let now = Utc::now().fixed_offset();

        // Upsert: delete old rating if it exists
        recipe_rating::Entity::delete_many()
            .filter(recipe_rating::Column::UserId.eq(user_id))
            .filter(recipe_rating::Column::RecipeId.eq(recipe_id))
            .exec(&self.db)
            .await?;

        let new_rating = recipe_rating::ActiveModel {
            user_id: Set(user_id),
            recipe_id: Set(recipe_id),
            rating: Set(rating),
            comment: Set(comment),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        new_rating.insert(&self.db).await?;

        // Update aggregated rating on recipe
        self.update_recipe_avg_rating(recipe_id).await?;

        // Trigger ML update
        self.preference_service
            .record_interaction(user_id, recipe_id, PreferenceSignal::Rated(rating))
            .await?;

        Ok(InteractionResponse {
            message: format!("Recipe rated {} stars", rating),
        })
    }

    /// Toggle favourite — add if not present, remove if already saved
    pub async fn toggle_favourite(
        &self,
        user_id: Uuid,
        recipe_id: i64,
    ) -> Result<FavouriteResponse, AppError> {
        // Verify recipe exists, importing it if missing
        if recipe::Entity::find_by_id(recipe_id).one(&self.db).await?.is_none() {
            let recipe_service = RecipeService::new(self.db.clone(), self.food_api_client.clone());
            recipe_service.get_recipe_or_import(recipe_id).await?;
        }

        let existing = user_favorite::Entity::find()
            .filter(user_favorite::Column::UserId.eq(user_id))
            .filter(user_favorite::Column::RecipeId.eq(recipe_id))
            .one(&self.db)
            .await?;

        if let Some(fav) = existing {
            // Remove favourite
            user_favorite::Entity::delete_by_id(fav.id)
                .exec(&self.db)
                .await?;

            Ok(FavouriteResponse {
                recipe_id,
                is_favourited: false,
            })
        } else {
            // Add favourite + trigger ML signal
            let now = Utc::now().fixed_offset();
            let fav = user_favorite::ActiveModel {
                user_id: Set(user_id),
                recipe_id: Set(recipe_id),
                saved_at: Set(now),
                ..Default::default()
            };
            fav.insert(&self.db).await?;

            self.preference_service
                .record_interaction(user_id, recipe_id, PreferenceSignal::Favourited)
                .await?;

            Ok(FavouriteResponse {
                recipe_id,
                is_favourited: true,
            })
        }
    }

    /// Log that a user cooked a recipe — deducts inventory + triggers ML update
    pub async fn mark_cooked(
        &self,
        user_id: Uuid,
        recipe_id: i64,
        servings_made: i32,
    ) -> Result<InteractionResponse, AppError> {
        let recipe = match recipe::Entity::find_by_id(recipe_id).one(&self.db).await? {
            Some(r) => r,
            None => {
                let recipe_service = RecipeService::new(self.db.clone(), self.food_api_client.clone());
                recipe_service.get_recipe_or_import(recipe_id).await?
            }
        };

        // Log cooking history first so deductions can reference it (enables undo)
        let now = Utc::now().fixed_offset();
        let history = cooking_history::ActiveModel {
            user_id: Set(user_id),
            recipe_id: Set(recipe_id),
            servings_made: Set(servings_made),
            inventory_deducted: Set(true),
            cooked_at: Set(now),
            ..Default::default()
        };
        let saved_history = history.insert(&self.db).await?;

        // Deduct ingredients from inventory, linking the audit to this cook
        self.inventory_service
            .deduct_for_recipe(
                user_id,
                recipe_id,
                Some(servings_made),
                recipe.servings,
                Some(saved_history.id),
            )
            .await?;

        // Trigger ML update
        self.preference_service
            .record_interaction(user_id, recipe_id, PreferenceSignal::Cooked)
            .await?;

        Ok(InteractionResponse {
            message: "Recipe marked as cooked. Inventory updated.".to_string(),
        })
    }

    /// Undo a cook: restore the deducted inventory and remove the cook event.
    pub async fn undo_cook(
        &self,
        user_id: Uuid,
        history_id: i64,
    ) -> Result<InteractionResponse, AppError> {
        let history = cooking_history::Entity::find_by_id(history_id)
            .one(&self.db)
            .await?
            .ok_or(AppError::NotFound("Cooking history".into()))?;
        if history.user_id != user_id {
            return Err(AppError::AuthenticationFailed);
        }

        // Restore inventory from the audit rows (also clears them), then drop the event.
        self.inventory_service.undo_deduction(user_id, history_id).await?;
        cooking_history::Entity::delete_by_id(history_id)
            .exec(&self.db)
            .await?;

        Ok(InteractionResponse {
            message: "Cook undone. Inventory restored.".to_string(),
        })
    }

    /// Get user's cooking history
    pub async fn get_cooking_history(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<CookingHistoryItem>, AppError> {
        let history = cooking_history::Entity::find()
            .filter(cooking_history::Column::UserId.eq(user_id))
            .all(&self.db)
            .await?;

        let recipe_ids: Vec<i64> = history.iter().map(|h| h.recipe_id).collect();
        let recipes: std::collections::HashMap<i64, String> =
            recipe::Entity::find()
                .filter(recipe::Column::Id.is_in(recipe_ids))
                .all(&self.db)
                .await?
                .into_iter()
                .map(|r| (r.id, r.name))
                .collect();

        Ok(history
            .into_iter()
            .map(|h| CookingHistoryItem {
                id: h.id,
                recipe_id: h.recipe_id,
                recipe_name: recipes.get(&h.recipe_id).cloned().unwrap_or_default(),
                servings_made: h.servings_made,
                inventory_deducted: h.inventory_deducted,
                cooked_at: h.cooked_at.to_rfc3339(),
            })
            .collect())
    }

    /// Recalculate and update a recipe's average rating
    async fn update_recipe_avg_rating(&self, recipe_id: i64) -> Result<(), AppError> {

        let ratings = recipe_rating::Entity::find()
            .filter(recipe_rating::Column::RecipeId.eq(recipe_id))
            .all(&self.db)
            .await?;

        let count = ratings.len() as i32;
        let avg = if count > 0 {
            let sum: i32 = ratings.iter().map(|r| r.rating as i32).sum();
            Some(rust_decimal::Decimal::from(sum) / rust_decimal::Decimal::from(count))
        } else {
            None
        };

        let now = Utc::now().fixed_offset();
        let recipe = recipe::Entity::find_by_id(recipe_id)
            .one(&self.db)
            .await?
            .ok_or(AppError::NotFound("Recipe".into()))?;

        let mut active: recipe::ActiveModel = recipe.into();
        active.average_rating = Set(avg);
        active.rating_count = Set(count);
        active.updated_at = Set(now);
        active.update(&self.db).await?;

        Ok(())
    }

    /// Get all recipes the user has favourited
    pub async fn get_favourites(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<serde_json::Value>, AppError> {
        let favourites = user_favorite::Entity::find()
            .filter(user_favorite::Column::UserId.eq(user_id))
            .all(&self.db)
            .await?;

        let recipe_ids: Vec<i64> = favourites.iter().map(|f| f.recipe_id).collect();

        let recipes = recipe::Entity::find()
            .filter(recipe::Column::Id.is_in(recipe_ids))
            .all(&self.db)
            .await?;

        let result = favourites
            .into_iter()
            .zip(recipes.into_iter())
            .map(|(fav, r)| {
                serde_json::json!({
                    "favourite_id": fav.id,
                    "saved_at": fav.saved_at.to_rfc3339(),
                    "recipe": {
                        "id": r.id,
                        "name": r.name,
                        "cuisine": r.cuisine,
                        "category": r.category,
                        "total_time_min": r.total_time_min,
                        "difficulty": r.difficulty,
                        "average_rating": r.average_rating,
                        "rating_count": r.rating_count,
                    }
                })
            })
            .collect();

        Ok(result)
    }
}
