//! Portion size entity
//! Maps common measurement descriptions to grams
//! e.g. "1 cup" = 240g, "1 tablespoon" = 15g

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "portion_sizes")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,

    pub ingredient_id: i64,

    /// Human-readable description e.g. "1 cup", "1 medium apple", "1 tablespoon"
    #[sea_orm(column_type = "Text")]
    pub description: String,

    /// Weight in grams for this portion
    pub weight_grams: Decimal,

    /// Unit used: "cup", "tbsp", "tsp", "piece", "slice", "oz", etc.
    #[sea_orm(column_type = "Text", nullable)]
    pub unit: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::ingredient::Entity",
        from = "Column::IngredientId",
        to = "super::ingredient::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    Ingredient,
}

impl Related<super::ingredient::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Ingredient.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
