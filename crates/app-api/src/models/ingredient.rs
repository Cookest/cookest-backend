use serde::{Deserialize, Serialize};


/// Query params for ingredient search (autocomplete)
#[derive(Debug, Deserialize)]
pub struct IngredientQuery {
    pub q: Option<String>,
    pub category: Option<String>,
    pub page: Option<u64>,
    pub per_page: Option<u64>,
}

/// Lightweight ingredient list item
#[derive(Debug, Serialize)]
pub struct IngredientListItem {
    pub id: i64,
    pub name: String,
    pub category: Option<String>,
}

/// Full ingredient detail with nutrients
#[derive(Debug, Serialize)]
pub struct IngredientDetail {
    pub id: i64,
    pub name: String,
    pub category: Option<String>,
    pub nutrients: Option<IngredientNutrientDetail>,
    pub portions: Vec<PortionDetail>,
}

#[derive(Debug, Serialize)]
pub struct IngredientNutrientDetail {
    pub calories: Option<rust_decimal::Decimal>,
    pub protein_g: Option<rust_decimal::Decimal>,
    pub carbs_g: Option<rust_decimal::Decimal>,
    pub fat_g: Option<rust_decimal::Decimal>,
    pub fiber_g: Option<rust_decimal::Decimal>,
    pub sugar_g: Option<rust_decimal::Decimal>,
    pub sodium_mg: Option<rust_decimal::Decimal>,
    pub saturated_fat_g: Option<rust_decimal::Decimal>,
    pub cholesterol_mg: Option<rust_decimal::Decimal>,
}

#[derive(Debug, Serialize)]
pub struct PortionDetail {
    pub description: String,
    pub weight_grams: rust_decimal::Decimal,
    pub unit: Option<String>,
}
