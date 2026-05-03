//! Ingredient nutrient entity - macros and micros per 100g
//! One-to-one with ingredients

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "ingredient_nutrients")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,

    pub ingredient_id: i64,

    /// Kilocalories per 100g
    pub calories: Option<Decimal>,

    /// Protein in grams per 100g
    pub protein_g: Option<Decimal>,

    /// Total carbohydrates in grams per 100g
    pub carbs_g: Option<Decimal>,

    /// Total fat in grams per 100g
    pub fat_g: Option<Decimal>,

    /// Dietary fiber in grams per 100g
    pub fiber_g: Option<Decimal>,

    /// Total sugars in grams per 100g
    pub sugar_g: Option<Decimal>,

    /// Sodium in milligrams per 100g
    pub sodium_mg: Option<Decimal>,

    /// Saturated fat in grams per 100g
    pub saturated_fat_g: Option<Decimal>,

    /// Cholesterol in milligrams per 100g
    pub cholesterol_mg: Option<Decimal>,

    /// Extended micronutrients stored as JSON
    /// e.g. {"vitamin_c_mg": 12.5, "iron_mg": 2.1, "calcium_mg": 45.0}
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub micronutrients: Option<Json>,
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
