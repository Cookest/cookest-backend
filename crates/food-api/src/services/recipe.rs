//! Recipe service — supports Local (SeaORM), FatSecret, and Hybrid data sources

use std::sync::Arc;
use std::str::FromStr;
use rust_decimal::Decimal;

use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, IntoActiveModel,
    ModelTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect,
};

use crate::config::FoodDataSource;
use crate::errors::AppError;
use crate::models::recipe::*;
use crate::services::FatSecretClient;
use crate::entity::{recipe, recipe_ingredient, recipe_step, recipe_image, recipe_nutrition};
use crate::entity::recipe::Entity as RecipeEntity;
use crate::entity::ingredient::Entity as IngEntity;

pub struct RecipeService {
    db: sea_orm::DatabaseConnection,
    source: FoodDataSource,
    fs_client: Option<Arc<FatSecretClient>>,
}

impl RecipeService {
    pub fn new(
        db: sea_orm::DatabaseConnection,
        source: FoodDataSource,
        fs_client: Option<Arc<FatSecretClient>>,
    ) -> Self {
        Self { db, source, fs_client }
    }

    // -------------------------------------------------------------------------
    // list_recipes
    // -------------------------------------------------------------------------

    /// List recipes with filters and pagination
    pub async fn list_recipes(
        &self,
        query: RecipeQuery,
    ) -> Result<PaginatedResponse<RecipeListItem>, AppError> {
        let page = query.page.unwrap_or(1).max(1);
        let per_page = query.per_page.unwrap_or(20).min(50);

        match &self.source {
            FoodDataSource::Local => {
                self.list_recipes_local(&query, page, per_page).await
            }
            FoodDataSource::FatSecret => {
                self.list_recipes_fatsecret(&query, page, per_page).await
            }
            FoodDataSource::Hybrid => {
                let local = self.list_recipes_local(&query, page, per_page).await?;
                if local.total > 0 {
                    Ok(local)
                } else {
                    self.list_recipes_fatsecret(&query, page, per_page).await
                }
            }
        }
    }

