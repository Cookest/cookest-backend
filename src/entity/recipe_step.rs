//! Recipe step entity
//! Ordered cooking instructions, each with optional step image

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "recipe_steps")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,

    pub recipe_id: i64,

    /// 1-based step number
    pub step_number: i32,

    /// The instruction text for this step
    #[sea_orm(column_type = "Text")]
    pub instruction: String,

    /// Optional duration hint for this step in minutes
    pub duration_min: Option<i32>,

    /// Optional image specific to this step (stored path or URL)
    #[sea_orm(column_type = "Text", nullable)]
    pub image_url: Option<String>,

    /// Optional tip or warning for this step
    #[sea_orm(column_type = "Text", nullable)]
    pub tip: Option<String>,
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
