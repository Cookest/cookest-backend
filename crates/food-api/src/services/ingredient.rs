//! Ingredient service — supports Local (SeaORM), FatSecret, and Hybrid data sources

use std::sync::Arc;
use std::str::FromStr;
use rust_decimal::Decimal;

use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect};

use crate::config::FoodDataSource;
use crate::errors::AppError;
use crate::models::ingredient::*;
use crate::models::recipe::PaginatedResponse;
use crate::services::FatSecretClient;
use crate::entity::ingredient::{Entity as IngredientEntity, Column as IngredientCol};
use crate::entity::ingredient_nutrient::Entity as NutrientEntity;
use crate::entity::portion_size::Entity as PortionEntity;

pub struct IngredientService {
    db: sea_orm::DatabaseConnection,
    source: FoodDataSource,
    fs_client: Option<Arc<FatSecretClient>>,
}

impl IngredientService {
    pub fn new(
        db: sea_orm::DatabaseConnection,
        source: FoodDataSource,
        fs_client: Option<Arc<FatSecretClient>>,
    ) -> Self {
        Self { db, source, fs_client }
    }

    // -------------------------------------------------------------------------
    // search
    // -------------------------------------------------------------------------

    /// Search ingredients
    pub async fn search(
        &self,
        query: IngredientQuery,
    ) -> Result<PaginatedResponse<IngredientListItem>, AppError> {
        let page = query.page.unwrap_or(1).max(1);
        let per_page = query.per_page.unwrap_or(20).min(100);

        match &self.source {
            FoodDataSource::Local => {
                self.search_local(&query, page, per_page).await
            }
            FoodDataSource::FatSecret => {
                self.search_fatsecret(&query, page, per_page).await
            }
            FoodDataSource::Hybrid => {
                let local = self.search_local(&query, page, per_page).await?;
                if local.total > 0 {
                    Ok(local)
                } else {
                    self.search_fatsecret(&query, page, per_page).await
                }
            }
        }
    }

    async fn search_local(
        &self,
        query: &IngredientQuery,
        page: u64,
        per_page: u64,
    ) -> Result<PaginatedResponse<IngredientListItem>, AppError> {
        let mut q = IngredientEntity::find();

        if let Some(search) = query.q.as_deref() {
            if !search.is_empty() {
                q = q.filter(IngredientCol::Name.contains(search));
            }
        }

        if let Some(category) = query.category.as_deref() {
            q = q.filter(IngredientCol::Category.eq(category));
        }

        let total = q.clone().count(&self.db).await?;
        let offset = ((page - 1) * per_page) as u64;

        let results = q
            .order_by_asc(IngredientCol::Name)
            .limit(per_page)
            .offset(offset)
            .all(&self.db)
            .await?;

        let items: Vec<IngredientListItem> = results
            .into_iter()
            .map(|r| IngredientListItem {
                id: r.id,
                name: r.name,
                category: r.category,
            })
            .collect();

        Ok(PaginatedResponse {
            data: items,
            total,
            page,
            per_page,
            total_pages: (total as f64 / per_page as f64).ceil() as u64,
        })
    }

