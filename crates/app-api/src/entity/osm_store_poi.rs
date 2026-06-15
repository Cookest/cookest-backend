//! OpenStreetMap supermarket POI cache
//! Nearby supermarkets discovered via the Overpass API, cached with a TTL so
//! repeated "stores near me" queries don't hammer the public Overpass endpoint.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "osm_store_pois")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,

    /// OpenStreetMap element id
    pub osm_id: i64,

    /// "node", "way", or "relation"
    #[sea_orm(column_type = "Text")]
    pub osm_type: String,

    #[sea_orm(column_type = "Text", nullable)]
    pub name: Option<String>,

    #[sea_orm(column_type = "Text", nullable)]
    pub brand: Option<String>,

    #[sea_orm(column_type = "Double")]
    pub lat: f64,

    #[sea_orm(column_type = "Double")]
    pub lng: f64,

    /// Linked curated store, if this POI was matched to one
    pub matched_store_id: Option<Uuid>,

    /// Raw OSM tags for debugging / future enrichment
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub raw_tags: Option<Json>,

    pub fetched_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
