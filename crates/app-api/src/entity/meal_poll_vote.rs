//! Meal poll vote entity — one vote per voter_key (cookie/device or user id).

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "meal_poll_votes")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub poll_id: Uuid,
    pub option_id: i64,
    pub voter_key: String,
    pub voter_name: Option<String>,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