    async fn search_fatsecret(
        &self,
        query: &IngredientQuery,
        page: u64,
        per_page: u64,
    ) -> Result<PaginatedResponse<IngredientListItem>, AppError> {
        let fs = self.fs_client.as_ref().ok_or_else(|| {
            AppError::Internal("FatSecret client not configured".to_string())
        })?;

        let fs_res = fs
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

    // -------------------------------------------------------------------------
    // get_ingredient
    // -------------------------------------------------------------------------

    /// Get full ingredient detail with nutrients and portions
    pub async fn get_ingredient(&self, id: i64) -> Result<IngredientDetail, AppError> {
        match &self.source {
            FoodDataSource::Local => self.get_ingredient_local(id).await,
            FoodDataSource::FatSecret => self.get_ingredient_fatsecret(id).await,
            FoodDataSource::Hybrid => {
                match self.get_ingredient_local(id).await {
                    Ok(detail) => Ok(detail),
                    Err(AppError::NotFound(_)) => self.get_ingredient_fatsecret(id).await,
                    Err(e) => Err(e),
                }
            }
        }
    }

    async fn get_ingredient_local(&self, id: i64) -> Result<IngredientDetail, AppError> {
        let ingredient = IngredientEntity::find_by_id(id)
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Ingredient {}", id)))?;

        let nutrient_model = NutrientEntity::find()
            .filter(crate::entity::ingredient_nutrient::Column::IngredientId.eq(id))
            .one(&self.db)
            .await?;

        let portion_models = PortionEntity::find()
            .filter(crate::entity::portion_size::Column::IngredientId.eq(id))
            .all(&self.db)
            .await?;

        let nutrients = nutrient_model.map(|n| IngredientNutrientDetail {
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

        let portions: Vec<PortionDetail> = portion_models
            .into_iter()
            .map(|p| PortionDetail {
                description: p.description,
                weight_grams: p.weight_grams,
                unit: p.unit,
            })
            .collect();

        Ok(IngredientDetail {
            id: ingredient.id,
            name: ingredient.name,
            category: ingredient.category,
            nutrients,
            portions,
        })
    }

    async fn get_ingredient_fatsecret(&self, id: i64) -> Result<IngredientDetail, AppError> {
        let fs = self.fs_client.as_ref().ok_or_else(|| {
            AppError::Internal("FatSecret client not configured".to_string())
        })?;

        let fs_res = fs
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

                for serving in servings_list {
                    let weight_grams = serving.metric_serving_amount
                        .as_deref()
                        .and_then(|s| Decimal::from_str(s).ok())
                        .unwrap_or(Decimal::from(100));

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

    // -------------------------------------------------------------------------
    // get_by_barcode
    // -------------------------------------------------------------------------

    /// Resolve a barcode to an ingredient detail
    pub async fn get_by_barcode(&self, barcode: &str) -> Result<IngredientDetail, AppError> {
        match &self.source {
            FoodDataSource::Local => {
                self.get_by_barcode_local(barcode).await
            }
            FoodDataSource::FatSecret => {
                self.get_by_barcode_fatsecret(barcode).await
            }
            FoodDataSource::Hybrid => {
                // Try local DB first (off_id column)
                match self.get_by_barcode_local(barcode).await {
                    Ok(detail) => return Ok(detail),
                    Err(AppError::NotFound(_)) => {}
                    Err(e) => return Err(e),
                }

                // Try OpenFoodFacts
                use crate::services::openfoodfacts::OpenFoodFactsClient;
                let off_client = OpenFoodFactsClient::new();
                match off_client.get_by_barcode(barcode, 0).await {
                    Ok(detail) => return Ok(detail),
                    Err(AppError::NotFound(_)) => {}
                    Err(e) => return Err(e),
                }

                // Fall back to FatSecret
                self.get_by_barcode_fatsecret(barcode).await
            }
        }
    }

    async fn get_by_barcode_local(&self, barcode: &str) -> Result<IngredientDetail, AppError> {
        let ingredient = IngredientEntity::find()
            .filter(IngredientCol::OffId.eq(barcode))
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("No ingredient found for barcode {}", barcode)))?;

        self.get_ingredient_local(ingredient.id).await
    }

    async fn get_by_barcode_fatsecret(&self, barcode: &str) -> Result<IngredientDetail, AppError> {
        let fs = self.fs_client.as_ref().ok_or_else(|| {
            AppError::Internal("FatSecret client not configured".to_string())
        })?;

        let food_id = fs
            .find_food_id_by_barcode(barcode)
            .await
            .map_err(|e| AppError::Internal(format!("FatSecret barcode lookup error: {}", e)))?
            .ok_or_else(|| AppError::NotFound(format!("No food found for barcode {}", barcode)))?;

        self.get_ingredient_fatsecret(food_id).await
    }
}
