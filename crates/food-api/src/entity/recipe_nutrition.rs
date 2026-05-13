//! Recipe nutrition entity
//! Precomputed macros per serving — avoids expensive joins at query time
//! Recalculated whenever recipe_ingredients changes

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "recipe_nutrition")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,

    #[sea_orm(unique)]
    pub recipe_id: i64,

    /// Whether values below are per serving or per 100g
    pub per_serving: bool,

    pub calories: Option<Decimal>,
    pub protein_g: Option<Decimal>,
    pub carbs_g: Option<Decimal>,
    pub fat_g: Option<Decimal>,
    pub fiber_g: Option<Decimal>,
    pub sugar_g: Option<Decimal>,
    pub sodium_mg: Option<Decimal>,
    pub saturated_fat_g: Option<Decimal>,
    pub cholesterol_mg: Option<Decimal>,

    /// Additional micros stored as JSON (vitamins, minerals)
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub micronutrients: Option<Json>,

    pub calculated_at: DateTimeWithTimeZone,
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
}

impl Related<super::recipe::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Recipe.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
