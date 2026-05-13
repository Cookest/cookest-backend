//! ETL scrape log — tracks which URLs have been scraped and their status

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "etl_scrape_log")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,

    /// The original URL that was scraped
    #[sea_orm(unique, column_type = "Text")]
    pub source_url: String,

    /// Domain e.g. "bbcgoodfood.com"
    #[sea_orm(column_type = "Text")]
    pub source_site: String,

    pub scraped_at: DateTimeWithTimeZone,

    /// "success" | "error" | "skipped"
    #[sea_orm(column_type = "Text")]
    pub status: String,

    /// FK to the recipe that was created (if successful)
    pub recipe_id: Option<i64>,

    /// Error message if status == "error"
    #[sea_orm(column_type = "Text", nullable)]
    pub error_msg: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::recipe::Entity",
        from = "Column::RecipeId",
        to = "super::recipe::Column::Id"
    )]
    Recipe,
}

impl Related<super::recipe::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Recipe.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
