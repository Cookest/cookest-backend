//! Inventory item entity
//! Tracks a user's stock of ingredients with quantity and expiry date

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "inventory_items")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,

    pub user_id: Uuid,

    pub ingredient_id: i64,

    /// Custom name override if the user entered it manually
    #[sea_orm(column_type = "Text", nullable)]
    pub custom_name: Option<String>,

    pub quantity: Decimal,

    /// "g", "kg", "ml", "l", "cup", "tbsp", "tsp", "piece"
    #[sea_orm(column_type = "Text")]
    pub unit: String,

    /// Date the item expires — used for expiry alerts and meal plan prioritization
    pub expiry_date: Option<Date>,

    /// User-defined location: "fridge", "freezer", "pantry"
    #[sea_orm(column_type = "Text", nullable)]
    pub storage_location: Option<String>,

    pub added_at: DateTimeWithTimeZone,
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
        on_delete = "Restrict"
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
