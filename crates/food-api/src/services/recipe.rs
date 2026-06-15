//! Recipe service — queries recipes using the FatSecret API

use std::sync::Arc;
use std::str::FromStr;
use rust_decimal::Decimal;

use crate::errors::AppError;
use crate::models::recipe::*;
use crate::services::FatSecretClient;

pub struct RecipeService {
    db: sea_orm::DatabaseConnection,
    fs_client: Arc<FatSecretClient>,
}

impl RecipeService {
    pub fn new(db: sea_orm::DatabaseConnection, fs_client: Arc<FatSecretClient>) -> Self {
        Self { db, fs_client }
    }

    /// List recipes with filters and pagination via FatSecret
    pub async fn list_recipes(
        &self,
        query: RecipeQuery,
    ) -> Result<PaginatedResponse<RecipeListItem>, AppError> {
        let page = query.page.unwrap_or(1).max(1);
        let per_page = query.per_page.unwrap_or(20).min(50);

        let fs_res = self.fs_client
            .search_recipes(query.q.as_deref(), page - 1, per_page)
            .await
            .map_err(|e| AppError::Internal(format!("FatSecret search error: {}", e)))?;

        let mut items = Vec::new();
        let mut total = 0;

        if let Some(body) = fs_res.recipes {
            if let Some(recipe_list) = body.recipe {
                for r in recipe_list {
                    let id = r.recipe_id.parse::<i64>().unwrap_or(0);
                    let slug = format!("{}-{}", slug::slugify(&r.recipe_name), id);
                    
                    // Nutrition fields parsed but unused for RecipeListItem


                    items.push(RecipeListItem {
                        id,
                        name: r.recipe_name,
                        slug,
                        cuisine: None,
                        category: None,
                        difficulty: None,
                        servings: 2,
                        total_time_min: None,
                        is_vegetarian: false,
                        is_vegan: false,
                        is_gluten_free: false,
                        is_dairy_free: false,
                        average_rating: None,
                        rating_count: 0,
                        primary_image_url: r.recipe_image,
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

    /// Get full recipe detail by ID via FatSecret
    pub async fn get_recipe(&self, id: i64) -> Result<RecipeDetail, AppError> {
        let fs_res = self.fs_client
            .get_recipe(id)
            .await
            .map_err(|e| AppError::NotFound(format!("Recipe ID {} not found in FatSecret: {}", id, e)))?;

        let r = fs_res.recipe;
        let recipe_id = r.recipe_id.parse::<i64>().unwrap_or(id);
        let slug = format!("{}-{}", slug::slugify(&r.recipe_name), recipe_id);
        let servings = r.servings.as_deref().and_then(|s| s.parse::<i32>().ok()).unwrap_or(2);
        
        let prep_time_min = r.prep_time_min.as_deref().and_then(|s| s.parse::<i32>().ok());
        let cook_time_min = r.cook_time_min.as_deref().and_then(|s| s.parse::<i32>().ok());
        let total_time_min = prep_time_min.zip(cook_time_min).map(|(p, c)| p + c);

        // Map ingredients
        let mut ingredients = Vec::new();
        if let Some(ings_wrapper) = r.ingredients {
            if let Some(ing_list) = ings_wrapper.ingredient {
                for (idx, ing) in ing_list.into_iter().enumerate() {
                    let food_id = ing.food_id.parse::<i64>().unwrap_or(0);
                    let quantity = ing.number_of_units.as_deref().and_then(|s| Decimal::from_str(s).ok());
                    ingredients.push(RecipeIngredientDetail {
                        id: (idx as i64) + 1,
                        ingredient_id: food_id,
                        ingredient_name: ing.food_name,
                        quantity,
                        unit: ing.measurement_description,
                        quantity_grams: None,
                        notes: Some(ing.ingredient_description),
                        display_order: idx as i32,
                    });
                }
            }
        }

        // Map steps
        let mut steps = Vec::new();
        if let Some(dirs_wrapper) = r.directions {
            if let Some(dir_list) = dirs_wrapper.direction {
                for dir in dir_list {
                    let step_number = dir.direction_number.parse::<i32>().unwrap_or(0);
                    steps.push(RecipeStepDetail {
                        id: step_number as i64,
                        step_number,
                        instruction: dir.direction_description,
                        duration_min: None,
                        image_url: None,
                        tip: None,
                    });
                }
            }
        }

        // Map primary image if available
        let mut images = Vec::new();
        if let Some(img_url) = r.recipe_image {
            images.push(RecipeImageDetail {
                id: 1,
                url: img_url,
                image_type: Some("primary".to_string()),
                is_primary: true,
                width: None,
                height: None,
            });
        }

        // Map nutrition
        let mut nutrition = None;
        if let Some(nut) = r.recipe_nutrition {
            nutrition = Some(RecipeNutritionDetail {
                calories: nut.calories.as_deref().and_then(|s| Decimal::from_str(s).ok()),
                protein_g: nut.protein.as_deref().and_then(|s| Decimal::from_str(s).ok()),
                carbs_g: nut.carbohydrate.as_deref().and_then(|s| Decimal::from_str(s).ok()),
                fat_g: nut.fat.as_deref().and_then(|s| Decimal::from_str(s).ok()),
                fiber_g: nut.fiber.as_deref().and_then(|s| Decimal::from_str(s).ok()),
                sugar_g: nut.sugar.as_deref().and_then(|s| Decimal::from_str(s).ok()),
                sodium_mg: nut.sodium.as_deref().and_then(|s| Decimal::from_str(s).ok()),
                saturated_fat_g: nut.saturated_fat.as_deref().and_then(|s| Decimal::from_str(s).ok()),
                per_serving: true,
            });
        }

        Ok(RecipeDetail {
            id: recipe_id,
            name: r.recipe_name,
            slug,
            description: r.recipe_description,
            cuisine: None,
            category: None,
            difficulty: None,
            servings,
            prep_time_min,
            cook_time_min,
            total_time_min,
            is_vegetarian: false,
            is_vegan: false,
            is_gluten_free: false,
            is_dairy_free: false,
            is_nut_free: false,
            source_url: r.recipe_url,
            average_rating: None,
            rating_count: 0,
            ingredients,
            steps,
            images,
            nutrition,
        })
    }

    /// Get recipe by slug
    pub async fn get_recipe_by_slug(&self, slug: &str) -> Result<RecipeDetail, AppError> {
        let id_str = slug.split('-').last().ok_or(AppError::NotFound("Recipe".into()))?;
        let id = id_str.parse::<i64>().map_err(|_| AppError::NotFound("Recipe".into()))?;
        self.get_recipe(id).await
    }

    /// Create a recipe (stub - not supported in FatSecret)
    pub async fn create_recipe(
        &self,
        _req: CreateRecipeRequest,
    ) -> Result<serde_json::Value, AppError> {
        Err(AppError::Internal("Not supported in FatSecret catalog".into()))
    }

    /// Update a recipe by ID (stub - not supported in FatSecret)
    pub async fn update_recipe(
        &self,
        _recipe_id: i64,
        _req: UpdateRecipeRequest,
    ) -> Result<serde_json::Value, AppError> {
        Err(AppError::Internal("Not supported in FatSecret catalog".into()))
    }

    /// Delete a recipe by ID (stub - not supported in FatSecret)
    pub async fn delete_recipe(&self, _recipe_id: i64) -> Result<(), AppError> {
        Err(AppError::Internal("Not supported in FatSecret catalog".into()))
    }
}
