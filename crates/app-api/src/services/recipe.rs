//! Recipe service — queries recipes with filtering, pagination, and full detail loads

use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    PaginatorTrait, Condition, ActiveModelTrait, Set,
};
use uuid::Uuid;
use chrono::Utc;

use crate::entity::{
    recipe, recipe_ingredient, recipe_step, recipe_image, recipe_nutrition, ingredient,
    inventory_item,
};
use cookest_shared::errors::AppError;
use crate::models::recipe::*;

pub struct RecipeService {
    db: DatabaseConnection,
}

impl RecipeService {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// List recipes with filters and pagination
    pub async fn list_recipes(
        &self,
        query: RecipeQuery,
    ) -> Result<PaginatedResponse<RecipeListItem>, AppError> {
        let page = query.page.unwrap_or(1).max(1);
        let per_page = query.per_page.unwrap_or(20).min(50);

        let mut condition = Condition::all();

        // Dietary filters
        if query.vegetarian == Some(true) {
            condition = condition.add(recipe::Column::IsVegetarian.eq(true));
        }
        if query.vegan == Some(true) {
            condition = condition.add(recipe::Column::IsVegan.eq(true));
        }
        if query.gluten_free == Some(true) {
            condition = condition.add(recipe::Column::IsGlutenFree.eq(true));
        }
        if query.dairy_free == Some(true) {
            condition = condition.add(recipe::Column::IsDairyFree.eq(true));
        }

        // Text filters
        if let Some(cuisine) = &query.cuisine {
            condition = condition.add(recipe::Column::Cuisine.eq(cuisine));
        }
        if let Some(category) = &query.category {
            condition = condition.add(recipe::Column::Category.eq(category));
        }
        if let Some(difficulty) = &query.difficulty {
            condition = condition.add(recipe::Column::Difficulty.eq(difficulty));
        }
        if let Some(max_time) = query.max_time {
            condition = condition.add(recipe::Column::TotalTimeMin.lte(max_time));
        }

        // Full-text search on name using ILIKE (pg_trgm handles performance)
        if let Some(ref q) = query.q {
            let pattern = format!("%{}%", q);
            condition = condition.add(recipe::Column::Name.like(pattern));
        }

        let paginator = recipe::Entity::find()
            .filter(condition)
            .order_by_asc(recipe::Column::Name)
            .paginate(&self.db, per_page);

        let total = paginator.num_items().await?;
        let recipes = paginator.fetch_page(page - 1).await?;

        // Fetch primary images for each recipe
        let recipe_ids: Vec<i64> = recipes.iter().map(|r| r.id).collect();
        let images = recipe_image::Entity::find()
            .filter(recipe_image::Column::RecipeId.is_in(recipe_ids))
            .filter(recipe_image::Column::IsPrimary.eq(true))
            .all(&self.db)
            .await?;

        let items = recipes
            .into_iter()
            .map(|r| {
                let primary_image = images
                    .iter()
                    .find(|img| img.recipe_id == r.id)
                    .map(|img| img.url.clone());

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
                    primary_image_url: primary_image,
                    match_pct: None,
                    owned_ingredients: None,
                    total_ingredients: None,
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

    /// Get full recipe detail by ID
    pub async fn get_recipe(&self, id: i64) -> Result<RecipeDetail, AppError> {
        let recipe = recipe::Entity::find_by_id(id)
            .one(&self.db)
            .await?
            .ok_or(AppError::NotFound("Recipe".into()))?;

        // Load ingredients with ingredient names
        let raw_ingredients = recipe_ingredient::Entity::find()
            .filter(recipe_ingredient::Column::RecipeId.eq(id))
            .order_by_asc(recipe_ingredient::Column::DisplayOrder)
            .all(&self.db)
            .await?;

        let ingredient_ids: Vec<i64> = raw_ingredients.iter().map(|i| i.ingredient_id).collect();
        let ingredients_map = ingredient::Entity::find()
            .filter(ingredient::Column::Id.is_in(ingredient_ids))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|ing| (ing.id, ing.name))
            .collect::<std::collections::HashMap<_, _>>();

        let ingredients = raw_ingredients
            .into_iter()
            .map(|ri| RecipeIngredientDetail {
                id: ri.id,
                ingredient_id: ri.ingredient_id,
                ingredient_name: ingredients_map
                    .get(&ri.ingredient_id)
                    .cloned()
                    .unwrap_or_default(),
                quantity: ri.quantity,
                unit: ri.unit,
                quantity_grams: ri.quantity_grams,
                notes: ri.notes,
                display_order: ri.display_order,
            })
            .collect();

        // Load steps
        let steps = recipe_step::Entity::find()
            .filter(recipe_step::Column::RecipeId.eq(id))
            .order_by_asc(recipe_step::Column::StepNumber)
            .all(&self.db)
            .await?
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

        // Load images
        let images = recipe_image::Entity::find()
            .filter(recipe_image::Column::RecipeId.eq(id))
            .all(&self.db)
            .await?
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

        // Load nutrition
        let nutrition = recipe_nutrition::Entity::find()
            .filter(recipe_nutrition::Column::RecipeId.eq(id))
            .one(&self.db)
            .await?
            .map(|n| RecipeNutritionDetail {
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
            id: recipe.id,
            name: recipe.name,
            slug: recipe.slug,
            description: recipe.description,
            cuisine: recipe.cuisine,
            category: recipe.category,
            difficulty: recipe.difficulty,
            servings: recipe.servings,
            prep_time_min: recipe.prep_time_min,
            cook_time_min: recipe.cook_time_min,
            total_time_min: recipe.total_time_min,
            is_vegetarian: recipe.is_vegetarian,
            is_vegan: recipe.is_vegan,
            is_gluten_free: recipe.is_gluten_free,
            is_dairy_free: recipe.is_dairy_free,
            is_nut_free: recipe.is_nut_free,
            source_url: recipe.source_url,
            average_rating: recipe.average_rating,
            rating_count: recipe.rating_count,
            ingredients,
            steps,
            images,
            nutrition,
        })
    }

    /// Get recipe by slug
    pub async fn get_recipe_by_slug(&self, slug: &str) -> Result<RecipeDetail, AppError> {
        let recipe = recipe::Entity::find()
            .filter(recipe::Column::Slug.eq(slug))
            .one(&self.db)
            .await?
            .ok_or(AppError::NotFound("Recipe".into()))?;

        self.get_recipe(recipe.id).await
    }

    /// List recipes with inventory match percentage for authenticated user.
    /// Adds match_pct, owned_ingredients, total_ingredients to each item.
    pub async fn list_recipes_with_inventory(
        &self,
        user_id: Uuid,
        query: RecipeQuery,
    ) -> Result<PaginatedResponse<RecipeListItem>, AppError> {
        // Load user inventory as a set of ingredient_ids
        let user_ingredient_ids: std::collections::HashSet<i64> =
            inventory_item::Entity::find()
                .filter(inventory_item::Column::UserId.eq(user_id))
                .all(&self.db)
                .await?
                .into_iter()
                .map(|i| i.ingredient_id)
                .collect();

        let mut result = self.list_recipes(query).await?;

        // For each recipe in the page, compute match_pct
        let recipe_ids: Vec<i64> = result.data.iter().map(|r| r.id).collect();
        let ingredients = recipe_ingredient::Entity::find()
            .filter(recipe_ingredient::Column::RecipeId.is_in(recipe_ids))
            .all(&self.db)
            .await?;

        let mut counts: std::collections::HashMap<i64, (usize, usize)> =
            std::collections::HashMap::new();
        for ri in &ingredients {
            let entry = counts.entry(ri.recipe_id).or_default();
            entry.1 += 1;
            if user_ingredient_ids.contains(&ri.ingredient_id) {
                entry.0 += 1;
            }
        }

        for item in &mut result.data {
            if let Some((owned, total)) = counts.get(&item.id) {
                let pct = if *total == 0 {
                    0.0
                } else {
                    (*owned as f64 / *total as f64 * 100.0).round()
                };
                item.match_pct = Some(pct);
                item.owned_ingredients = Some(*owned);
                item.total_ingredients = Some(*total);
            }
        }

        // Sort by match_pct descending (best matches first)
        result.data.sort_by(|a, b| {
            b.match_pct
                .unwrap_or(0.0)
                .partial_cmp(&a.match_pct.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(result)
    }

    /// Create a recipe (Pro tier users only — enforced in handler)
    pub async fn create_recipe(
        &self,
        user_id: Uuid,
        req: CreateRecipeRequest,
    ) -> Result<serde_json::Value, AppError> {
        use slug::slugify;
        let now = Utc::now().fixed_offset();
        let base_slug = slugify(&req.name);
        // Ensure unique slug by appending a short random suffix if needed
        let slug = format!("{}-{}", base_slug, &uuid::Uuid::new_v4().to_string()[..8]);

        let model = recipe::ActiveModel {
            name: Set(req.name.clone()),
            slug: Set(slug.clone()),
            description: Set(req.description),
            cuisine: Set(req.cuisine),
            category: Set(req.category),
            difficulty: Set(req.difficulty),
            servings: Set(req.servings.unwrap_or(2)),
            prep_time_min: Set(req.prep_time_min),
            cook_time_min: Set(req.cook_time_min),
            total_time_min: Set(req.prep_time_min.zip(req.cook_time_min).map(|(p, c)| p + c)),
            is_vegetarian: Set(req.is_vegetarian.unwrap_or(false)),
            is_vegan: Set(req.is_vegan.unwrap_or(false)),
            is_gluten_free: Set(req.is_gluten_free.unwrap_or(false)),
            is_dairy_free: Set(req.is_dairy_free.unwrap_or(false)),
            is_nut_free: Set(req.is_nut_free.unwrap_or(false)),
            author_id: Set(Some(user_id)),
            is_public: Set(req.is_public.unwrap_or(true)),
            rating_count: Set(0),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };

        let saved = model.insert(&self.db).await?;

        Ok(serde_json::json!({
            "id": saved.id,
            "slug": saved.slug,
            "name": saved.name,
            "is_public": saved.is_public,
            "author_id": saved.author_id,
            "message": "Recipe created. Use POST /api/recipes/:id/ingredients and /steps to add content."
        }))
    }

    /// Update a user's own recipe (author only)
    pub async fn update_recipe(
        &self,
        user_id: Uuid,
        recipe_id: i64,
        req: UpdateRecipeRequest,
    ) -> Result<serde_json::Value, AppError> {
        let existing = recipe::Entity::find_by_id(recipe_id)
            .one(&self.db)
            .await?
            .ok_or(AppError::NotFound("Recipe".into()))?;

        if existing.author_id != Some(user_id) {
            return Err(AppError::Forbidden);
        }

        let now = Utc::now().fixed_offset();
        let mut model: recipe::ActiveModel = existing.into();

        if let Some(name) = req.name { model.name = Set(name); }
        if let Some(desc) = req.description { model.description = Set(Some(desc)); }
        if let Some(c) = req.cuisine { model.cuisine = Set(Some(c)); }
        if let Some(c) = req.category { model.category = Set(Some(c)); }
        if let Some(d) = req.difficulty { model.difficulty = Set(Some(d)); }
        if let Some(s) = req.servings { model.servings = Set(s); }
        if let Some(p) = req.prep_time_min { model.prep_time_min = Set(Some(p)); }
        if let Some(c) = req.cook_time_min { model.cook_time_min = Set(Some(c)); }
        if let Some(v) = req.is_vegetarian { model.is_vegetarian = Set(v); }
        if let Some(v) = req.is_vegan { model.is_vegan = Set(v); }
        if let Some(v) = req.is_gluten_free { model.is_gluten_free = Set(v); }
        if let Some(v) = req.is_dairy_free { model.is_dairy_free = Set(v); }
        if let Some(v) = req.is_nut_free { model.is_nut_free = Set(v); }
        if let Some(p) = req.is_public { model.is_public = Set(p); }
        model.updated_at = Set(now);

        let saved = model.update(&self.db).await?;

        Ok(serde_json::json!({
            "id": saved.id,
            "slug": saved.slug,
            "name": saved.name,
            "is_public": saved.is_public,
        }))
    }

    /// Delete a user's own recipe (author only)
    pub async fn delete_recipe(&self, user_id: Uuid, recipe_id: i64) -> Result<(), AppError> {
        let existing = recipe::Entity::find_by_id(recipe_id)
            .one(&self.db)
            .await?
            .ok_or(AppError::NotFound("Recipe".into()))?;

        if existing.author_id != Some(user_id) {
            return Err(AppError::Forbidden);
        }

        recipe::Entity::delete_by_id(recipe_id)
            .exec(&self.db)
            .await?;

        Ok(())
    }

    /// List recipes created by this user
    pub async fn list_my_recipes(
        &self,
        user_id: Uuid,
        page: u64,
        per_page: u64,
    ) -> Result<PaginatedResponse<RecipeListItem>, AppError> {
        let per_page = per_page.min(50);
        let paginator = recipe::Entity::find()
            .filter(recipe::Column::AuthorId.eq(user_id))
            .order_by_desc(recipe::Column::CreatedAt)
            .paginate(&self.db, per_page);

        let total = paginator.num_items().await?;
        let recipes = paginator.fetch_page(page.saturating_sub(1)).await?;

        let recipe_ids: Vec<i64> = recipes.iter().map(|r| r.id).collect();
        let images = recipe_image::Entity::find()
            .filter(recipe_image::Column::RecipeId.is_in(recipe_ids))
            .filter(recipe_image::Column::IsPrimary.eq(true))
            .all(&self.db)
            .await?;

        let items = recipes
            .into_iter()
            .map(|r| {
                let primary_image = images
                    .iter()
                    .find(|img| img.recipe_id == r.id)
                    .map(|img| img.url.clone());

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
                    primary_image_url: primary_image,
                    match_pct: None,
                    owned_ingredients: None,
                    total_ingredients: None,
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
}
