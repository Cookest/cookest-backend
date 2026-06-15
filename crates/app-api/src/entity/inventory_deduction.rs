//! Inventory deduction audit entity
//! One row per inventory item touched when a recipe is cooked or an item is
//! manually consumed. Enables undo and a per-user deduction history.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "inventory_deductions")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,

    pub user_id: Uuid,

    /// The inventory row that was decremented (may since have been deleted)
    pub inventory_item_id: Option<i64>,

    pub ingredient_id: i64,

    /// Recipe this deduction came from (null for manual consume)
    pub recipe_id: Option<i64>,

    /// Cooking-history entry this deduction belongs to (null for manual consume)
    pub cooking_history_id: Option<i64>,

    pub qty_before: Decimal,
    pub qty_deducted: Decimal,

    #[sea_orm(column_type = "Text")]
    pub unit: String,

    /// Whether the inventory row was removed because it hit zero
    pub was_deleted: bool,

    /// "cook", "manual", or "expired"
    #[sea_orm(column_type = "Text")]
    pub reason: String,

    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
