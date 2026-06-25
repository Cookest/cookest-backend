//! Meal plan suggestion entity
//! Recipes suggested by family members for specific slots

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "meal_plan_suggestions")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,

    pub plan_id: i64,
    pub slot_id: i64,
    pub recipe_id: i64,
    pub suggested_by: Uuid,

    /// 'pending', 'approved', 'rejected'
    pub status: String,

    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::meal_plan::Entity",
        from = "Column::PlanId",
        to = "super::meal_plan::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    MealPlan,

    #[sea_orm(
        belongs_to = "super::meal_plan_slot::Entity",
        from = "Column::SlotId",
        to = "super::meal_plan_slot::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    MealPlanSlot,

    #[sea_orm(
        belongs_to = "super::recipe::Entity",
        from = "Column::RecipeId",
        to = "super::recipe::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    Recipe,

    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::SuggestedBy",
        to = "super::user::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    User,
}

impl Related<super::meal_plan::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MealPlan.def()
    }
}

impl Related<super::meal_plan_slot::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MealPlanSlot.def()
    }
}

impl Related<super::recipe::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Recipe.def()
    }
}

impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::User.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
