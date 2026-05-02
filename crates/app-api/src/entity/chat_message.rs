//! Chat message entity
//! Individual messages within a chat session

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "chat_messages")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,

    pub session_id: i64,

    /// "user" | "assistant" | "system"
    #[sea_orm(column_type = "Text")]
    pub role: String,

    /// The message content
    #[sea_orm(column_type = "Text")]
    pub content: String,

    /// Optional: tokens used for this message (for Ollama monitoring)
    pub tokens_used: Option<i32>,

    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::chat_session::Entity",
        from = "Column::SessionId",
        to = "super::chat_session::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    ChatSession,
}

impl Related<super::chat_session::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ChatSession.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
