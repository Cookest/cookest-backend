use actix_web::{web, HttpResponse};
use std::sync::Arc;
use uuid::Uuid;

use cookest_shared::errors::AppError;
use crate::middleware::Claims;
use crate::services::notification::NotificationService;

pub async fn get_notifications(
    notification_service: web::Data<Arc<NotificationService>>,
    claims: web::ReqData<Claims>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    
    let notifications = notification_service.get_user_notifications(user_id)
        .await
        .map_err(|e| AppError::Internal(e))?;
        
    Ok(HttpResponse::Ok().json(notifications))
}

pub async fn mark_read(
    notification_service: web::Data<Arc<NotificationService>>,
    claims: web::ReqData<Claims>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let notification_id = path.into_inner();
    
    notification_service.mark_as_read(notification_id, user_id)
        .await
        .map_err(|e| AppError::Internal(e))?;
        
    Ok(HttpResponse::Ok().json(serde_json::json!({ "success": true })))
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/users/me/notifications")
            .route("", web::get().to(get_notifications))
            .route("/{id}/read", web::put().to(mark_read)),
    );
}
