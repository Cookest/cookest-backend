//! Ingredient allergen tracking
//! Maps ingredients to their allergen information

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "ingredient_allergens")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,

    pub ingredient_id: i64,

    /// Allergen type: gluten, dairy, egg, peanut, tree_nut, shellfish, fish,
    /// soy, sesame, sulfite, lupine, celery, mustard, crustacean, mollusk, wheat, lactose
    #[sea_orm(column_type = "Text")]
    pub allergen: String,

    /// Severity: "contains" | "may_contain" (traces/cross-contamination)
    #[sea_orm(column_type = "Text")]
    pub severity: String,
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
