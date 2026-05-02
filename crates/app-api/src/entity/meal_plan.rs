//! Meal plan entity
//! One plan per user per week — contains all meal slots

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "meal_plans")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,

    pub user_id: Uuid,

    /// Start of the week this plan covers (always a Monday)
    pub week_start: Date,

    /// Whether the AI generated this plan automatically
    pub is_ai_generated: bool,

    pub created_at: DateTimeWithTimeZone,
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

    #[sea_orm(has_many = "super::meal_plan_slot::Entity")]
    MealPlanSlots,
}

impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::User.def()
    }
}

impl Related<super::meal_plan_slot::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MealPlanSlots.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
