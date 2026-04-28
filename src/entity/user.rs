//! User entity for SeaORM
//!
//! Security notes:
//! - password_hash is never serialized to JSON
//! - refresh_token_hash stored for secure token rotation
//! - stripe_customer_id excluded from public responses

use rust_decimal::Decimal;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,

    #[sea_orm(unique)]
    pub email: String,

    pub name: Option<String>,

    /// Argon2id hashed password - NEVER expose this
    #[sea_orm(column_type = "Text")]
    pub password_hash: String,

    /// Hashed refresh token for secure token rotation
    #[sea_orm(column_type = "Text", nullable)]
    pub refresh_token_hash: Option<String>,

    /// Number of people in the household — used for auto-scaling recipe portions
    pub household_size: i32,

    /// Dietary restrictions stored as an array of strings: e.g. ["vegetarian", "gluten_free"]
    pub dietary_restrictions: Option<Vec<String>>,

    /// Food allergies stored as an array of strings: e.g. ["nuts", "shellfish"]
    pub allergies: Option<Vec<String>>,

    /// URL to profile avatar stored in S3
    #[sea_orm(column_type = "Text", nullable)]
    pub avatar_url: Option<String>,

    /// Whether the user's email has been verified
    pub is_email_verified: bool,

    /// Whether 2FA is enabled
    pub two_factor_enabled: bool,

    /// TOTP secret for 2FA (encrypted)
    #[sea_orm(column_type = "Text", nullable)]
    pub totp_secret: Option<String>,

    /// Tracks failed login attempts for account lockout
    pub failed_login_attempts: i32,

    /// Account lockout until this time
    pub locked_until: Option<DateTimeWithTimeZone>,

    // ── Subscription ───────────────────────────────────────────────────
    /// Subscription tier: "free", "pro", or "family"
    pub subscription_tier: String,

    /// When the current paid subscription expires (NULL for free tier)
    pub subscription_valid_until: Option<DateTimeWithTimeZone>,

    /// Stripe customer ID for billing — never expose in public responses
    #[sea_orm(column_type = "Text", nullable)]
    pub stripe_customer_id: Option<String>,

    // ── Onboarding & Profile ────────────────────────────────────────────
    /// "beginner", "intermediate", "advanced"
    pub cooking_skill_level: Option<String>,

    /// Preferred cuisine types e.g. ["italian", "asian"]
    pub preferred_cuisines: Option<Vec<String>>,

    /// Health / fitness goals e.g. ["weight_loss", "muscle_gain"]
    pub health_goals: Option<Vec<String>>,

    /// Weekly grocery budget in the user's currency
    #[sea_orm(column_type = "Decimal(Some((10, 2)))", nullable)]
    pub weekly_budget: Option<Decimal>,

    /// Maximum preferred cooking time per meal in minutes
    pub preferred_time_per_meal_min: Option<i32>,

    /// Whether the user has completed the onboarding flow
    pub onboarding_completed: bool,

    /// Admin flag — checked from DB for privileged endpoints (never trusted from JWT alone)
    pub is_admin: bool,

    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::inventory_item::Entity")]
    InventoryItems,

    #[sea_orm(has_many = "super::user_favorite::Entity")]
    UserFavorites,

    #[sea_orm(has_many = "super::recipe_rating::Entity")]
    RecipeRatings,

    #[sea_orm(has_many = "super::cooking_history::Entity")]
    CookingHistory,

    #[sea_orm(has_many = "super::meal_plan::Entity")]
    MealPlans,

    #[sea_orm(has_many = "super::chat_session::Entity")]
    ChatSessions,

    #[sea_orm(has_many = "super::shopping_list_item::Entity")]
    ShoppingListItems,

    #[sea_orm(has_many = "super::user_push_token::Entity")]
    PushTokens,
}

impl Related<super::inventory_item::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::InventoryItems.def()
    }
}

impl Related<super::user_favorite::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::UserFavorites.def()
    }
}

impl Related<super::recipe_rating::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::RecipeRatings.def()
    }
}

impl Related<super::cooking_history::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::CookingHistory.def()
    }
}

impl Related<super::meal_plan::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MealPlans.def()
    }
}

impl Related<super::chat_session::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ChatSessions.def()
    }
}

impl Related<super::shopping_list_item::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ShoppingListItems.def()
    }
}

impl Related<super::user_push_token::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PushTokens.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

/// Safe user representation for API responses — excludes all sensitive fields
#[derive(Debug, Serialize, Deserialize)]
pub struct UserResponse {
    pub id: Uuid,
    pub email: String,
    pub name: Option<String>,
    pub household_size: i32,
    pub dietary_restrictions: Option<Vec<String>>,
    pub allergies: Option<Vec<String>>,
    pub avatar_url: Option<String>,
    pub is_email_verified: bool,
    pub two_factor_enabled: bool,
    // Onboarding & preferences
    pub cooking_skill_level: Option<String>,
    pub preferred_cuisines: Option<Vec<String>>,
    pub health_goals: Option<Vec<String>>,
    pub weekly_budget: Option<Decimal>,
    pub preferred_time_per_meal_min: Option<i32>,
    pub onboarding_completed: bool,
    // Subscription (tier only — no stripe IDs)
    pub subscription_tier: String,
    pub subscription_valid_until: Option<DateTimeWithTimeZone>,
    pub created_at: DateTimeWithTimeZone,
}

impl From<Model> for UserResponse {
    fn from(user: Model) -> Self {
        Self {
            id: user.id,
            email: user.email,
            name: user.name,
            household_size: user.household_size,
            dietary_restrictions: user.dietary_restrictions,
            allergies: user.allergies,
            avatar_url: user.avatar_url,
            is_email_verified: user.is_email_verified,
            two_factor_enabled: user.two_factor_enabled,
            cooking_skill_level: user.cooking_skill_level,
            preferred_cuisines: user.preferred_cuisines,
            health_goals: user.health_goals,
            weekly_budget: user.weekly_budget,
            preferred_time_per_meal_min: user.preferred_time_per_meal_min,
            onboarding_completed: user.onboarding_completed,
            subscription_tier: user.subscription_tier,
            subscription_valid_until: user.subscription_valid_until,
            created_at: user.created_at,
        }
    }
}

