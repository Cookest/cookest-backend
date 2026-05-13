//! Ingredient entity - master ingredient list
//! Populated by ETL pipeline from USDA FoodData Central

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "ingredients")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,

    #[sea_orm(unique, column_type = "Text")]
    pub name: String,

    /// Category: "protein", "dairy", "vegetable", "grain", "fruit", "fat", "spice", "other"
    #[sea_orm(column_type = "Text", nullable)]
    pub category: Option<String>,

    /// USDA FoodData Central ID — for future ETL linking
    pub fdc_id: Option<i32>,

    /// Open Food Facts barcode
    #[sea_orm(column_type = "Text", nullable)]
    pub off_id: Option<String>,

    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_one = "super::ingredient_nutrient::Entity")]
    IngredientNutrient,

    #[sea_orm(has_many = "super::portion_size::Entity")]
    PortionSizes,

    #[sea_orm(has_many = "super::recipe_ingredient::Entity")]
    RecipeIngredients,

    #[sea_orm(has_many = "super::inventory_item::Entity")]
    InventoryItems,
}

impl Related<super::ingredient_nutrient::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::IngredientNutrient.def()
    }
}

impl Related<super::portion_size::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PortionSizes.def()
    }
}

impl Related<super::recipe_ingredient::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::RecipeIngredients.def()
    }
}

impl Related<super::inventory_item::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::InventoryItems.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
