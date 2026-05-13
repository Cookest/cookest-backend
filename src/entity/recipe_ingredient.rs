//! Recipe ingredient join table
//! Links a recipe to its ingredients with quantities

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "recipe_ingredients")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,

    pub recipe_id: i64,
    pub ingredient_id: i64,

    /// Amount as written in the recipe (e.g. 2.5)
    pub quantity: Option<Decimal>,

    /// Unit as written (e.g. "cup", "g", "tbsp", "piece")
    #[sea_orm(column_type = "Text", nullable)]
    pub unit: Option<String>,

    /// Quantity normalized to grams — used for nutrient calculation
    /// quantity_grams / 100 * nutrient_per_100g = nutrient for this ingredient
    pub quantity_grams: Option<Decimal>,

    /// Extra notes: "finely chopped", "room temperature", "divided"
    #[sea_orm(column_type = "Text", nullable)]
    pub notes: Option<String>,

    /// Order in the ingredient list
    pub display_order: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::recipe::Entity",
        from = "Column::RecipeId",
        to = "super::recipe::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    Recipe,

    #[sea_orm(
        belongs_to = "super::ingredient::Entity",
        from = "Column::IngredientId",
        to = "super::ingredient::Column::Id",
        on_update = "Cascade",
        on_delete = "Restrict"
    )]
    Ingredient,
}

impl Related<super::recipe::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Recipe.def()
    }
}

impl Related<super::ingredient::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Ingredient.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
