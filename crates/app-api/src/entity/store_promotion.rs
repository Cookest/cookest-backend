//! Store promotion entity — a published, admin-approved price/discount from a store flyer

use rust_decimal::Decimal;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "store_promotions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,

    pub store_id: Uuid,

    /// Raw product name from the flyer (e.g. "Chicken Breast 500g")
    pub product_name: String,

    pub brand: Option<String>,

    #[sea_orm(column_type = "Decimal(Some((10, 2)))", nullable)]
    pub original_price: Option<Decimal>,

    #[sea_orm(column_type = "Decimal(Some((10, 2)))")]
    pub discounted_price: Decimal,

    /// Discount percentage computed at insert time
    #[sea_orm(column_type = "Decimal(Some((5, 2)))", nullable)]
    pub discount_pct: Option<Decimal>,

    /// Unit string e.g. "kg", "500g", "piece"
    pub unit: Option<String>,

    pub valid_from: Option<DateTimeWithTimeZone>,
    pub valid_until: Option<DateTimeWithTimeZone>,

    pub is_active: bool,

    /// URL of the source PDF this promotion was extracted from
    pub source_pdf_url: Option<String>,

    /// AI confidence score 0.0–1.0
    #[sea_orm(column_type = "Decimal(Some((4, 3)))", nullable)]
    pub confidence: Option<Decimal>,

    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
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
}

impl Related<super::store::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Store.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
