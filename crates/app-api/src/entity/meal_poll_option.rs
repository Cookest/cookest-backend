//! Meal poll option entity — a candidate dish people can vote for.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "meal_poll_options")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub poll_id: Uuid,
    pub recipe_id: Option<i64>,
    pub label: String,
    pub image_url: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
