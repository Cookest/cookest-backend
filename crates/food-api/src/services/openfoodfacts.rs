//! OpenFoodFacts keyless API client — barcode lookups with no API key required.

use reqwest::Client;
use serde::Deserialize;
use crate::errors::AppError;
use crate::models::ingredient::{IngredientDetail, IngredientNutrientDetail, PortionDetail};
use rust_decimal::Decimal;
use std::str::FromStr;

#[derive(Deserialize)]
struct OffResponse {
    status: i32,
    product: Option<OffProduct>,
}

#[derive(Deserialize)]
struct OffProduct {
    product_name: Option<String>,
    categories: Option<String>,
    image_url: Option<String>,
    nutriments: Option<OffNutriments>,
}

#[derive(Deserialize)]
struct OffNutriments {
    #[serde(rename = "energy-kcal_100g")]
    energy_kcal_100g: Option<f64>,
    #[serde(rename = "proteins_100g")]
    proteins_100g: Option<f64>,
    #[serde(rename = "carbohydrates_100g")]
    carbohydrates_100g: Option<f64>,
    #[serde(rename = "fat_100g")]
    fat_100g: Option<f64>,
    #[serde(rename = "fiber_100g")]
    fiber_100g: Option<f64>,
    #[serde(rename = "sugars_100g")]
    sugars_100g: Option<f64>,
    #[serde(rename = "sodium_100g")]
    sodium_100g: Option<f64>,
    #[serde(rename = "saturated-fat_100g")]
    saturated_fat_100g: Option<f64>,
}

pub struct OpenFoodFactsClient {
    http: Client,
}

impl OpenFoodFactsClient {
    pub fn new() -> Self {
        Self { http: Client::new() }
    }

    /// Fetch ingredient detail from OpenFoodFacts by barcode.
    pub async fn get_by_barcode(&self, barcode: &str, ingredient_id: i64) -> Result<IngredientDetail, AppError> {
        let url = format!("https://world.openfoodfacts.org/api/v2/product/{}.json", barcode);

        let resp: OffResponse = self.http
            .get(&url)
            .header("User-Agent", "Cookest-SelfHost/1.0")
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("OpenFoodFacts request error: {}", e)))?
            .json()
            .await
            .map_err(|e| AppError::Internal(format!("OpenFoodFacts parse error: {}", e)))?;

        if resp.status == 0 {
            return Err(AppError::NotFound(format!("Barcode {} not found in OpenFoodFacts", barcode)));
        }

        let product = resp.product.unwrap_or_default();
        let name = product.product_name.unwrap_or_else(|| barcode.to_string());
        let category = product.categories.map(|c| c.split(',').next().unwrap_or("").trim().to_string());
        let image_url = product.image_url.clone();

        let nutrients = product.nutriments.map(|n| IngredientNutrientDetail {
            calories: n.energy_kcal_100g.and_then(|v| Decimal::from_str(&v.to_string()).ok()),
            protein_g: n.proteins_100g.and_then(|v| Decimal::from_str(&v.to_string()).ok()),
            carbs_g: n.carbohydrates_100g.and_then(|v| Decimal::from_str(&v.to_string()).ok()),
            fat_g: n.fat_100g.and_then(|v| Decimal::from_str(&v.to_string()).ok()),
            fiber_g: n.fiber_100g.and_then(|v| Decimal::from_str(&v.to_string()).ok()),
            sugar_g: n.sugars_100g.and_then(|v| Decimal::from_str(&v.to_string()).ok()),
            sodium_mg: n.sodium_100g.map(|v| v * 1000.0).and_then(|v| Decimal::from_str(&v.to_string()).ok()),
            saturated_fat_g: n.saturated_fat_100g.and_then(|v| Decimal::from_str(&v.to_string()).ok()),
            cholesterol_mg: None,
        });

        let portions = vec![PortionDetail {
            description: "100g".to_string(),
            weight_grams: Decimal::from(100),
            unit: Some("g".to_string()),
        }];

        Ok(IngredientDetail {
            id: ingredient_id,
            name,
            category,
            image_url,
            nutrients,
            portions,
        })
    }
}

impl Default for OffProduct {
    fn default() -> Self {
        Self {
            product_name: None,
            categories: None,
            image_url: None,
            nutriments: None,
        }
    }
}
