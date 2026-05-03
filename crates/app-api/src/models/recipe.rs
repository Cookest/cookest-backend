use serde::{Deserialize, Serialize};
use validator::Validate;

/// Query params for listing/filtering recipes
#[derive(Debug, Deserialize)]
pub struct RecipeQuery {
    /// Full-text search
    pub q: Option<String>,
    /// Filter by cuisine e.g. "Italian"
    pub cuisine: Option<String>,
    /// Filter by category e.g. "dinner"
    pub category: Option<String>,
    /// Filter by difficulty: "easy" | "medium" | "hard"
    pub difficulty: Option<String>,
    /// Vegetarian only
    pub vegetarian: Option<bool>,
    /// Vegan only
    pub vegan: Option<bool>,
    /// Gluten-free only
    pub gluten_free: Option<bool>,
    /// Dairy-free only
    pub dairy_free: Option<bool>,
    /// Max total time in minutes
    pub max_time: Option<i32>,
    /// If true (auth required), adds match_pct field (owned ingredients / total ingredients)
    pub match_inventory: Option<bool>,
    /// Page number (1-indexed)
    pub page: Option<u64>,
    /// Results per page (max 50)
    pub per_page: Option<u64>,
}

/// Lightweight recipe list item
#[derive(Debug, Serialize)]
pub struct RecipeListItem {
    pub id: i64,
    pub name: String,
    pub slug: String,
    pub cuisine: Option<String>,
    pub category: Option<String>,
    pub difficulty: Option<String>,
    pub servings: i32,
    pub total_time_min: Option<i32>,
    pub is_vegetarian: bool,
    pub is_vegan: bool,
    pub is_gluten_free: bool,
    pub is_dairy_free: bool,
    pub average_rating: Option<rust_decimal::Decimal>,
    pub rating_count: i32,
    pub primary_image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owned_ingredients: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_ingredients: Option<usize>,
}

/// Full recipe detail response
#[derive(Debug, Serialize)]
pub struct RecipeDetail {
    pub id: i64,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub cuisine: Option<String>,
    pub category: Option<String>,
    pub difficulty: Option<String>,
    pub servings: i32,
    pub prep_time_min: Option<i32>,
    pub cook_time_min: Option<i32>,
    pub total_time_min: Option<i32>,
    pub is_vegetarian: bool,
    pub is_vegan: bool,
    pub is_gluten_free: bool,
    pub is_dairy_free: bool,
    pub is_nut_free: bool,
    pub source_url: Option<String>,
    pub average_rating: Option<rust_decimal::Decimal>,
    pub rating_count: i32,
    pub ingredients: Vec<RecipeIngredientDetail>,
    pub steps: Vec<RecipeStepDetail>,
    pub images: Vec<RecipeImageDetail>,
    pub nutrition: Option<RecipeNutritionDetail>,
}

#[derive(Debug, Serialize)]
pub struct RecipeIngredientDetail {
    pub id: i64,
    pub ingredient_id: i64,
    pub ingredient_name: String,
    pub quantity: Option<rust_decimal::Decimal>,
    pub unit: Option<String>,
    pub quantity_grams: Option<rust_decimal::Decimal>,
    pub notes: Option<String>,
    pub display_order: i32,
}

#[derive(Debug, Serialize)]
pub struct RecipeStepDetail {
    pub id: i64,
    pub step_number: i32,
    pub instruction: String,
    pub duration_min: Option<i32>,
    pub image_url: Option<String>,
    pub tip: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RecipeImageDetail {
    pub id: i64,
    pub url: String,
    pub image_type: Option<String>,
    pub is_primary: bool,
    pub width: Option<i32>,
    pub height: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct RecipeNutritionDetail {
    pub calories: Option<rust_decimal::Decimal>,
    pub protein_g: Option<rust_decimal::Decimal>,
    pub carbs_g: Option<rust_decimal::Decimal>,
    pub fat_g: Option<rust_decimal::Decimal>,
    pub fiber_g: Option<rust_decimal::Decimal>,
    pub sugar_g: Option<rust_decimal::Decimal>,
    pub sodium_mg: Option<rust_decimal::Decimal>,
    pub saturated_fat_g: Option<rust_decimal::Decimal>,
    pub per_serving: bool,
}

/// Paginated response wrapper
#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub total: u64,
    pub page: u64,
    pub per_page: u64,
    pub total_pages: u64,
}

/// Request to scale a recipe
#[derive(Debug, Deserialize, Validate)]
pub struct ScaleRequest {
    #[validate(range(min = 1, max = 100))]
    pub servings: i32,
}

/// Create or update a user recipe
#[derive(Debug, Deserialize, Validate)]
pub struct CreateRecipeRequest {
    #[validate(length(min = 1, max = 200))]
    pub name: String,
    pub description: Option<String>,
    pub cuisine: Option<String>,
    /// "breakfast" | "lunch" | "dinner" | "snack" | "dessert"
    pub category: Option<String>,
    /// "easy" | "medium" | "hard"
    pub difficulty: Option<String>,
    #[validate(range(min = 1, max = 100))]
    pub servings: Option<i32>,
    pub prep_time_min: Option<i32>,
    pub cook_time_min: Option<i32>,
    pub is_vegetarian: Option<bool>,
    pub is_vegan: Option<bool>,
    pub is_gluten_free: Option<bool>,
    pub is_dairy_free: Option<bool>,
    pub is_nut_free: Option<bool>,
    /// Whether this recipe is visible to other users
    pub is_public: Option<bool>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateRecipeRequest {
    #[validate(length(min = 1, max = 200))]
    pub name: Option<String>,
    pub description: Option<String>,
    pub cuisine: Option<String>,
    pub category: Option<String>,
    pub difficulty: Option<String>,
    pub servings: Option<i32>,
    pub prep_time_min: Option<i32>,
    pub cook_time_min: Option<i32>,
    pub is_vegetarian: Option<bool>,
    pub is_vegan: Option<bool>,
    pub is_gluten_free: Option<bool>,
    pub is_dairy_free: Option<bool>,
    pub is_nut_free: Option<bool>,
    pub is_public: Option<bool>,
}

/// Simple pagination params (used where no recipe-specific filters are needed)
#[derive(Debug, Deserialize)]
pub struct PaginationQuery {
    pub page: Option<u64>,
    pub per_page: Option<u64>,
}
