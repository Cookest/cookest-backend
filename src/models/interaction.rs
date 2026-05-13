use serde::{Deserialize, Serialize};
use validator::Validate;

/// Request to rate a recipe
#[derive(Debug, Deserialize, Validate)]
pub struct RateRecipeRequest {
    #[validate(range(min = 1, max = 5))]
    pub rating: i16,

    #[validate(length(max = 1000))]
    pub comment: Option<String>,
}

/// Response after rating / cooking
#[derive(Debug, Serialize)]
pub struct InteractionResponse {
    pub message: String,
}

/// Response for favourite status
#[derive(Debug, Serialize)]
pub struct FavouriteResponse {
    pub recipe_id: i64,
    pub is_favourited: bool,
}

/// Cooking history entry
#[derive(Debug, Serialize)]
pub struct CookingHistoryItem {
    pub id: i64,
    pub recipe_id: i64,
    pub recipe_name: String,
    pub servings_made: i32,
    pub inventory_deducted: bool,
    pub cooked_at: String,
}
