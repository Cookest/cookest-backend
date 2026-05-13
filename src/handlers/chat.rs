//! Chat handlers — AI assistant endpoints

use actix_web::{web, HttpResponse};
use std::sync::Arc;
use uuid::Uuid;

use crate::errors::AppError;
use crate::middleware::Claims;
use crate::services::chat::{ChatRequest, ChatService};

/// POST /api/chat
/// Send a message to the AI assistant (creates or continues a session)
pub async fn send_message(
    chat_svc: web::Data<Arc<ChatService>>,
    claims: web::ReqData<Claims>,
    body: web::Json<ChatRequest>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let reply = chat_svc.chat(user_id, body.into_inner()).await?;
    Ok(HttpResponse::Ok().json(reply))
}

/// GET /api/chat/sessions
/// List all chat sessions for the authenticated user
pub async fn list_sessions(
    chat_svc: web::Data<Arc<ChatService>>,
    claims: web::ReqData<Claims>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let sessions = chat_svc.list_sessions(user_id).await?;
    Ok(HttpResponse::Ok().json(sessions))
}

/// GET /api/chat/sessions/{id}/messages
/// Get all messages in a session (excludes system messages)
pub async fn get_messages(
    chat_svc: web::Data<Arc<ChatService>>,
    claims: web::ReqData<Claims>,
    path: web::Path<i64>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    let messages = chat_svc.get_messages(user_id, path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(messages))
}

/// DELETE /api/chat/sessions/{id}
/// Delete a session and all its messages
pub async fn delete_session(
    chat_svc: web::Data<Arc<ChatService>>,
    claims: web::ReqData<Claims>,
    path: web::Path<i64>,
) -> Result<HttpResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::InvalidToken)?;
    chat_svc.delete_session(user_id, path.into_inner()).await?;
    Ok(HttpResponse::NoContent().finish())
}

/// Route configuration
pub fn configure_chat(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/chat")
            .route("", web::post().to(send_message))
            .route("/sessions", web::get().to(list_sessions))
            .route("/sessions/{id}/messages", web::get().to(get_messages))
            .route("/sessions/{id}", web::delete().to(delete_session)),
    );
}
