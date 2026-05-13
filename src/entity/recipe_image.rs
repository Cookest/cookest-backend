//! Recipe image entity
//! Multiple images per recipe (hero, thumbnail, step photos)

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "recipe_images")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,

    pub recipe_id: i64,

    /// Path in S3 / local storage, or external URL
    #[sea_orm(column_type = "Text")]
    pub url: String,

    /// "hero" | "thumbnail" | "step" | "ingredient"
    #[sea_orm(column_type = "Text", nullable)]
    pub image_type: Option<String>,

    /// Whether this is the main display image
    pub is_primary: bool,

    /// Image dimensions for lazy loading placeholders
    pub width: Option<i32>,
    pub height: Option<i32>,

    /// Source attribution: "themealdb" | "custom" | "mm_food" | "usda"
    #[sea_orm(column_type = "Text", nullable)]
    pub source: Option<String>,

    pub created_at: DateTimeWithTimeZone,
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