    async fn list_recipes_local(
        &self,
        query: &RecipeQuery,
        page: u64,
        per_page: u64,
    ) -> Result<PaginatedResponse<RecipeListItem>, AppError> {
        use crate::entity::recipe::Column as RecipeCol;

        let mut q = RecipeEntity::find();

        if let Some(search) = query.q.as_deref() {
            if !search.is_empty() {
                q = q.filter(
                    sea_orm::Condition::any()
                        .add(RecipeCol::Name.contains(search))
                );
            }
        }

        if let Some(cuisine) = query.cuisine.as_deref() {
            q = q.filter(RecipeCol::Cuisine.eq(cuisine));
        }
        if let Some(category) = query.category.as_deref() {
            q = q.filter(RecipeCol::Category.eq(category));
        }
        if let Some(difficulty) = query.difficulty.as_deref() {
            q = q.filter(RecipeCol::Difficulty.eq(difficulty));
        }
        if let Some(true) = query.vegetarian {
            q = q.filter(RecipeCol::IsVegetarian.eq(true));
        }
        if let Some(true) = query.vegan {
            q = q.filter(RecipeCol::IsVegan.eq(true));
        }
        if let Some(true) = query.gluten_free {
            q = q.filter(RecipeCol::IsGlutenFree.eq(true));
        }
        if let Some(true) = query.dairy_free {
            q = q.filter(RecipeCol::IsDairyFree.eq(true));
        }
        if let Some(max_time) = query.max_time {
            q = q.filter(RecipeCol::TotalTimeMin.lte(max_time));
        }

        let total = q.clone().count(&self.db).await?;
        let offset = ((page - 1) * per_page) as u64;

        let results = q
            .order_by_asc(RecipeCol::Name)
            .limit(per_page)
            .offset(offset)
            .all(&self.db)
            .await?;

        // For each recipe, find its primary image
        let recipe_ids: Vec<i64> = results.iter().map(|r| r.id).collect();
        let images = recipe_image::Entity::find()
            .filter(recipe_image::Column::RecipeId.is_in(recipe_ids))
            .filter(recipe_image::Column::IsPrimary.eq(true))
            .all(&self.db)
            .await?;
        let primary_images: std::collections::HashMap<i64, String> = images
            .into_iter()
            .map(|img| (img.recipe_id, img.url))
            .collect();

        let items: Vec<RecipeListItem> = results
            .into_iter()
            .map(|r| {
                let primary_image_url = primary_images.get(&r.id).cloned();
                RecipeListItem {
                    id: r.id,
                    name: r.name,
                    slug: r.slug,
                    cuisine: r.cuisine,
                    category: r.category,
                    difficulty: r.difficulty,
                    servings: r.servings,
                    total_time_min: r.total_time_min,
                    is_vegetarian: r.is_vegetarian,
                    is_vegan: r.is_vegan,
                    is_gluten_free: r.is_gluten_free,
                    is_dairy_free: r.is_dairy_free,
                    average_rating: r.average_rating,
                    rating_count: r.rating_count,
                    primary_image_url,
                }
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

    async fn list_recipes_fatsecret(
        &self,
        query: &RecipeQuery,
        page: u64,
        per_page: u64,
    ) -> Result<PaginatedResponse<RecipeListItem>, AppError> {
        let fs = self.fs_client.as_ref().ok_or_else(|| {
            AppError::Internal("FatSecret client not configured".to_string())
        })?;

        let fs_res = fs
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

    // -------------------------------------------------------------------------
    // get_recipe
    // -------------------------------------------------------------------------

    /// Get full recipe detail by ID
    pub async fn get_recipe(&self, id: i64) -> Result<RecipeDetail, AppError> {
        match &self.source {
            FoodDataSource::Local => self.get_recipe_local(id).await,
            FoodDataSource::FatSecret => self.get_recipe_fatsecret(id).await,
            FoodDataSource::Hybrid => {
                match self.get_recipe_local(id).await {
                    Ok(detail) => Ok(detail),
                    Err(AppError::NotFound(_)) => self.get_recipe_fatsecret(id).await,
                    Err(e) => Err(e),
                }
            }
        }
    }

    async fn get_recipe_local(&self, id: i64) -> Result<RecipeDetail, AppError> {
        let r = RecipeEntity::find_by_id(id)
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Recipe {}", id)))?;

        let steps = recipe_step::Entity::find()
            .filter(recipe_step::Column::RecipeId.eq(id))
            .order_by_asc(recipe_step::Column::StepNumber)
            .all(&self.db)
            .await?;

        let ingredients = recipe_ingredient::Entity::find()
            .filter(recipe_ingredient::Column::RecipeId.eq(id))
            .order_by_asc(recipe_ingredient::Column::DisplayOrder)
            .all(&self.db)
            .await?;

        let ing_ids: Vec<i64> = ingredients.iter().map(|i| i.ingredient_id).collect();
        let ing_names: std::collections::HashMap<i64, String> = IngEntity::find()
            .filter(crate::entity::ingredient::Column::Id.is_in(ing_ids))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|i| (i.id, i.name))
            .collect();

        let images = recipe_image::Entity::find()
            .filter(recipe_image::Column::RecipeId.eq(id))
            .all(&self.db)
            .await?;

        let nutrition = recipe_nutrition::Entity::find()
            .filter(recipe_nutrition::Column::RecipeId.eq(id))
            .one(&self.db)
            .await?;

        let mapped_steps: Vec<RecipeStepDetail> = steps
            .into_iter()
            .map(|s| RecipeStepDetail {
                id: s.id,
                step_number: s.step_number,
                instruction: s.instruction,
                duration_min: s.duration_min,
                image_url: s.image_url,
                tip: s.tip,
            })
            .collect();

        let mapped_ingredients: Vec<RecipeIngredientDetail> = ingredients
            .into_iter()
            .map(|i| {
                let name = ing_names.get(&i.ingredient_id).cloned().unwrap_or_default();
                RecipeIngredientDetail {
                    id: i.id,
                    ingredient_id: i.ingredient_id,
                    ingredient_name: name,
                    quantity: i.quantity,
                    unit: i.unit,
                    quantity_grams: i.quantity_grams,
                    notes: i.notes,
                    display_order: i.display_order,
                }
            })
            .collect();

        let mapped_images: Vec<RecipeImageDetail> = images
            .into_iter()
            .map(|img| RecipeImageDetail {
                id: img.id,
                url: img.url,
                image_type: img.image_type,
                is_primary: img.is_primary,
                width: img.width,
                height: img.height,
            })
            .collect();

        let mapped_nutrition = nutrition.map(|n| RecipeNutritionDetail {
            calories: n.calories,
            protein_g: n.protein_g,
            carbs_g: n.carbs_g,
            fat_g: n.fat_g,
            fiber_g: n.fiber_g,
            sugar_g: n.sugar_g,
            sodium_mg: n.sodium_mg,
            saturated_fat_g: n.saturated_fat_g,
            per_serving: n.per_serving,
        });

        Ok(RecipeDetail {
            id: r.id,
            name: r.name,
            slug: r.slug,
            description: r.description,
            cuisine: r.cuisine,
            category: r.category,
            difficulty: r.difficulty,
            servings: r.servings,
            prep_time_min: r.prep_time_min,
            cook_time_min: r.cook_time_min,
            total_time_min: r.total_time_min,
            is_vegetarian: r.is_vegetarian,
            is_vegan: r.is_vegan,
            is_gluten_free: r.is_gluten_free,
            is_dairy_free: r.is_dairy_free,
            is_nut_free: r.is_nut_free,
            source_url: r.source_url,
            average_rating: r.average_rating,
            rating_count: r.rating_count,
            ingredients: mapped_ingredients,
            steps: mapped_steps,
            images: mapped_images,
            nutrition: mapped_nutrition,
        })
    }

    async fn get_recipe_fatsecret(&self, id: i64) -> Result<RecipeDetail, AppError> {
        let fs = self.fs_client.as_ref().ok_or_else(|| {
            AppError::Internal("FatSecret client not configured".to_string())
        })?;

        let fs_res = fs
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

    // -------------------------------------------------------------------------
    // get_recipe_by_slug
    // -------------------------------------------------------------------------

    /// Get recipe by slug
    pub async fn get_recipe_by_slug(&self, slug_str: &str) -> Result<RecipeDetail, AppError> {
        match &self.source {
            FoodDataSource::Local | FoodDataSource::Hybrid => {
                use crate::entity::recipe::Column as RecipeCol;
                let r = RecipeEntity::find()
                    .filter(RecipeCol::Slug.eq(slug_str))
                    .one(&self.db)
                    .await?;
                if let Some(model) = r {
                    return self.get_recipe_local(model.id).await;
                }
                // For Hybrid, fall through to FatSecret id-based slug parsing
                if matches!(self.source, FoodDataSource::Hybrid) {
                    let id_str = slug_str.split('-').last()
                        .ok_or_else(|| AppError::NotFound("Recipe".into()))?;
                    let id = id_str.parse::<i64>()
                        .map_err(|_| AppError::NotFound("Recipe".into()))?;
                    return self.get_recipe_fatsecret(id).await;
                }
                Err(AppError::NotFound(format!("Recipe slug {}", slug_str)))
            }
            FoodDataSource::FatSecret => {
                let id_str = slug_str.split('-').last()
                    .ok_or_else(|| AppError::NotFound("Recipe".into()))?;
                let id = id_str.parse::<i64>()
                    .map_err(|_| AppError::NotFound("Recipe".into()))?;
                self.get_recipe_fatsecret(id).await
            }
        }
    }

    // -------------------------------------------------------------------------
    // create_recipe
    // -------------------------------------------------------------------------

    /// Create a recipe
    pub async fn create_recipe(
        &self,
        req: CreateRecipeRequest,
    ) -> Result<serde_json::Value, AppError> {
        match &self.source {
            FoodDataSource::FatSecret => {
                Err(AppError::Internal("Not supported in FatSecret catalog".into()))
            }
            FoodDataSource::Local | FoodDataSource::Hybrid => {
                use crate::services::time_region::{estimate_time, classify_region};

                let servings = req.servings.unwrap_or(2);
                let name_slug = slug::slugify(&req.name);
                // Make slug unique by appending a timestamp
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                let unique_slug = format!("{}-{}", name_slug, ts);

                // Infer time if not provided
                let (prep_time_min, cook_time_min, total_time_min) =
                    if req.prep_time_min.is_none() || req.cook_time_min.is_none() {
                        let est = estimate_time(
                            &[],
                            0,
                            0,
                            req.category.as_deref(),
                        );
                        let prep = req.prep_time_min.unwrap_or(est.prep_time_min);
                        let cook = req.cook_time_min.unwrap_or(est.cook_time_min);
                        let total = prep + cook;
                        (Some(prep), Some(cook), Some(total))
                    } else {
                        let prep = req.prep_time_min;
                        let cook = req.cook_time_min;
                        let total = prep.zip(cook).map(|(p, c)| p + c);
                        (prep, cook, total)
                    };

                // Infer cuisine via region classifier if not provided
                let cuisine = req.cuisine.or_else(|| {
                    Some(classify_region(&[], &[]))
                });

                let now: sea_orm::prelude::DateTimeWithTimeZone =
                    chrono::Utc::now().into();

                let active_model = recipe::ActiveModel {
                    id: ActiveValue::NotSet,
                    name: ActiveValue::Set(req.name),
                    slug: ActiveValue::Set(unique_slug),
                    description: ActiveValue::Set(req.description),
                    cuisine: ActiveValue::Set(cuisine),
                    category: ActiveValue::Set(req.category),
                    difficulty: ActiveValue::Set(req.difficulty),
                    servings: ActiveValue::Set(servings),
                    prep_time_min: ActiveValue::Set(prep_time_min),
                    cook_time_min: ActiveValue::Set(cook_time_min),
                    total_time_min: ActiveValue::Set(total_time_min),
                    is_vegetarian: ActiveValue::Set(req.is_vegetarian.unwrap_or(false)),
                    is_vegan: ActiveValue::Set(req.is_vegan.unwrap_or(false)),
                    is_gluten_free: ActiveValue::Set(req.is_gluten_free.unwrap_or(false)),
                    is_dairy_free: ActiveValue::Set(req.is_dairy_free.unwrap_or(false)),
                    is_nut_free: ActiveValue::Set(req.is_nut_free.unwrap_or(false)),
                    source_url: ActiveValue::Set(None),
                    average_rating: ActiveValue::Set(None),
                    rating_count: ActiveValue::Set(0),
                    author_id: ActiveValue::Set(None),
                    is_public: ActiveValue::Set(req.is_public.unwrap_or(true)),
                    fs_recipe_id: ActiveValue::Set(None),
                    created_at: ActiveValue::Set(now),
                    updated_at: ActiveValue::Set(now),
                };

                let inserted = active_model.insert(&self.db).await?;
                Ok(serde_json::json!({ "id": inserted.id, "slug": inserted.slug }))
            }
        }
    }

    // -------------------------------------------------------------------------
    // update_recipe
    // -------------------------------------------------------------------------

    /// Update a recipe by ID
    pub async fn update_recipe(
        &self,
        recipe_id: i64,
        req: UpdateRecipeRequest,
    ) -> Result<serde_json::Value, AppError> {
        match &self.source {
            FoodDataSource::FatSecret => {
                Err(AppError::Internal("Not supported in FatSecret catalog".into()))
            }
            FoodDataSource::Local | FoodDataSource::Hybrid => {
                let existing = RecipeEntity::find_by_id(recipe_id)
                    .one(&self.db)
                    .await?
                    .ok_or_else(|| AppError::NotFound(format!("Recipe {}", recipe_id)))?;

                let mut am = existing.into_active_model();

                if let Some(name) = req.name {
                    am.name = ActiveValue::Set(name);
                }
                if let Some(desc) = req.description {
                    am.description = ActiveValue::Set(Some(desc));
                }
                if let Some(cuisine) = req.cuisine {
                    am.cuisine = ActiveValue::Set(Some(cuisine));
                }
                if let Some(category) = req.category {
                    am.category = ActiveValue::Set(Some(category));
                }
                if let Some(difficulty) = req.difficulty {
                    am.difficulty = ActiveValue::Set(Some(difficulty));
                }
                if let Some(servings) = req.servings {
                    am.servings = ActiveValue::Set(servings);
                }
                if let Some(prep) = req.prep_time_min {
                    am.prep_time_min = ActiveValue::Set(Some(prep));
                }
                if let Some(cook) = req.cook_time_min {
                    am.cook_time_min = ActiveValue::Set(Some(cook));
                }
                if let Some(is_veg) = req.is_vegetarian {
                    am.is_vegetarian = ActiveValue::Set(is_veg);
                }
                if let Some(is_vegan) = req.is_vegan {
                    am.is_vegan = ActiveValue::Set(is_vegan);
                }
                if let Some(is_gf) = req.is_gluten_free {
                    am.is_gluten_free = ActiveValue::Set(is_gf);
                }
                if let Some(is_df) = req.is_dairy_free {
                    am.is_dairy_free = ActiveValue::Set(is_df);
                }
                if let Some(is_nf) = req.is_nut_free {
                    am.is_nut_free = ActiveValue::Set(is_nf);
                }
                if let Some(is_pub) = req.is_public {
                    am.is_public = ActiveValue::Set(is_pub);
                }

                let now: sea_orm::prelude::DateTimeWithTimeZone =
                    chrono::Utc::now().into();
                am.updated_at = ActiveValue::Set(now);

                let updated = am.update(&self.db).await?;
                Ok(serde_json::json!({ "id": updated.id, "slug": updated.slug }))
            }
        }
    }

    // -------------------------------------------------------------------------
    // delete_recipe
    // -------------------------------------------------------------------------

    /// Delete a recipe by ID
    pub async fn delete_recipe(&self, recipe_id: i64) -> Result<(), AppError> {
        match &self.source {
            FoodDataSource::FatSecret => {
                Err(AppError::Internal("Not supported in FatSecret catalog".into()))
            }
            FoodDataSource::Local | FoodDataSource::Hybrid => {
                let existing = RecipeEntity::find_by_id(recipe_id)
                    .one(&self.db)
                    .await?
                    .ok_or_else(|| AppError::NotFound(format!("Recipe {}", recipe_id)))?;

                existing.delete(&self.db).await?;
                Ok(())
            }
        }
    }
}
