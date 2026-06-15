//! Ingredient service — queries ingredients using the FatSecret API

use std::sync::Arc;
use std::str::FromStr;
use rust_decimal::Decimal;

use crate::errors::AppError;
use crate::models::ingredient::*;
use crate::models::recipe::PaginatedResponse;
use crate::services::FatSecretClient;

pub struct IngredientService {
    db: sea_orm::DatabaseConnection,
    fs_client: Arc<FatSecretClient>,
}

impl IngredientService {
    pub fn new(db: sea_orm::DatabaseConnection, fs_client: Arc<FatSecretClient>) -> Self {
        Self { db, fs_client }
    }

    /// Search ingredients via FatSecret
    pub async fn search(
        &self,
        query: IngredientQuery,
    ) -> Result<PaginatedResponse<IngredientListItem>, AppError> {
        let page = query.page.unwrap_or(1).max(1);
        let per_page = query.per_page.unwrap_or(20).min(100);

        let fs_res = self.fs_client
            .search_ingredients(query.q.as_deref(), page - 1, per_page)
            .await
            .map_err(|e| AppError::Internal(format!("FatSecret search ingredients error: {}", e)))?;

        let mut items = Vec::new();
        let mut total = 0;

        if let Some(body) = fs_res.foods {
            if let Some(food_list) = body.food {
                for r in food_list {
                    let id = r.food_id.parse::<i64>().unwrap_or(0);
                    let category = r.brand_name.clone().or(Some(r.food_type.clone()));
                    items.push(IngredientListItem {
                        id,
                        name: r.food_name,
                        category,
                    });
                }
            }
            total = body.total_results
                .and_then(|t| t.parse::<u64>().ok())
                .unwrap_or(items.len() as u64);
        }

        Ok(PaginatedResponse {
            data: items,
            total,
            page,
            per_page,
            total_pages: (total as f64 / per_page as f64).ceil() as u64,
        })
    }

    /// Get full ingredient detail with nutrients and portions via FatSecret
    pub async fn get_ingredient(&self, id: i64) -> Result<IngredientDetail, AppError> {
        let fs_res = self.fs_client
            .get_ingredient(id)
            .await
            .map_err(|e| AppError::NotFound(format!("Ingredient ID {} not found in FatSecret: {}", id, e)))?;

        let food = fs_res.food;
        let food_id = food.food_id.parse::<i64>().unwrap_or(id);
        let category = food.brand_name.clone().or(Some(food.food_type.clone()));

        let mut nutrients = None;
        let mut portions = Vec::new();

        if let Some(servings_wrapper) = food.servings {
            if let Some(servings_list) = servings_wrapper.serving {
                // Map the first serving as the primary nutrition profile
                if let Some(first_serving) = servings_list.first() {
                    nutrients = Some(IngredientNutrientDetail {
                        calories: first_serving.calories.as_deref().and_then(|s| Decimal::from_str(s).ok()),
                        protein_g: first_serving.protein.as_deref().and_then(|s| Decimal::from_str(s).ok()),
                        carbs_g: first_serving.carbohydrate.as_deref().and_then(|s| Decimal::from_str(s).ok()),
                        fat_g: first_serving.fat.as_deref().and_then(|s| Decimal::from_str(s).ok()),
                        fiber_g: first_serving.fiber.as_deref().and_then(|s| Decimal::from_str(s).ok()),
                        sugar_g: first_serving.sugar.as_deref().and_then(|s| Decimal::from_str(s).ok()),
                        sodium_mg: first_serving.sodium.as_deref().and_then(|s| Decimal::from_str(s).ok()),
                        saturated_fat_g: first_serving.saturated_fat.as_deref().and_then(|s| Decimal::from_str(s).ok()),
                        cholesterol_mg: first_serving.cholesterol.as_deref().and_then(|s| Decimal::from_str(s).ok()),
                    });
                }

                // Map all servings to portions
                for serving in servings_list {
                    let weight_grams = serving.metric_serving_amount
                        .as_deref()
                        .and_then(|s| Decimal::from_str(s).ok())
                        .unwrap_or(Decimal::from(100)); // Default fallback

                    portions.push(PortionDetail {
                        description: serving.serving_description,
                        weight_grams,
                        unit: serving.metric_serving_unit,
                    });
                }
            }
        }

        Ok(IngredientDetail {
            id: food_id,
            name: food.food_name,
            category,
            nutrients,
            portions,
        })
    }
}
