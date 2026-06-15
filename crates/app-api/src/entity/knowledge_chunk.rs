//! Nutrition knowledge chunk (RAG)
//! Text chunks from open nutrition books, embedded with `nomic-embed-text` and
//! stored in a pgvector column. The `embedding` column is intentionally NOT
//! mapped here — it is written by the Python ingestion pipeline and queried via
//! raw SQL (`embedding <=> $1`) in the embeddings service.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "knowledge_chunks")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,

    /// Source book / file name
    #[sea_orm(column_type = "Text")]
    pub source: String,

    #[sea_orm(column_type = "Text", nullable)]
    pub title: Option<String>,

    pub chunk_index: i32,

    #[sea_orm(column_type = "Text")]
    pub content: String,

    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
