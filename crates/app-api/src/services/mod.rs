//! Business-logic services; each module owns one domain area.
pub mod auth;
pub mod chat;
pub mod chat_tools;
pub mod email;
pub mod embeddings;
pub mod household;
pub mod ingredient;
pub mod interaction;
pub mod inventory;
pub mod meal_plan;
pub mod meal_plan_suggestion;
pub mod meal_poll;
pub mod notification;
pub mod nutrition;
pub mod onboarding;
pub mod preference;
pub mod pricing;
pub mod profile;
pub mod push_token;
pub mod recipe;
pub mod recipe_gen;
pub mod scan;
pub mod shopping_list;
pub mod store;
pub mod subscription;
pub mod taste_profile;
pub mod token;

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use uuid::Uuid;

/// Returns the effective user ID for data ownership.
/// If the user is part of a household, returns the household owner's ID.
/// This ensures family members sync their meal plans, inventory, and shopping lists.
pub async fn get_effective_user_id(db: &DatabaseConnection, user_id: Uuid) -> Result<Uuid, String> {
    use crate::entity::{household, household_member};

    let member = household_member::Entity::find()
        .filter(household_member::Column::UserId.eq(user_id))
        .one(db)
        .await
        .map_err(|e| e.to_string())?;

    if let Some(m) = member {
        let h = household::Entity::find_by_id(m.household_id)
            .one(db)
            .await
            .map_err(|e| e.to_string())?;

        if let Some(household) = h {
            return Ok(household.owner_id);
        }
    }

    Ok(user_id)
}

pub use auth::AuthService;
pub use chat::ChatService;
pub use email::EmailService;
pub use household::HouseholdService;
pub use ingredient::IngredientService;
pub use interaction::InteractionService;
pub use inventory::InventoryService;
pub use meal_plan::MealPlanService;
pub use meal_plan_suggestion::MealPlanSuggestionService;
pub use meal_poll::MealPollService;
pub use notification::NotificationService;
pub use nutrition::NutritionService;
pub use onboarding::OnboardingService;
pub use preference::PreferenceService;
pub use pricing::PricingService;
pub use profile::ProfileService;
pub use push_token::PushTokenService;
pub use recipe::RecipeService;
pub use recipe_gen::RecipeGenService;
pub use scan::ScanService;
pub use shopping_list::ShoppingListService;
pub use store::StoreService;
pub use subscription::SubscriptionService;
pub use token::TokenService;
