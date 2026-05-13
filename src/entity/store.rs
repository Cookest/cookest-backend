//! Store entity — represents a supermarket or grocery chain

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "stores")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,

    pub name: String,

    /// URL-safe identifier e.g. "lidl", "kaufland"
    #[sea_orm(unique)]
    pub slug: String,

    pub website: Option<String>,

    pub logo_url: Option<String>,

    pub country: Option<String>,

    pub city: Option<String>,

    /// Latitude for geolocation features
    pub lat: Option<f64>,

    /// Longitude for geolocation features
    pub lng: Option<f64>,

    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::store_promotion::Entity")]
    Promotions,

    #[sea_orm(has_many = "super::pdf_processing_job::Entity")]
    PdfJobs,
}

impl Related<super::store_promotion::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Promotions.def()
    }
}

impl Related<super::pdf_processing_job::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PdfJobs.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
