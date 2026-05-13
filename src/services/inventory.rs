//! Inventory Service — CRUD for user food stock with expiry tracking

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set,
};
use uuid::Uuid;

use crate::entity::{inventory_item, ingredient};
use crate::errors::AppError;
use crate::models::inventory::*;

pub struct InventoryService {
    db: DatabaseConnection,
}

impl InventoryService {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// List all inventory items for a user with expiry metadata
    pub async fn list(&self, user_id: Uuid) -> Result<Vec<InventoryItemResponse>, AppError> {
        let items = inventory_item::Entity::find()
            .filter(inventory_item::Column::UserId.eq(user_id))
            .all(&self.db)
            .await?;

        // Bulk load ingredient names
        let ingredient_ids: Vec<i64> = items.iter().map(|i| i.ingredient_id).collect();
        let ingredients: std::collections::HashMap<i64, String> =
            ingredient::Entity::find()
                .filter(ingredient::Column::Id.is_in(ingredient_ids))
                .all(&self.db)
                .await?
                .into_iter()
                .map(|ing| (ing.id, ing.name))
                .collect();

        let today = Utc::now().date_naive();

        let responses = items
            .into_iter()
            .map(|item| {
                let days_until_expiry = item.expiry_date.map(|d| (d - today).num_days());
                let expiry_warning = days_until_expiry.map(|d| d <= 5).unwrap_or(false);
                let ingredient_name = ingredients
                    .get(&item.ingredient_id)
                    .cloned()
                    .unwrap_or_default();

                InventoryItemResponse {
                    id: item.id,
                    ingredient_id: item.ingredient_id,
                    ingredient_name,
                    custom_name: item.custom_name,
                    quantity: item.quantity,
                    unit: item.unit,
                    expiry_date: item.expiry_date,
                    storage_location: item.storage_location,
                    days_until_expiry,
                    expiry_warning,
                }
            })
            .collect();

        Ok(responses)
    }

    /// Add a new item to inventory
    pub async fn add(
        &self,
        user_id: Uuid,
        req: AddInventoryItem,
    ) -> Result<InventoryItemResponse, AppError> {
        // Verify ingredient exists
        let ing = ingredient::Entity::find_by_id(req.ingredient_id)
            .one(&self.db)
            .await?
            .ok_or(AppError::NotFound("Ingredient".into()))?;

        let now = Utc::now().fixed_offset();
        let today = Utc::now().date_naive();

        let new_item = inventory_item::ActiveModel {
            user_id: Set(user_id),
            ingredient_id: Set(req.ingredient_id),
            custom_name: Set(req.custom_name.clone()),
            quantity: Set(req.quantity),
            unit: Set(req.unit.clone()),
            expiry_date: Set(req.expiry_date),
            storage_location: Set(req.storage_location.clone()),
            added_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };

        let saved = new_item.insert(&self.db).await?;

        let days_until_expiry = saved.expiry_date.map(|d| (d - today).num_days());
        let expiry_warning = days_until_expiry.map(|d| d <= 5).unwrap_or(false);

        Ok(InventoryItemResponse {
            id: saved.id,
            ingredient_id: saved.ingredient_id,
            ingredient_name: ing.name,
            custom_name: saved.custom_name,
            quantity: saved.quantity,
            unit: saved.unit,
            expiry_date: saved.expiry_date,
            storage_location: saved.storage_location,
            days_until_expiry,
            expiry_warning,
        })
    }

