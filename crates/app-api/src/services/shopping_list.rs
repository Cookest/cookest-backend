//! Shopping list service — persistent per-user shopping list with meal plan sync

use chrono::Utc;
use rust_decimal::Decimal;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::shopping_list_item::{self, ActiveModel, Entity as ShoppingListItem};
use cookest_shared::errors::AppError;

#[derive(Debug, Serialize)]
pub struct ShoppingListItemResponse {
    pub id: Uuid,
    pub ingredient_id: Option<i64>,
    pub name: String,
    pub quantity: Option<Decimal>,
    pub unit: Option<String>,
    pub is_checked: bool,
    pub is_manual: bool,
    pub meal_plan_id: Option<i64>,
    pub created_at: sea_orm::prelude::DateTimeWithTimeZone,
}

impl From<shopping_list_item::Model> for ShoppingListItemResponse {
    fn from(m: shopping_list_item::Model) -> Self {
        Self {
            id: m.id,
            ingredient_id: m.ingredient_id,
            name: m.name,
            quantity: m.quantity,
            unit: m.unit,
            is_checked: m.is_checked,
            is_manual: m.is_manual,
            meal_plan_id: m.meal_plan_id,
            created_at: m.created_at,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AddItemRequest {
    pub ingredient_id: Option<i64>,
    pub name: String,
    pub quantity: Option<Decimal>,
    pub unit: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SyncItem {
    pub ingredient_id: Option<i64>,
    pub name: String,
    pub quantity: Option<Decimal>,
    pub unit: Option<String>,
    pub meal_plan_id: Option<i64>,
}

pub struct ShoppingListService {
    db: DatabaseConnection,
}

impl ShoppingListService {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// Get all shopping list items for a user
    pub async fn get_list(&self, user_id: Uuid) -> Result<Vec<ShoppingListItemResponse>, AppError> {
        let items = ShoppingListItem::find()
            .filter(shopping_list_item::Column::UserId.eq(user_id))
            .all(&self.db)
            .await?;
        Ok(items.into_iter().map(ShoppingListItemResponse::from).collect())
    }

    /// Add a manual item to the shopping list
    pub async fn add_item(
        &self,
        user_id: Uuid,
        req: AddItemRequest,
    ) -> Result<ShoppingListItemResponse, AppError> {
        let now = Utc::now().fixed_offset();
        let item = ActiveModel {
            id: Set(Uuid::new_v4()),
            user_id: Set(user_id),
            ingredient_id: Set(req.ingredient_id),
            name: Set(req.name),
            quantity: Set(req.quantity),
            unit: Set(req.unit),
            is_checked: Set(false),
            is_manual: Set(true),
            meal_plan_id: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        };
        let inserted = item.insert(&self.db).await?;
        Ok(ShoppingListItemResponse::from(inserted))
    }

    /// Toggle checked status
    pub async fn toggle_check(
        &self,
        user_id: Uuid,
        item_id: Uuid,
    ) -> Result<ShoppingListItemResponse, AppError> {
        let item = ShoppingListItem::find_by_id(item_id)
            .filter(shopping_list_item::Column::UserId.eq(user_id))
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound("Shopping list item".to_string()))?;

        let new_checked = !item.is_checked;
        let mut active: ActiveModel = item.into();
        active.is_checked = Set(new_checked);
        active.updated_at = Set(Utc::now().fixed_offset());
        let updated = active.update(&self.db).await?;
        Ok(ShoppingListItemResponse::from(updated))
    }

    /// Remove an item
    pub async fn delete_item(&self, user_id: Uuid, item_id: Uuid) -> Result<(), AppError> {
        let item = ShoppingListItem::find_by_id(item_id)
            .filter(shopping_list_item::Column::UserId.eq(user_id))
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound("Shopping list item".to_string()))?;

        let active: ActiveModel = item.into();
        active.delete(&self.db).await?;
        Ok(())
    }

    /// Sync shopping list from a set of items — replaces all non-manual items in a transaction
    /// Uses ingredient_id as the stable key to prevent duplicates
    pub async fn sync_from_meal_plan(
        &self,
        user_id: Uuid,
        items: Vec<SyncItem>,
    ) -> Result<Vec<ShoppingListItemResponse>, AppError> {
        let txn = self.db.begin().await?;

        // Delete all previously-synced (non-manual) items
        ShoppingListItem::delete_many()
            .filter(shopping_list_item::Column::UserId.eq(user_id))
            .filter(shopping_list_item::Column::IsManual.eq(false))
            .exec(&txn)
            .await?;

        let now = Utc::now().fixed_offset();
        let mut result = vec![];

        for item in items {
            let model = ActiveModel {
                id: Set(Uuid::new_v4()),
                user_id: Set(user_id),
                ingredient_id: Set(item.ingredient_id),
                name: Set(item.name),
                quantity: Set(item.quantity),
                unit: Set(item.unit),
                is_checked: Set(false),
                is_manual: Set(false),
                meal_plan_id: Set(item.meal_plan_id),
                created_at: Set(now),
                updated_at: Set(now),
            };
            let inserted = model.insert(&txn).await?;
            result.push(ShoppingListItemResponse::from(inserted));
        }

        txn.commit().await?;
        Ok(result)
    }

    /// Clear all checked items
    pub async fn clear_checked(&self, user_id: Uuid) -> Result<u64, AppError> {
        let res = ShoppingListItem::delete_many()
            .filter(shopping_list_item::Column::UserId.eq(user_id))
            .filter(shopping_list_item::Column::IsChecked.eq(true))
            .exec(&self.db)
            .await?;
        Ok(res.rows_affected)
    }
}
