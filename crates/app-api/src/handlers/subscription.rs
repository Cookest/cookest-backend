//! Subscription handlers — tier info, Stripe checkout, webhooks

use actix_web::{web, HttpRequest, HttpResponse};
use sea_orm::{DatabaseConnection, EntityTrait};
use serde::Deserialize;
use std::sync::Arc;

use crate::entity::user::Entity as User;
use cookest_shared::errors::AppError;
use crate::middleware::auth::AuthenticatedUser;
use crate::services::subscription::SubscriptionService;
use crate::services::token::SubscriptionTier;

/// Public routes — webhook only (no JWT)
pub fn configure_subscription(cfg: &mut web::ServiceConfig) {
    cfg.route("/api/webhooks/stripe", web::post().to(stripe_webhook));
}

/// Protected subscription routes (JWT required)
pub fn configure_subscription_protected(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/subscription")
            .route("", web::get().to(get_subscription))
            .route("/checkout", web::post().to(create_checkout)),
    );
}

/// Return current subscription tier + features list
async fn get_subscription(
    user: AuthenticatedUser,
    db: web::Data<DatabaseConnection>,
    _sub_service: web::Data<Arc<SubscriptionService>>,
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

    let stripe_key = match std::env::var("STRIPE_SECRET_KEY") {
        Ok(k) => k,
        Err(_) => return Ok(HttpResponse::Ok().json(serde_json::json!({
            "message": "Stripe integration pending — configure STRIPE_SECRET_KEY",
            "tier": tier,
            "success_url": body.success_url,
            "cancel_url": body.cancel_url,
            "checkout_url": null,
        }))),
    };

    // Determine price ID from env based on tier
    let price_id = match tier.as_str() {
        "pro" => std::env::var("STRIPE_PRO_PRICE_ID").unwrap_or_else(|_| "price_dummy_pro".to_string()),
        "family" => std::env::var("STRIPE_FAMILY_PRICE_ID").unwrap_or_else(|_| "price_dummy_family".to_string()),
        _ => "price_dummy".to_string(),
    };

    // Call Stripe API
    let client = reqwest::Client::new();
    let res = client.post("https://api.stripe.com/v1/checkout/sessions")
        .basic_auth(&stripe_key, Some(""))
        .form(&[
            ("success_url", body.success_url.as_str()),
            ("cancel_url", body.cancel_url.as_str()),
            ("mode", "subscription"),
            ("line_items[0][price]", price_id.as_str()),
            ("line_items[0][quantity]", "1"),
            ("client_reference_id", user.id.to_string().as_str()),
            ("metadata[tier]", tier.as_str()),
            ("metadata[user_id]", user.id.to_string().as_str()),
        ])
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Stripe request failed: {}", e)))?;

    if !res.status().is_success() {
        let err_body = res.text().await.unwrap_or_default();
        tracing::error!("Stripe API error: {}", err_body);
        return Err(AppError::Internal("Failed to create Stripe checkout session".to_string()));
    }

    let stripe_res: serde_json::Value = res.json().await
        .map_err(|_| AppError::Internal("Failed to parse Stripe response".to_string()))?;

    let checkout_url = stripe_res["url"].as_str().unwrap_or("");

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "checkout_url": checkout_url,
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
                // Propagate errors so Stripe receives a 5xx and retries the webhook.
                // Swallowing the error here would silently leave the user on the wrong tier.
                sub_service
                    .update_user_subscription(customer_id, tier, valid_until)
                    .await?;
            }
        }
        "customer.subscription.deleted" => {
            let customer_id = event["data"]["object"]["customer"].as_str().unwrap_or("");
            if !customer_id.is_empty() {
                // Propagate errors so Stripe receives a 5xx and retries the webhook.
                sub_service
                    .update_user_subscription(customer_id, SubscriptionTier::Free, None)
                    .await?;
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
