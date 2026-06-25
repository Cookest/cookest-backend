//! Ingredient service — searches ingredients via food-api and caches/details them locally

use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    ActiveModelTrait, Set, TransactionTrait,
};
use chrono::Utc;
use std::sync::Arc;

use crate::entity::{ingredient, ingredient_nutrient, portion_size};
use crate::handlers::browse::FoodApiClient;
use cookest_shared::errors::AppError;
use crate::models::ingredient::*;
use crate::models::recipe::PaginatedResponse;

pub struct IngredientService {
    db: DatabaseConnection,
    food_api_client: FoodApiClient,
}

impl IngredientService {
    pub fn new(db: DatabaseConnection, food_api_client: FoodApiClient) -> Self {
        Self { db, food_api_client }
    }

    /// Search ingredients (used for inventory autocomplete) — proxies to food-api
    pub async fn search(
        &self,
        query: IngredientQuery,
    ) -> Result<PaginatedResponse<IngredientListItem>, AppError> {
        let q = query.q.unwrap_or_default();
        let page = query.page.unwrap_or(1);
        let per_page = query.per_page.unwrap_or(20);

        let path = format!("/api/v1/ingredients?q={}&page={}&per_page={}", q, page, per_page);
        let req = self.food_api_client.get(&path);
        
        let resp = req.send().await
            .map_err(|e| AppError::Internal(format!("Failed to search ingredients via food-api: {}", e)))?;
            
        let result = resp.json::<PaginatedResponse<IngredientListItem>>().await
            .map_err(|e| AppError::Internal(format!("Failed to parse search results from food-api: {}", e)))?;

        Ok(result)
    }

    /// Get full ingredient detail with nutrients and portions, caching it locally if missing
    pub async fn get_ingredient(&self, id: i64) -> Result<IngredientDetail, AppError> {
        // 1. Try local lookup first
        let existing = ingredient::Entity::find_by_id(id)
            .one(&self.db)
            .await?;

        if let Some(ing) = existing {
            let nutrients = ingredient_nutrient::Entity::find()
                .filter(ingredient_nutrient::Column::IngredientId.eq(id))
                .one(&self.db)
                .await?
                .map(|n| IngredientNutrientDetail {
                    calories: n.calories,
                    protein_g: n.protein_g,
                    carbs_g: n.carbs_g,
                    fat_g: n.fat_g,
                    fiber_g: n.fiber_g,
                    sugar_g: n.sugar_g,
                    sodium_mg: n.sodium_mg,
                    saturated_fat_g: n.saturated_fat_g,
                    cholesterol_mg: n.cholesterol_mg,
                });

            let portions = portion_size::Entity::find()
                .filter(portion_size::Column::IngredientId.eq(id))
                .all(&self.db)
                .await?
                .into_iter()
                .map(|p| PortionDetail {
                    description: p.description,
                    weight_grams: p.weight_grams,
                    unit: p.unit,
                })
                .collect();

            return Ok(IngredientDetail {
                id: ing.id,
                name: ing.name,
                category: ing.category,
                image_url: ing.image_url,
                nutrients,
                portions,
            });
        }

        // 2. Fetch from food-api if missing locally
        let path = format!("/api/v1/ingredients/{}", id);
        let req = self.food_api_client.get(&path);
        
        let resp = req.send().await
            .map_err(|e| AppError::Internal(format!("Failed to reach food-api: {}", e)))?;
            
        let fs_ing = resp.json::<IngredientDetail>().await
            .map_err(|e| AppError::Internal(format!("Failed to parse ingredient detail from food-api: {}", e)))?;

        // 3. Cache/insert the ingredient into the local app-db
        let txn_db = self.db.clone();
        let fs_ing_clone = fs_ing.clone();
        txn_db.transaction::<_, (), AppError>(move |txn| {
            Box::pin(async move {
                if ingredient::Entity::find_by_id(fs_ing_clone.id).one(txn).await?.is_none() {
                    let ing_model = ingredient::ActiveModel {
                        id: Set(fs_ing_clone.id),
                        name: Set(fs_ing_clone.name.clone()),
                        category: Set(fs_ing_clone.category.clone()),
                        created_at: Set(Utc::now().fixed_offset()),
                        ..Default::default()
                    };
                    ing_model.insert(txn).await?;

                    if let Some(nut) = &fs_ing_clone.nutrients {
                        let nut_model = ingredient_nutrient::ActiveModel {
                            ingredient_id: Set(fs_ing_clone.id),
                            calories: Set(nut.calories),
                            protein_g: Set(nut.protein_g),
                            carbs_g: Set(nut.carbs_g),
                            fat_g: Set(nut.fat_g),
                            fiber_g: Set(nut.fiber_g),
                            sugar_g: Set(nut.sugar_g),
                            sodium_mg: Set(nut.sodium_mg),
                            saturated_fat_g: Set(nut.saturated_fat_g),
                            cholesterol_mg: Set(nut.cholesterol_mg),
                            ..Default::default()
                        };
                        nut_model.insert(txn).await?;
                    }

                    for p in &fs_ing_clone.portions {
                        let p_model = portion_size::ActiveModel {
                            ingredient_id: Set(fs_ing_clone.id),
                            description: Set(p.description.clone()),
                            weight_grams: Set(p.weight_grams),
                            unit: Set(p.unit.clone()),
                            ..Default::default()
                        };
                        p_model.insert(txn).await?;
                    }
                }
                Ok(())
            })
        }).await.map_err(|e| match e {
            sea_orm::TransactionError::Connection(de) => AppError::from(de),
            sea_orm::TransactionError::Transaction(ae) => ae,
        })?;

        Ok(fs_ing)
    }
}
