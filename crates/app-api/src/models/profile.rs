use serde::{Deserialize, Serialize};
use validator::Validate;

/// Request to update user profile
#[derive(Debug, Deserialize, Validate)]
pub struct UpdateProfileRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: Option<String>,

    /// Number of people in the household (used to scale recipes)
    #[validate(range(min = 1, max = 50))]
    pub household_size: Option<i32>,

    /// e.g. ["vegetarian", "vegan", "gluten_free", "dairy_free"]
    pub dietary_restrictions: Option<Vec<String>>,

    /// e.g. ["nuts", "shellfish", "eggs"]
    pub allergies: Option<Vec<String>>,

    /// URL to avatar image (set by separate upload endpoint)
    pub avatar_url: Option<String>,
}

/// Full user profile response
#[derive(Debug, Serialize)]
pub struct ProfileResponse {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
    pub household_size: i32,
    pub dietary_restrictions: Option<Vec<String>>,
    pub allergies: Option<Vec<String>>,
    pub avatar_url: Option<String>,
    pub is_email_verified: bool,
    pub two_factor_enabled: bool,
    pub created_at: String,
}
