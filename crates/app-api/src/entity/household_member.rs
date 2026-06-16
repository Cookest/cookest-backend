//! Household member entity — links a user to a household with a role.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "household_members")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub household_id: Uuid,
    pub user_id: Uuid,
    /// "owner" | "member"
    pub role: String,
    pub joined_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
