//! PDF processing job entity
//! Tracks the lifecycle of a supermarket flyer being processed by the AI

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "pdf_processing_jobs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,

    pub store_id: Uuid,

    /// Path on disk where the uploaded PDF is stored
    #[sea_orm(column_type = "Text")]
    pub file_path: String,

    /// "pending" | "processing" | "done" | "failed"
    pub status: String,

    /// Error message if status = "failed"
    pub error: Option<String>,

    /// Number of processing attempts (for retry logic)
    pub retry_count: i32,

    pub started_at: Option<DateTimeWithTimeZone>,
    pub heartbeat_at: Option<DateTimeWithTimeZone>,
    pub processed_at: Option<DateTimeWithTimeZone>,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::store::Entity",
        from = "Column::StoreId",
        to = "super::store::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    Store,

    #[sea_orm(has_many = "super::store_promotion_candidate::Entity")]
    Candidates,
}

impl Related<super::store::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Store.def()
    }
}

impl Related<super::store_promotion_candidate::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Candidates.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
