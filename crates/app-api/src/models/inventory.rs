use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Request to add an item to inventory
#[derive(Debug, Deserialize)]
pub struct AddInventoryItem {
    pub ingredient_id: i64,
    pub custom_name: Option<String>,
    pub quantity: Decimal,
    pub unit: String,
    pub expiry_date: Option<NaiveDate>,
    pub storage_location: Option<String>,
}

/// Quick-add: add by name (creates ingredient if needed)
#[derive(Debug, Deserialize)]
pub struct QuickAddItem {
    pub name: String,
    pub quantity: f64,
    pub unit: String,
    pub storage_location: Option<String>,
    pub expiry_date: Option<String>,
}

/// Request to update an existing inventory item
#[derive(Debug, Deserialize)]
pub struct UpdateInventoryItem {
    pub quantity: Option<Decimal>,
    pub unit: Option<String>,
    pub expiry_date: Option<NaiveDate>,
    pub storage_location: Option<String>,
}

/// Inventory item response
#[derive(Debug, Serialize)]
pub struct InventoryItemResponse {
    pub id: i64,
    pub ingredient_id: i64,
    pub ingredient_name: String,
    pub custom_name: Option<String>,
    pub quantity: Decimal,
    pub unit: String,
    pub expiry_date: Option<NaiveDate>,
    pub storage_location: Option<String>,
    /// Days until expiry: negative = already expired, None = no expiry date
    pub days_until_expiry: Option<i64>,
    /// True if expiring within 5 days
    pub expiry_warning: bool,
}

/// A recipe suggestion based on pantry contents
#[derive(Debug, Serialize)]
pub struct RecipeSuggestion {
    pub recipe_id: i64,
    pub name: String,
    pub slug: String,
    pub primary_image_url: Option<String>,
    pub total_time_min: Option<i32>,
    pub difficulty: Option<String>,
    /// How many of the required ingredients the user has
    pub ingredients_have: i32,
    /// Total required ingredients
    pub ingredients_total: i32,
    /// Match percentage (0–100)
    pub match_pct: i32,
}
