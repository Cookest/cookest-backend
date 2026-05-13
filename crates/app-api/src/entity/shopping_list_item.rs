//! Shopping list item entity
//! Persisted per-user shopping list with optional meal plan linkage

use rust_decimal::Decimal;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "shopping_list_items")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,

    pub user_id: Uuid,

    /// Linked ingredient (NULL for manually-added free-text items)
    pub ingredient_id: Option<i64>,

    /// Display name — may differ from canonical ingredient name
    pub name: String,

    pub quantity: Option<Decimal>,

    pub unit: Option<String>,

    /// Whether the user has checked this off while shopping
    pub is_checked: bool,

    /// True = added manually by user; False = generated from meal plan
    pub is_manual: bool,

    /// Which meal plan generated this item (NULL for manual items)
    pub meal_plan_id: Option<i64>,

    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::UserId",
        to = "super::user::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    User,

    #[sea_orm(
        belongs_to = "super::ingredient::Entity",
        from = "Column::IngredientId",
        to = "super::ingredient::Column::Id",
        on_update = "Cascade",
        on_delete = "SetNull"
    )]
    Ingredient,
}

impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::User.def()
    }
}

impl Related<super::ingredient::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Ingredient.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
