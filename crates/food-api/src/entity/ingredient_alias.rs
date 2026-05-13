//! Ingredient alias entity — multilingual ingredient name lookup

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "ingredient_aliases")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,

    pub ingredient_id: i64,

    /// Alias name, e.g. "Mehl" (German for "flour")
    #[sea_orm(unique, column_type = "Text")]
    pub alias: String,

    /// ISO 639-1 language code: "en", "fr", "de", "it", "es", "pt"
    #[sea_orm(column_type = "Text")]
    pub language: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::ingredient::Entity",
        from = "Column::IngredientId",
        to = "super::ingredient::Column::Id"
    )]
    Ingredient,
}

impl Related<super::ingredient::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Ingredient.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
