//! Shopping list handlers — persisted list + meal plan sync (all routes require auth)

use actix_web::{web, HttpResponse};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::errors::AppError;
use crate::middleware::auth::AuthenticatedUser;
use crate::services::shopping_list::{AddItemRequest, ShoppingListService, SyncItem};

pub fn configure_shopping_list(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/shopping-list")
            .route("", web::get().to(get_list))
            .route("/items", web::post().to(add_item))
            .route("/items/{id}/check", web::patch().to(toggle_check))
            .route("/items/{id}", web::delete().to(delete_item))
            .route("/sync", web::post().to(sync_from_plan))
            .route("/clear-checked", web::delete().to(clear_checked)),
    );
}

async fn get_list(
    user: AuthenticatedUser,
    service: web::Data<Arc<ShoppingListService>>,
) -> Result<HttpResponse, AppError> {
    let items = service.get_list(user.id).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "items": items })))
}

async fn add_item(
    user: AuthenticatedUser,
    service: web::Data<Arc<ShoppingListService>>,
    body: web::Json<AddItemRequest>,
) -> Result<HttpResponse, AppError> {
    let item = service.add_item(user.id, body.into_inner()).await?;
    Ok(HttpResponse::Created().json(item))
}

async fn toggle_check(
    user: AuthenticatedUser,
    service: web::Data<Arc<ShoppingListService>>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let item = service.toggle_check(user.id, path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(item))
}

async fn delete_item(
    user: AuthenticatedUser,
    service: web::Data<Arc<ShoppingListService>>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    service.delete_item(user.id, path.into_inner()).await?;
    Ok(HttpResponse::NoContent().finish())
}

#[derive(Deserialize)]
struct SyncRequest {
    items: Vec<SyncItem>,
}

async fn sync_from_plan(
    user: AuthenticatedUser,
    service: web::Data<Arc<ShoppingListService>>,
    body: web::Json<SyncRequest>,
) -> Result<HttpResponse, AppError> {
    let items = service.sync_from_meal_plan(user.id, body.into_inner().items).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "items": items })))
}

async fn clear_checked(
    user: AuthenticatedUser,
    service: web::Data<Arc<ShoppingListService>>,
) -> Result<HttpResponse, AppError> {
    let count = service.clear_checked(user.id).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "deleted": count })))
}
