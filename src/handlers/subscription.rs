//! Subscription handlers — tier info, Stripe checkout, webhooks

use actix_web::{web, HttpRequest, HttpResponse};
use sea_orm::{DatabaseConnection, EntityTrait};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::entity::user::Entity as User;
use crate::errors::AppError;
use crate::middleware::auth::AuthenticatedUser;
use crate::services::subscription::SubscriptionService;
use crate::services::token::SubscriptionTier;

pub fn configure_subscription(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/subscription")
            .route("", web::get().to(get_subscription))
            .route("/checkout", web::post().to(create_checkout)),
    );
    cfg.route(
        "/api/webhooks/stripe",
        web::post().to(stripe_webhook),
    );
}

/// Return current subscription tier + features list
async fn get_subscription(
    user: AuthenticatedUser,
    db: web::Data<DatabaseConnection>,
    sub_service: web::Data<Arc<SubscriptionService>>,
) -> Result<HttpResponse, AppError> {
    // Read fresh from DB (claims may be stale if subscription changed)
    let user_model = User::find_by_id(user.id)
        .one(db.get_ref())
        .await?
        .ok_or_else(|| AppError::NotFound("User".to_string()))?;

    let tier = SubscriptionTier::from_str(&user_model.subscription_tier);
    let features = SubscriptionService::features_for_tier(&tier);

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "tier": user_model.subscription_tier,
        "valid_until": user_model.subscription_valid_until,
        "features": features,
    })))
}

#[derive(Deserialize)]
struct CheckoutRequest {
    tier: String,        // "pro" | "family"
    success_url: String,
    cancel_url: String,
}

/// Create a Stripe checkout session → returns URL for client to redirect to
async fn create_checkout(
    user: AuthenticatedUser,
    _sub_service: web::Data<Arc<SubscriptionService>>,
    body: web::Json<CheckoutRequest>,
) -> Result<HttpResponse, AppError> {
    // Validate tier selection
    let tier = match body.tier.as_str() {
        "pro" | "family" => body.tier.clone(),
        _ => return Err(AppError::Validation(validator::ValidationErrors::new())),
    };

    // In production: call Stripe API here to create a checkout session
    // For now we return a placeholder that makes the structure clear
    tracing::info!(
        "Checkout requested: user={}, tier={}, success_url={}",
        user.id, tier, body.success_url
    );

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "message": "Stripe integration pending — configure STRIPE_SECRET_KEY",
        "tier": tier,
        "success_url": body.success_url,
        "cancel_url": body.cancel_url,
    })))
}

/// Stripe webhook — verify signature, then handle subscription events
async fn stripe_webhook(
    req: HttpRequest,
    body: web::Bytes,
    sub_service: web::Data<Arc<SubscriptionService>>,
) -> Result<HttpResponse, AppError> {
    let sig_header = req
        .headers()
        .get("stripe-signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Verify HMAC signature against raw body
    sub_service.verify_stripe_signature(&body, sig_header)?;

    let event: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| AppError::Internal(format!("Invalid webhook JSON: {}", e)))?;

    let event_id = event["id"].as_str().unwrap_or("");
    let event_type = event["type"].as_str().unwrap_or("");

    // Idempotency — skip already-processed events
    if sub_service.is_event_processed(event_id).await? {
        tracing::debug!("Stripe event {} already processed, skipping", event_id);
        return Ok(HttpResponse::Ok().json(serde_json::json!({ "received": true })));
    }

    match event_type {
        "customer.subscription.created" | "customer.subscription.updated" => {
            let data = &event["data"]["object"];
            let customer_id = data["customer"].as_str().unwrap_or("");
            let status = data["status"].as_str().unwrap_or("active");
            let period_end = data["current_period_end"].as_i64();

            // Determine tier from price/product metadata or plan interval
            // For now we derive from the subscription metadata field
            let tier_str = data["metadata"]["tier"].as_str().unwrap_or("free");
            let tier = SubscriptionTier::from_str(tier_str);

            let valid_until = period_end.map(|ts| {
                chrono::DateTime::from_timestamp(ts, 0)
                    .unwrap_or_default()
                    .fixed_offset()
            });

            if matches!(status, "active" | "trialing") && !customer_id.is_empty() {
                sub_service
                    .update_user_subscription(customer_id, tier, valid_until)
                    .await
                    .unwrap_or_else(|e| tracing::warn!("Sub update failed: {:?}", e));
            }
        }
        "customer.subscription.deleted" => {
            let customer_id = event["data"]["object"]["customer"].as_str().unwrap_or("");
            if !customer_id.is_empty() {
                sub_service
                    .update_user_subscription(customer_id, SubscriptionTier::Free, None)
                    .await
                    .unwrap_or_else(|e| tracing::warn!("Sub cancel failed: {:?}", e));
            }
        }
        _ => tracing::debug!("Unhandled Stripe event: {}", event_type),
    }

    // Mark processed for idempotency
    if !event_id.is_empty() {
        sub_service.mark_event_processed(event_id).await?;
    }

    Ok(HttpResponse::Ok().json(serde_json::json!({ "received": true })))
}
