//! User preferences entity
//! Stores the ML preference vector that learns from user behaviour over time

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "user_preferences")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub user_id: Uuid,

    /// Learned cuisine weights — e.g. {"Italian": 0.87, "Japanese": 0.62}
    /// Range: -1.0 (disliked) to 1.0 (loved), starts at 0.0
    #[sea_orm(column_type = "JsonBinary")]
    pub cuisine_weights: Json,

    /// Learned ingredient weights — e.g. {"chicken": 0.9, "mushrooms": -0.6}
    #[sea_orm(column_type = "JsonBinary")]
    pub ingredient_weights: Json,

    /// Macro preferences — e.g. {"protein": 0.8, "carbs": 0.3}
    #[sea_orm(column_type = "JsonBinary")]
    pub macro_bias: Json,

    /// Difficulty preference — e.g. {"easy": 0.7, "medium": 0.5, "hard": 0.1}
    #[sea_orm(column_type = "JsonBinary")]
    pub difficulty_weights: Json,

    /// Preferred cooking time in minutes (weighted average of cooked recipes)
    pub preferred_time_min: i32,

    /// Total number of interactions (for confidence weighting)
    pub interaction_count: i32,

    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::UserId",
        to = "super::user::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    User,
}

impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::User.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
