//! Subscription service — manages user tiers and Stripe webhook processing

use chrono::Utc;
use hmac::{Hmac, Mac};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use sha2::Sha256;
use uuid::Uuid;

use crate::entity::stripe_processed_event;
use crate::entity::user::{self, ActiveModel as UserActiveModel, Entity as User};
use cookest_shared::errors::AppError;
use crate::services::token::SubscriptionTier;

type HmacSha256 = Hmac<Sha256>;

/// Feature gate constants — used by `require_tier` middleware and handlers
pub const FEATURE_AI_MEAL_PLAN: &str = "ai_meal_plan";
pub const FEATURE_PRICE_COMPARISON: &str = "price_comparison";
pub const FEATURE_USER_RECIPES: &str = "user_recipes";
pub const FEATURE_SHOPPING_OPTIMIZER: &str = "shopping_optimizer";

pub struct SubscriptionService {
    db: DatabaseConnection,
    stripe_webhook_secret: Option<String>,
}

impl SubscriptionService {
    pub fn new(db: DatabaseConnection, stripe_webhook_secret: Option<String>) -> Self {
        Self { db, stripe_webhook_secret }
    }

    /// Return the feature list for a given tier
    pub fn features_for_tier(tier: &SubscriptionTier) -> Vec<&'static str> {
        match tier {
            SubscriptionTier::Free => vec![],
            SubscriptionTier::Pro | SubscriptionTier::Family => vec![
                FEATURE_AI_MEAL_PLAN,
                FEATURE_PRICE_COMPARISON,
                FEATURE_USER_RECIPES,
                FEATURE_SHOPPING_OPTIMIZER,
            ],
        }
    }

    /// Verify Stripe webhook signature (HMAC-SHA256 over raw body)
    pub fn verify_stripe_signature(&self, payload: &[u8], sig_header: &str) -> Result<(), AppError> {
        let secret = self.stripe_webhook_secret.as_deref().ok_or_else(|| {
            AppError::Internal("Stripe webhook secret not configured".to_string())
        })?;

        // Stripe signature header format: "t=<timestamp>,v1=<sig1>,v1=<sig2>..."
        let mut timestamp = "";
        let mut signatures: Vec<&str> = vec![];

        for part in sig_header.split(',') {
            if let Some(ts) = part.strip_prefix("t=") {
                timestamp = ts;
            } else if let Some(sig) = part.strip_prefix("v1=") {
                signatures.push(sig);
            }
        }

        if timestamp.is_empty() || signatures.is_empty() {
            return Err(AppError::InvalidToken);
        }

        let signed_payload = format!("{}.{}", timestamp, String::from_utf8_lossy(payload));

        let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
            .map_err(|e| AppError::Internal(format!("HMAC init failed: {}", e)))?;
        mac.update(signed_payload.as_bytes());
        let expected = mac.finalize().into_bytes();
        let expected_hex = format!("{:x}", expected);

        if !signatures.iter().any(|&s| s == expected_hex) {
            tracing::warn!("Stripe webhook signature mismatch");
            return Err(AppError::InvalidToken);
        }

        Ok(())
    }

    /// Check if a Stripe event has already been processed (idempotency)
    pub async fn is_event_processed(&self, event_id: &str) -> Result<bool, AppError> {
        use crate::entity::stripe_processed_event::Entity as StripeEvent;
        Ok(StripeEvent::find_by_id(event_id.to_string())
            .one(&self.db)
            .await?
            .is_some())
    }

    /// Mark a Stripe event as processed
    pub async fn mark_event_processed(&self, event_id: &str) -> Result<(), AppError> {
        let record = stripe_processed_event::ActiveModel {
            event_id: Set(event_id.to_string()),
            processed_at: Set(Utc::now().fixed_offset()),
        };
        record.insert(&self.db).await?;
        Ok(())
    }

    /// Update a user's subscription tier — called by webhook handler
    pub async fn update_user_subscription(
        &self,
        stripe_customer_id: &str,
        tier: SubscriptionTier,
        valid_until: Option<chrono::DateTime<chrono::FixedOffset>>,
    ) -> Result<(), AppError> {
        let user = User::find()
            .filter(user::Column::StripeCustomerId.eq(stripe_customer_id))
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound("User with that Stripe customer ID".to_string()))?;

        let mut active: UserActiveModel = user.into();
        active.subscription_tier = Set(tier.as_str().to_string());
        active.subscription_valid_until = Set(valid_until);
        active.updated_at = Set(Utc::now().fixed_offset());
        active.update(&self.db).await?;

        Ok(())
    }

    /// Link a Stripe customer ID to a user (called after checkout session creation)
    pub async fn set_stripe_customer_id(
        &self,
        user_id: Uuid,
        stripe_customer_id: &str,
    ) -> Result<(), AppError> {
        let user = User::find_by_id(user_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound("User".to_string()))?;

        let mut active: UserActiveModel = user.into();
        active.stripe_customer_id = Set(Some(stripe_customer_id.to_string()));
        active.updated_at = Set(Utc::now().fixed_offset());
        active.update(&self.db).await?;

        Ok(())
    }

    /// Require Pro or Family tier; returns HTTP 402 if the user is on Free tier.
    pub async fn require_pro(&self, claims: &crate::middleware::Claims) -> Result<(), AppError> {
        let tier = claims.tier.as_ref().unwrap_or(&SubscriptionTier::Free);
        match tier {
            SubscriptionTier::Pro | SubscriptionTier::Family => Ok(()),
            SubscriptionTier::Free => Err(AppError::SubscriptionRequired {
                feature: FEATURE_USER_RECIPES.to_string(),
            }),
        }
    }
}
