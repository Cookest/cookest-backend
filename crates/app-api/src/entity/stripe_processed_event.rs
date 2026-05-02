//! Stripe processed event entity — idempotency guard for webhook delivery
//! Stripe may deliver the same event multiple times; this table prevents double-processing

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "stripe_processed_events")]
pub struct Model {
    /// Stripe event ID (e.g. "evt_1AbcDef...") — used as primary key
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub event_id: String,

    pub processed_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
