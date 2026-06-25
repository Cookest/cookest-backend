use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, Set,
};
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

use crate::entity::notification::{
    self, ActiveModel as NotificationActiveModel, Entity as NotificationEntity,
};
use crate::services::email::EmailService;

#[derive(Clone)]
pub struct NotificationService {
    db: Arc<DatabaseConnection>,
    email_service: Arc<EmailService>,
}

impl NotificationService {
    pub fn new(db: Arc<DatabaseConnection>, email_service: Arc<EmailService>) -> Self {
        Self { db, email_service }
    }

    pub async fn create_notification(
        &self,
        user_id: Uuid,
        title: &str,
        message: &str,
        notification_type: &str,
        metadata: serde_json::Value,
    ) -> Result<notification::Model, String> {
        let active_model = NotificationActiveModel {
            id: Set(Uuid::new_v4()),
            user_id: Set(user_id),
            title: Set(title.to_string()),
            message: Set(message.to_string()),
            r#type: Set(notification_type.to_string()),
            metadata: Set(metadata),
            is_read: Set(false),
            created_at: Set(Utc::now().into()),
        };

        let notification = active_model
            .insert(self.db.as_ref())
            .await
            .map_err(|e| e.to_string())?;

        // Note: For actual push notifications (APNS/FCM), we would look up user_push_tokens for user_id
        // and dispatch an HTTP request to FCM/APNS here.
        info!(
            "Sending push notification to user {}: {} - {}",
            user_id, title, message
        );

        Ok(notification)
    }

    pub async fn get_user_notifications(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<notification::Model>, String> {
        NotificationEntity::find()
            .filter(notification::Column::UserId.eq(user_id))
            .order_by_desc(notification::Column::CreatedAt)
            .all(self.db.as_ref())
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn mark_as_read(&self, notification_id: Uuid, user_id: Uuid) -> Result<(), String> {
        let notification = NotificationEntity::find_by_id(notification_id)
            .filter(notification::Column::UserId.eq(user_id))
            .one(self.db.as_ref())
            .await
            .map_err(|e| e.to_string())?;

        if let Some(notification) = notification {
            let mut active_model: NotificationActiveModel = notification.into();
            active_model.is_read = Set(true);
            active_model
                .update(self.db.as_ref())
                .await
                .map_err(|e| e.to_string())?;
        }

        Ok(())
    }
}
