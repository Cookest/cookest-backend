//! Inventory Service — CRUD for user food stock with expiry tracking

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set,
    QueryOrder, QuerySelect, PaginatorTrait,
};
use uuid::Uuid;
use rust_decimal::Decimal;
use std::str::FromStr;

use crate::entity::{inventory_item, inventory_deduction, ingredient, recipe_ingredient, recipe, user};
use cookest_shared::errors::AppError;
use crate::models::inventory::*;
use crate::services::scan::BulkAddItem;

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

    /// Resolve-or-create an app-db ingredient from a FatSecret food id, then add to inventory.
    /// Backs the barcode add-to-pantry flow so inventory references a stable local ingredient.
    #[allow(clippy::too_many_arguments)]
    pub async fn add_from_fatsecret(
        &self,
        user_id: Uuid,
        fs_food_id: i64,
        name: String,
        category: Option<String>,
        quantity: f64,
        unit: String,
        storage_location: Option<String>,
        expiry_date: Option<chrono::NaiveDate>,
    ) -> Result<InventoryItemResponse, AppError> {
        // 1. Find by FatSecret id, else by exact name (backfilling fs_food_id), else create.
        let by_fs = ingredient::Entity::find()
            .filter(ingredient::Column::FsFoodId.eq(fs_food_id))
            .one(&self.db)
            .await?;

        let ing = match by_fs {
            Some(i) => i,
            None => {
                let by_name = ingredient::Entity::find()
                    .filter(ingredient::Column::Name.eq(name.trim()))
                    .one(&self.db)
                    .await?;
                match by_name {
                    Some(i) => {
                        let mut active: ingredient::ActiveModel = i.into();
                        active.fs_food_id = Set(Some(fs_food_id));
                        active.update(&self.db).await?
                    }
                    None => {
                        let now = Utc::now().fixed_offset();
                        let new_ing = ingredient::ActiveModel {
                            name: Set(name.trim().to_string()),
                            category: Set(category),
                            fdc_id: Set(None),
                            off_id: Set(None),
                            fs_food_id: Set(Some(fs_food_id)),
                            created_at: Set(now),
                            ..Default::default()
                        };
                        new_ing.insert(&self.db).await?
                    }
                }
            }
        };

        // 2. Add the inventory item referencing the resolved ingredient.
        let dec_qty = Decimal::from_str(&quantity.to_string()).unwrap_or(Decimal::ONE);
        self.add(
            user_id,
            AddInventoryItem {
                ingredient_id: ing.id,
                custom_name: None,
                quantity: dec_qty,
                unit,
                expiry_date,
                storage_location,
            },
        )
        .await
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

    /// Deduct ingredients from inventory after cooking a recipe.
    /// FIFO/expiry-aware: consumes the earliest-expiring stock first across
    /// multiple rows, writing one `inventory_deductions` audit row per touched
    /// item so the cook can be undone. When `servings_made` is `None`, falls
    /// back to the user's household size.
    pub async fn deduct_for_recipe(
        &self,
        user_id: Uuid,
        recipe_id: i64,
        servings_made: Option<i32>,
        recipe_servings: i32,
        cooking_history_id: Option<i64>,
    ) -> Result<(), AppError> {
        // Effective servings: explicit value, else the user's household size, else 1.
        let eff_servings = match servings_made {
            Some(s) if s > 0 => s,
            _ => user::Entity::find_by_id(user_id)
                .one(&self.db)
                .await?
                .map(|u| u.household_size)
                .unwrap_or(1),
        };

        let recipe_ings = recipe_ingredient::Entity::find()
            .filter(recipe_ingredient::Column::RecipeId.eq(recipe_id))
            .all(&self.db)
            .await?;

        let denom = Decimal::from(recipe_servings.max(1));
        let now = Utc::now().fixed_offset();

        for ri in recipe_ings {
            let Some(grams) = ri.quantity_grams else { continue };
            let mut needed = grams * Decimal::from(eff_servings) / denom;
            if needed <= Decimal::ZERO {
                continue;
            }

            // Earliest expiry first (Postgres ASC sorts NULLs last), then oldest added.
            let candidates = inventory_item::Entity::find()
                .filter(inventory_item::Column::UserId.eq(user_id))
                .filter(inventory_item::Column::IngredientId.eq(ri.ingredient_id))
                .order_by_asc(inventory_item::Column::ExpiryDate)
                .order_by_asc(inventory_item::Column::AddedAt)
                .all(&self.db)
                .await?;

            for inv_item in candidates {
                if needed <= Decimal::ZERO {
                    break;
                }
                let qty_before = inv_item.quantity;
                let take = needed.min(qty_before);
                let new_quantity = qty_before - take;
                let was_deleted = new_quantity <= Decimal::ZERO;

                inventory_deduction::ActiveModel {
                    user_id: Set(user_id),
                    inventory_item_id: Set(Some(inv_item.id)),
                    ingredient_id: Set(ri.ingredient_id),
                    recipe_id: Set(Some(recipe_id)),
                    cooking_history_id: Set(cooking_history_id),
                    qty_before: Set(qty_before),
                    qty_deducted: Set(take),
                    unit: Set(inv_item.unit.clone()),
                    was_deleted: Set(was_deleted),
                    reason: Set("cook".to_string()),
                    created_at: Set(now),
                    ..Default::default()
                }
                .insert(&self.db)
                .await?;

                if was_deleted {
                    inventory_item::Entity::delete_by_id(inv_item.id)
                        .exec(&self.db)
                        .await?;
                } else {
                    let mut active: inventory_item::ActiveModel = inv_item.into();
                    active.quantity = Set(new_quantity);
                    active.updated_at = Set(now);
                    active.update(&self.db).await?;
                }

                needed -= take;
            }
        }

        Ok(())
    }

    /// Manually consume a quantity of a single pantry item (writes an audit row).
    pub async fn consume(
        &self,
        user_id: Uuid,
        item_id: i64,
        quantity: Decimal,
    ) -> Result<ConsumeResponse, AppError> {
        let item = inventory_item::Entity::find_by_id(item_id)
            .one(&self.db)
            .await?
            .ok_or(AppError::NotFound("Inventory item".into()))?;
        if item.user_id != user_id {
            return Err(AppError::AuthenticationFailed);
        }

        let qty_before = item.quantity;
        let take = quantity.min(qty_before).max(Decimal::ZERO);
        let new_quantity = qty_before - take;
        let was_deleted = new_quantity <= Decimal::ZERO;
        let now = Utc::now().fixed_offset();
        let today = Utc::now().date_naive();

        let ing_name = ingredient::Entity::find_by_id(item.ingredient_id)
            .one(&self.db)
            .await?
            .map(|i| i.name)
            .unwrap_or_default();

        inventory_deduction::ActiveModel {
            user_id: Set(user_id),
            inventory_item_id: Set(Some(item_id)),
            ingredient_id: Set(item.ingredient_id),
            recipe_id: Set(None),
            cooking_history_id: Set(None),
            qty_before: Set(qty_before),
            qty_deducted: Set(take),
            unit: Set(item.unit.clone()),
            was_deleted: Set(was_deleted),
            reason: Set("manual".to_string()),
            created_at: Set(now),
            ..Default::default()
        }
        .insert(&self.db)
        .await?;

        if was_deleted {
            inventory_item::Entity::delete_by_id(item_id).exec(&self.db).await?;
            Ok(ConsumeResponse { deleted: true, consumed: take, item: None })
        } else {
            let mut active: inventory_item::ActiveModel = item.into();
            active.quantity = Set(new_quantity);
            active.updated_at = Set(now);
            let saved = active.update(&self.db).await?;
            let days_until_expiry = saved.expiry_date.map(|d| (d - today).num_days());
            let expiry_warning = days_until_expiry.map(|d| d <= 5).unwrap_or(false);
            Ok(ConsumeResponse {
                deleted: false,
                consumed: take,
                item: Some(InventoryItemResponse {
                    id: saved.id,
                    ingredient_id: saved.ingredient_id,
                    ingredient_name: ing_name,
                    custom_name: saved.custom_name,
                    quantity: saved.quantity,
                    unit: saved.unit,
                    expiry_date: saved.expiry_date,
                    storage_location: saved.storage_location,
                    days_until_expiry,
                    expiry_warning,
                }),
            })
        }
    }

    /// Restore inventory from the audit rows of a cook, then drop those rows.
    pub async fn undo_deduction(
        &self,
        user_id: Uuid,
        cooking_history_id: i64,
    ) -> Result<(), AppError> {
        let deductions = inventory_deduction::Entity::find()
            .filter(inventory_deduction::Column::CookingHistoryId.eq(cooking_history_id))
            .filter(inventory_deduction::Column::UserId.eq(user_id))
            .all(&self.db)
            .await?;

        let now = Utc::now().fixed_offset();

        for d in &deductions {
            // If the row still exists, add the deducted amount back; else recreate it.
            let existing = match d.inventory_item_id {
                Some(id) => inventory_item::Entity::find_by_id(id).one(&self.db).await?,
                None => None,
            };
            match existing {
                Some(item) => {
                    let restored = item.quantity + d.qty_deducted;
                    let mut active: inventory_item::ActiveModel = item.into();
                    active.quantity = Set(restored);
                    active.updated_at = Set(now);
                    active.update(&self.db).await?;
                }
                None => {
                    let qty = if d.was_deleted { d.qty_before } else { d.qty_deducted };
                    inventory_item::ActiveModel {
                        user_id: Set(user_id),
                        ingredient_id: Set(d.ingredient_id),
                        custom_name: Set(None),
                        quantity: Set(qty),
                        unit: Set(d.unit.clone()),
                        expiry_date: Set(None),
                        storage_location: Set(None),
                        added_at: Set(now),
                        updated_at: Set(now),
                        ..Default::default()
                    }
                    .insert(&self.db)
                    .await?;
                }
            }
        }

        inventory_deduction::Entity::delete_many()
            .filter(inventory_deduction::Column::CookingHistoryId.eq(cooking_history_id))
            .filter(inventory_deduction::Column::UserId.eq(user_id))
            .exec(&self.db)
            .await?;

        Ok(())
    }

    /// Recent deduction audit entries for a user (most recent first).
    pub async fn get_deductions(
        &self,
        user_id: Uuid,
        limit: u64,
    ) -> Result<Vec<inventory_deduction::Model>, AppError> {
        let rows = inventory_deduction::Entity::find()
            .filter(inventory_deduction::Column::UserId.eq(user_id))
            .order_by_desc(inventory_deduction::Column::CreatedAt)
            .limit(limit)
            .all(&self.db)
            .await?;
        Ok(rows)
    }

    /// Find-or-create an ingredient by name, then add to inventory.
    /// Used for quick-add and bulk-add from scan results.
    pub async fn quick_add(
        &self,
        user_id: Uuid,
        name: String,
        quantity: f64,
        unit: String,
        storage_location: Option<String>,
        expiry_date: Option<chrono::NaiveDate>,
    ) -> Result<InventoryItemResponse, AppError> {
        let name_lower = name.trim().to_lowercase();

        // Try to find existing ingredient (case-insensitive)
        let existing = ingredient::Entity::find()
            .filter(ingredient::Column::Name.like(format!("%{}%", &name_lower)))
            .order_by_asc(ingredient::Column::Name)
            .one(&self.db)
            .await?;

        let ing = match existing {
            Some(i) => i,
            None => {
                // Create a new ingredient on-the-fly
                let now = Utc::now().fixed_offset();
                let new_ing = ingredient::ActiveModel {
                    name: Set(name.trim().to_string()),
                    category: Set(None),
                    fdc_id: Set(None),
                    off_id: Set(None),
                    created_at: Set(now),
                    ..Default::default()
                };
                new_ing.insert(&self.db).await?
            }
        };

        let dec_qty = Decimal::from_str(&quantity.to_string())
            .unwrap_or(Decimal::ONE);
        let now = Utc::now().fixed_offset();
        let today = Utc::now().date_naive();

        let new_item = inventory_item::ActiveModel {
            user_id: Set(user_id),
            ingredient_id: Set(ing.id),
            custom_name: Set(None),
            quantity: Set(dec_qty),
            unit: Set(unit.clone()),
            expiry_date: Set(expiry_date),
            storage_location: Set(storage_location.clone()),
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

    /// Bulk-add items (from scan results). Find-or-create ingredients by name.
    pub async fn bulk_add(
        &self,
        user_id: Uuid,
        items: Vec<BulkAddItem>,
    ) -> Result<Vec<InventoryItemResponse>, AppError> {
        let mut results = Vec::with_capacity(items.len());
        for item in items {
            let expiry = item.expiry_date.as_deref()
                .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
            match self.quick_add(
                user_id,
                item.name,
                item.quantity,
                item.unit,
                item.storage_location,
                expiry,
            ).await {
                Ok(inv) => results.push(inv),
                Err(e) => tracing::warn!("bulk_add: skipping item due to error: {}", e),
            }
        }
        Ok(results)
    }

    /// Return recipes that can be (partially) made with the user's current inventory.
    /// Scores each recipe by the fraction of its ingredients that are in the pantry.
    pub async fn recipe_suggestions(
        &self,
        user_id: Uuid,
        min_match: f64,
        limit: u64,
    ) -> Result<Vec<RecipeSuggestion>, AppError> {
        use std::collections::HashMap;

        // 1. Get the user's inventory (set of ingredient_ids)
        let inv_items = inventory_item::Entity::find()
            .filter(inventory_item::Column::UserId.eq(user_id))
            .all(&self.db)
            .await?;

        if inv_items.is_empty() {
            return Ok(vec![]);
        }

        let pantry_ids: std::collections::HashSet<i64> =
            inv_items.iter().map(|i| i.ingredient_id).collect();

        // 2. Load all recipe ingredients grouped by recipe_id (limit to first 500 recipes for perf)
        let recipe_ings = recipe_ingredient::Entity::find()
            .all(&self.db)
            .await?;

        let mut recipe_ing_map: HashMap<i64, Vec<i64>> = HashMap::new();
        for ri in &recipe_ings {
            recipe_ing_map
                .entry(ri.recipe_id)
                .or_default()
                .push(ri.ingredient_id);
        }

        // 3. Score each recipe
        let mut scored: Vec<(i64, usize, usize)> = recipe_ing_map
            .iter()
            .filter_map(|(recipe_id, ing_ids)| {
                let total = ing_ids.len();
                if total == 0 {
                    return None;
                }
                let have = ing_ids.iter().filter(|id| pantry_ids.contains(id)).count();
                let score = have as f64 / total as f64;
                if score >= min_match {
                    Some((*recipe_id, have, total))
                } else {
                    None
                }
            })
            .collect();

        // Sort by score desc, then by absolute count of matching ingredients
        scored.sort_by(|a, b| {
            let score_a = a.1 as f64 / a.2 as f64;
            let score_b = b.1 as f64 / b.2 as f64;
            score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
        });

        scored.truncate(limit as usize);

        if scored.is_empty() {
            return Ok(vec![]);
        }

        // 4. Load recipe details
        let recipe_ids: Vec<i64> = scored.iter().map(|(id, _, _)| *id).collect();
        let recipes = recipe::Entity::find()
            .filter(recipe::Column::Id.is_in(recipe_ids.clone()))
            .all(&self.db)
            .await?;

        // 5. Load primary images
        use crate::entity::recipe_image;
        let images: HashMap<i64, String> = recipe_image::Entity::find()
            .filter(recipe_image::Column::RecipeId.is_in(recipe_ids.clone()))
            .filter(recipe_image::Column::IsPrimary.eq(true))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|img| (img.recipe_id, img.url))
            .collect();

        let recipe_map: HashMap<i64, recipe::Model> =
            recipes.into_iter().map(|r| (r.id, r)).collect();

        let suggestions = scored
            .iter()
            .filter_map(|(recipe_id, have, total)| {
                let r = recipe_map.get(recipe_id)?;
                Some(RecipeSuggestion {
                    recipe_id: r.id,
                    name: r.name.clone(),
                    slug: r.slug.clone(),
                    primary_image_url: images.get(&r.id).cloned(),
                    total_time_min: r.total_time_min,
                    difficulty: r.difficulty.clone(),
                    ingredients_have: *have as i32,
                    ingredients_total: *total as i32,
                    match_pct: ((*have as f64 / *total as f64) * 100.0).round() as i32,
                })
            })
            .collect();

        Ok(suggestions)
    }
}
