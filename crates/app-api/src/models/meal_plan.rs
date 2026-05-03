use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use validator::Validate;

/// Request to generate a meal plan
#[derive(Debug, Deserialize, Validate)]
pub struct GenerateMealPlanRequest {
    /// Week start date (must be a Monday)
    pub week_start: NaiveDate,
}

/// Meal plan slot response
#[derive(Debug, Serialize)]
pub struct MealPlanSlotResponse {
    pub id: i64,
    pub day_of_week: i16,
    pub meal_type: String,
    pub recipe_id: i64,
    pub recipe_name: String,
    pub recipe_image_url: Option<String>,
    pub total_time_min: Option<i32>,
    pub servings: i32,
    pub is_completed: bool,
}

/// Full week meal plan response
#[derive(Debug, Serialize)]
pub struct MealPlanResponse {
    pub id: i64,
    pub week_start: NaiveDate,
    pub is_ai_generated: bool,
    pub slots: Vec<MealPlanSlotResponse>,
}
