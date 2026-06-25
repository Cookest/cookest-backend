use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set, ColumnTrait, QueryFilter, QueryOrder};
use std::sync::Arc;
use uuid::Uuid;
use chrono::Utc;

use crate::entity::meal_plan_suggestion::{self, Entity as SuggestionEntity, ActiveModel as SuggestionActiveModel};
use crate::services::notification::NotificationService;

#[derive(Clone)]
pub struct MealPlanSuggestionService {
    db: Arc<DatabaseConnection>,
    notification_service: Arc<NotificationService>,
}

impl MealPlanSuggestionService {
    pub fn new(db: Arc<DatabaseConnection>, notification_service: Arc<NotificationService>) -> Self {
        Self { db, notification_service }
    }

    pub async fn create_suggestion(
        &self,
        plan_id: i64,
        slot_id: i64,
        recipe_id: i64,
        suggested_by: Uuid,
        family_owner_id: Uuid, // To notify the owner
    ) -> Result<meal_plan_suggestion::Model, String> {
        let active_model = SuggestionActiveModel {
            plan_id: Set(plan_id),
            slot_id: Set(slot_id),
            recipe_id: Set(recipe_id),
            suggested_by: Set(suggested_by),
            status: Set("pending".to_string()),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
            ..Default::default()
        };

        let suggestion = active_model.insert(self.db.as_ref()).await.map_err(|e| e.to_string())?;

        // Notify family owner
        let _ = self.notification_service.create_notification(
            family_owner_id,
            "New Recipe Suggestion",
            "A family member has suggested a new recipe for your meal plan.",
            "suggestion_created",
            serde_json::json!({
                "suggestion_id": suggestion.id,
                "plan_id": plan_id,
                "slot_id": slot_id,
            }),
        ).await;

        Ok(suggestion)
    }

    pub async fn get_suggestions_for_plan(&self, plan_id: i64) -> Result<Vec<meal_plan_suggestion::Model>, String> {
        SuggestionEntity::find()
            .filter(meal_plan_suggestion::Column::PlanId.eq(plan_id))
            .order_by_desc(meal_plan_suggestion::Column::CreatedAt)
            .all(self.db.as_ref())
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn update_suggestion_status(
        &self,
        suggestion_id: i64,
        status: &str, // 'approved' or 'rejected'
    ) -> Result<meal_plan_suggestion::Model, String> {
        let suggestion = SuggestionEntity::find_by_id(suggestion_id)
            .one(self.db.as_ref())
            .await
            .map_err(|e| e.to_string())?;

        let suggestion = suggestion.ok_or_else(|| "Suggestion not found".to_string())?;
        
        let mut active_model: SuggestionActiveModel = suggestion.clone().into();
        active_model.status = Set(status.to_string());
        active_model.updated_at = Set(Utc::now().into());
        let updated = active_model.update(self.db.as_ref()).await.map_err(|e| e.to_string())?;

        // Notify the suggester
        let _ = self.notification_service.create_notification(
            suggestion.suggested_by,
            if status == "approved" { "Suggestion Approved" } else { "Suggestion Rejected" },
            &format!("Your recipe suggestion was {}.", status),
            &format!("suggestion_{}", status),
            serde_json::json!({
                "suggestion_id": suggestion.id,
            }),
        ).await;

        Ok(updated)
    }
}
