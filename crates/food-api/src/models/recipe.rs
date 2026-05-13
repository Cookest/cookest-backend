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
    /// Filter by language: "en" | "fr" | "de" | "it" | "es" | "pt"
    pub language: Option<String>,
    /// Filter by tag (single tag match) e.g. "quick"
    pub tag: Option<String>,
    /// Filter by source site e.g. "bbcgoodfood.com"
    pub source_site: Option<String>,
    /// Vegetarian only
    pub vegetarian: Option<bool>,
    /// Vegan only
    pub vegan: Option<bool>,
    /// Gluten-free only
    pub gluten_free: Option<bool>,
    /// Dairy-free only
    pub dairy_free: Option<bool>,
    /// Nut-free only
    pub nut_free: Option<bool>,
    /// Max total time in minutes
    pub max_time: Option<i32>,
    /// Sort field: "name" | "rating" | "time" | "created" | "random"
    pub sort: Option<String>,
    /// Sort direction: "asc" | "desc" (default: "asc" for name/time, "desc" for rating/created)
    pub order: Option<String>,
    /// Page number (1-indexed)
    pub page: Option<u64>,
    /// Results per page (max 100)
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
    pub language: String,
    pub tags: Option<Vec<String>>,
    pub source_site: Option<String>,
    pub average_rating: Option<rust_decimal::Decimal>,
    pub rating_count: i32,
    pub primary_image_url: Option<String>,
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
    pub source_site: Option<String>,
    pub language: String,
    pub tags: Option<Vec<String>>,
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
    pub local_path: Option<String>,
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
    pub cholesterol_mg: Option<rust_decimal::Decimal>,
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

/// Create a recipe
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

/// Simple pagination params
#[derive(Debug, Deserialize)]
pub struct PaginationQuery {
    pub page: Option<u64>,
    pub per_page: Option<u64>,
}

/// Database statistics response
#[derive(Debug, Serialize)]
pub struct StatsResponse {
    pub total_recipes: i64,
    pub total_ingredients: i64,
    pub total_images: i64,
    pub recipes_with_nutrition: i64,
    pub recipes_with_steps: i64,
    pub by_language: Vec<CountByField>,
    pub by_category: Vec<CountByField>,
    pub by_source: Vec<CountByField>,
}

#[derive(Debug, Serialize)]
pub struct CountByField {
    pub field: String,
    pub count: i64,
}

/// Random recipe query params
#[derive(Debug, Deserialize)]
pub struct RandomQuery {
    /// Number of random recipes to return (max 20, default 5)
    pub count: Option<u32>,
    pub language: Option<String>,
    pub category: Option<String>,
    pub vegetarian: Option<bool>,
    pub vegan: Option<bool>,
}

/// Ingredient-based recipe search params
#[derive(Debug, Deserialize)]
pub struct ByIngredientQuery {
    pub name: String,
    pub page: Option<u64>,
    pub per_page: Option<u64>,
}
