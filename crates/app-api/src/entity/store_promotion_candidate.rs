//! Staging table for AI-extracted promotions awaiting admin review
//! Promotions are moved to store_promotions after approval

use rust_decimal::Decimal;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "store_promotion_candidates")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,

    pub store_id: Uuid,
    pub job_id: Uuid,

    pub product_name: String,
    pub brand: Option<String>,

    #[sea_orm(column_type = "Decimal(Some((10, 2)))", nullable)]
    pub original_price: Option<Decimal>,

    #[sea_orm(column_type = "Decimal(Some((10, 2)))")]
    pub discounted_price: Decimal,

    #[sea_orm(column_type = "Decimal(Some((5, 2)))", nullable)]
    pub discount_pct: Option<Decimal>,

    pub unit: Option<String>,

    pub valid_from: Option<DateTimeWithTimeZone>,
    pub valid_until: Option<DateTimeWithTimeZone>,

    /// AI confidence score 0.0–1.0
    #[sea_orm(column_type = "Decimal(Some((4, 3)))", nullable)]
    pub confidence: Option<Decimal>,

    /// "pending" | "approved" | "rejected"
    pub review_status: String,

    pub reviewed_by: Option<Uuid>,
    pub reviewed_at: Option<DateTimeWithTimeZone>,

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

    #[sea_orm(
        belongs_to = "super::pdf_processing_job::Entity",
        from = "Column::JobId",
        to = "super::pdf_processing_job::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    PdfJob,
}

impl Related<super::store::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Store.def()
    }
}

impl Related<super::pdf_processing_job::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PdfJob.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