    /// Update an existing inventory item (quantity, expiry, etc.)
    pub async fn update(
        &self,
        user_id: Uuid,
        item_id: i64,
        req: UpdateInventoryItem,
    ) -> Result<InventoryItemResponse, AppError> {
        let item = inventory_item::Entity::find_by_id(item_id)
            .one(&self.db)
            .await?
            .ok_or(AppError::NotFound("Inventory item".into()))?;

        // Only allow user to update their own items
        if item.user_id != user_id {
            return Err(AppError::AuthenticationFailed);
        }

        let ing = ingredient::Entity::find_by_id(item.ingredient_id)
            .one(&self.db)
            .await?
            .ok_or(AppError::NotFound("Ingredient".into()))?;

        let now = Utc::now().fixed_offset();
        let today = Utc::now().date_naive();

        let mut active: inventory_item::ActiveModel = item.into();
        if let Some(q) = req.quantity {
            active.quantity = Set(q);
        }
        if let Some(u) = req.unit {
            active.unit = Set(u);
        }
        if let Some(e) = req.expiry_date {
            active.expiry_date = Set(Some(e));
        }
        if let Some(loc) = req.storage_location {
            active.storage_location = Set(Some(loc));
        }
        active.updated_at = Set(now);

        let saved = active.update(&self.db).await?;

        let days_until_expiry = saved.expiry_date.map(|d| (d - today).num_days());
        let expiry_warning = days_until_expiry.map(|d| d <= 5).unwrap_or(false);

        Ok(InventoryItemResponse {
            id: saved.id,
            ingredient_id: saved.ingredient_id,
            ingredient_name: ing.name,
            custom_name: saved.custom_name,
            quantity: saved.quantity,
            unit: saved.unit,
            expiry_date: saved.expiry_date,
            storage_location: saved.storage_location,
            days_until_expiry,
            expiry_warning,
        })
    }

    /// Remove an item from inventory
    pub async fn delete(&self, user_id: Uuid, item_id: i64) -> Result<(), AppError> {
        let item = inventory_item::Entity::find_by_id(item_id)
            .one(&self.db)
            .await?
            .ok_or(AppError::NotFound("Inventory item".into()))?;

        if item.user_id != user_id {
            return Err(AppError::AuthenticationFailed);
        }

        inventory_item::Entity::delete_by_id(item_id)
            .exec(&self.db)
            .await?;

        Ok(())
    }

    /// Get items expiring within the next N days (for alerts)
    pub async fn expiring_soon(
        &self,
        user_id: Uuid,
        days: i64,
    ) -> Result<Vec<InventoryItemResponse>, AppError> {
        let all = self.list(user_id).await?;
        Ok(all
            .into_iter()
            .filter(|item| {
                item.days_until_expiry
                    .map(|d| d >= 0 && d <= days)
                    .unwrap_or(false)
            })
            .collect())
    }

    /// Deduct ingredients from inventory after cooking a recipe
    /// Called automatically when user marks a recipe as cooked
    pub async fn deduct_for_recipe(
        &self,
        user_id: Uuid,
        recipe_id: i64,
        servings_made: i32,
        recipe_servings: i32,
    ) -> Result<(), AppError> {
        use crate::entity::recipe_ingredient;

        let recipe_ings = recipe_ingredient::Entity::find()
            .filter(recipe_ingredient::Column::RecipeId.eq(recipe_id))
            .all(&self.db)
            .await?;

        let scaling = servings_made as f64 / recipe_servings.max(1) as f64;

        for ri in recipe_ings {
            if let Some(grams) = ri.quantity_grams {
                let needed = grams * rust_decimal::Decimal::try_from(scaling).unwrap_or_default();

                // Find this ingredient in user's inventory (prioritise earliest expiry)
                if let Some(inv_item) = inventory_item::Entity::find()
                    .filter(inventory_item::Column::UserId.eq(user_id))
                    .filter(inventory_item::Column::IngredientId.eq(ri.ingredient_id))
                    .one(&self.db)
                    .await?
                {
                    let new_quantity = (inv_item.quantity - needed).max(rust_decimal::Decimal::ZERO);
                    let now = Utc::now().fixed_offset();

                    if new_quantity == rust_decimal::Decimal::ZERO {
                        // Remove item if fully consumed
                        inventory_item::Entity::delete_by_id(inv_item.id)
                            .exec(&self.db)
                            .await?;
                    } else {
                        let mut active: inventory_item::ActiveModel = inv_item.into();
                        active.quantity = Set(new_quantity);
                        active.updated_at = Set(now);
                        active.update(&self.db).await?;
                    }
                }
            }
        }

        Ok(())
    }
}
