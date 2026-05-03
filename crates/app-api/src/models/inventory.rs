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
