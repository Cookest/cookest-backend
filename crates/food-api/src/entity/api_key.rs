//! API key entity for authenticating food-api consumers

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "api_keys")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: Uuid,

    /// Display name for the key owner
    #[sea_orm(column_type = "Text")]
    pub name: String,

    /// SHA-256 hash of the raw API key
    #[sea_orm(column_type = "Text")]
    pub key_hash: String,

    /// Subscription tier: "free" | "starter" | "pro" | "enterprise"
    #[sea_orm(column_type = "Text")]
    pub tier: String,

    /// Requests per minute limit
    pub rate_limit_rpm: i32,

    /// Monthly request count (reset on billing cycle)
    pub monthly_usage: i64,

    /// Monthly request limit
    pub monthly_limit: i64,

    pub is_active: bool,

    pub created_at: DateTimeWithTimeZone,
    pub last_used_at: Option<DateTimeWithTimeZone>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
