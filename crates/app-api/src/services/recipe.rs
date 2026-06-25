//! Recipe service — queries recipes with filtering, pagination, and full detail loads

use chrono::Utc;
use sea_orm::sea_query::{extension::postgres::PgExpr, Expr};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, Set, TransactionTrait,
};
use uuid::Uuid;

use crate::entity::{
    ingredient, inventory_item, recipe, recipe_image, recipe_ingredient, recipe_nutrition,
    recipe_step,
};
use crate::handlers::browse::FoodApiClient;
use crate::models::recipe::*;
use crate::services::IngredientService;
use cookest_shared::errors::AppError;

pub struct RecipeService {
    db: DatabaseConnection,
    food_api_client: FoodApiClient,
    s3_client: Option<aws_sdk_s3::Client>,
    s3_bucket: Option<String>,
    s3_public_url: Option<String>,
}

impl RecipeService {
    pub fn new(db: DatabaseConnection, food_api_client: FoodApiClient) -> Self {
        Self {
            db,
            food_api_client,
            s3_client: None,
            s3_bucket: None,
            s3_public_url: None,
        }
    }

    pub fn with_s3(
        mut self,
        s3_client: aws_sdk_s3::Client,
        s3_bucket: String,
        s3_public_url: Option<String>,
    ) -> Self {
        self.s3_client = Some(s3_client);
        self.s3_bucket = Some(s3_bucket);
        self.s3_public_url = s3_public_url;
        self
    }

    /// Ingredient resolver used to validate + mirror catalog ingredients.
    fn ingredient_service(&self) -> IngredientService {
        IngredientService::new(self.db.clone(), self.food_api_client.clone())
    }

    /// Validate that every referenced ingredient exists in the master catalog and
    /// is mirrored locally (so the recipe_ingredient FK resolves). Rejects unknown
    /// ids — recipes may only use preset catalog ingredients.
    async fn ensure_ingredients_mirrored(
        &self,
        ingredients: &[CreateRecipeIngredientRequest],
    ) -> Result<(), AppError> {
        let ing_svc = self.ingredient_service();
        for req_ing in ingredients {
            ing_svc.ensure_local_mirror(req_ing.ingredient_id).await?;
        }
        Ok(())
    }

    /// List recipes with filters and pagination
    pub async fn list_recipes(
        &self,
        user_id: Option<Uuid>,
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
            condition = condition.add(Expr::col(recipe::Column::Cuisine).ilike(cuisine));
        }
        if let Some(category) = &query.category {
            condition = condition.add(Expr::col(recipe::Column::Category).ilike(category));
        }
        if let Some(difficulty) = &query.difficulty {
            condition = condition.add(Expr::col(recipe::Column::Difficulty).ilike(difficulty));
        }
        if let Some(max_time) = query.max_time {
            condition = condition.add(recipe::Column::TotalTimeMin.lte(max_time));
        }

        // Full-text search on name using ILIKE (pg_trgm handles performance)
        if let Some(ref q) = query.q {
            let pattern = format!("%{}%", q);
            condition = condition.add(Expr::col(recipe::Column::Name).ilike(pattern));
        }

        // Show public recipes OR recipes owned by the current user
        if let Some(uid) = user_id {
            condition = condition.add(
                Condition::any()
                    .add(recipe::Column::IsPublic.eq(true))
                    .add(recipe::Column::AuthorId.eq(uid)),
            );
        } else {
            condition = condition.add(recipe::Column::IsPublic.eq(true));
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
        let local_exists = recipe::Entity::find_by_id(id)
            .one(&self.db)
            .await?
            .is_some();
        if !local_exists {
            let _ = self.get_recipe_or_import(id).await?;
        }

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
        let images: Vec<RecipeImageDetail> = recipe_image::Entity::find()
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

        let primary_image_url = images
            .iter()
            .find(|img| img.is_primary)
            .or_else(|| images.first())
            .map(|img| img.url.clone());

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
            primary_image_url,
            ingredients,
            steps,
            images,
            nutrition,
            author_id: recipe.author_id,
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
        let user_ingredient_ids: std::collections::HashSet<i64> = inventory_item::Entity::find()
            .filter(inventory_item::Column::UserId.eq(user_id))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|i| i.ingredient_id)
            .collect();

        let mut result = self.list_recipes(Some(user_id), query).await?;

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

        // Validate + mirror catalog ingredients up front (rejects unknown ids before
        // any recipe row is created).
        if let Some(ings) = &req.ingredients {
            self.ensure_ingredients_mirrored(ings).await?;
        }

        let model = recipe::ActiveModel {
            name: Set(req.name.clone()),
            slug: Set(slug.clone()),
            description: Set(req.description.clone()),
            cuisine: Set(req.cuisine.clone()),
            category: Set(req.category.clone()),
            difficulty: Set(req.difficulty.clone()),
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

        let saved = self
            .db
            .transaction::<_, recipe::Model, AppError>(|txn| {
                Box::pin(async move {
                    let saved_recipe = model.insert(txn).await?;

                    if let Some(ingredients) = req.ingredients {
                        for (i, req_ing) in ingredients.into_iter().enumerate() {
                            let ri_model = recipe_ingredient::ActiveModel {
                                recipe_id: Set(saved_recipe.id),
                                ingredient_id: Set(req_ing.ingredient_id),
                                quantity: Set(req_ing.quantity),
                                unit: Set(req_ing.unit),
                                notes: Set(req_ing.notes),
                                display_order: Set(i as i32),
                                ..Default::default()
                            };
                            ri_model.insert(txn).await?;
                        }
                    }

                    if let Some(steps) = req.steps {
                        for (i, req_step) in steps.into_iter().enumerate() {
                            let step_model = recipe_step::ActiveModel {
                                recipe_id: Set(saved_recipe.id),
                                step_number: Set((i + 1) as i32),
                                instruction: Set(req_step.instruction),
                                duration_min: Set(req_step.duration_min),
                                ..Default::default()
                            };
                            step_model.insert(txn).await?;
                        }
                    }

                    Ok(saved_recipe)
                })
            })
            .await
            .map_err(|e| match e {
                sea_orm::TransactionError::Connection(de) => AppError::from(de),
                sea_orm::TransactionError::Transaction(ae) => ae,
            })?;

        Ok(serde_json::json!({
            "id": saved.id,
            "slug": saved.slug,
            "name": saved.name,
            "is_public": saved.is_public,
            "author_id": saved.author_id,
            "message": "Recipe created successfully."
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

        // Validate + mirror catalog ingredients up front (rejects unknown ids).
        if let Some(ings) = &req.ingredients {
            self.ensure_ingredients_mirrored(ings).await?;
        }

        let now = Utc::now().fixed_offset();
        let mut model: recipe::ActiveModel = existing.into();

        if let Some(name) = &req.name {
            model.name = Set(name.clone());
        }
        if let Some(desc) = &req.description {
            model.description = Set(Some(desc.clone()));
        }
        if let Some(c) = &req.cuisine {
            model.cuisine = Set(Some(c.clone()));
        }
        if let Some(c) = &req.category {
            model.category = Set(Some(c.clone()));
        }
        if let Some(d) = &req.difficulty {
            model.difficulty = Set(Some(d.clone()));
        }
        if let Some(s) = req.servings {
            model.servings = Set(s);
        }
        if let Some(p) = req.prep_time_min {
            model.prep_time_min = Set(Some(p));
        }
        if let Some(c) = req.cook_time_min {
            model.cook_time_min = Set(Some(c));
        }
        if let Some(v) = req.is_vegetarian {
            model.is_vegetarian = Set(v);
        }
        if let Some(v) = req.is_vegan {
            model.is_vegan = Set(v);
        }
        if let Some(v) = req.is_gluten_free {
            model.is_gluten_free = Set(v);
        }
        if let Some(v) = req.is_dairy_free {
            model.is_dairy_free = Set(v);
        }
        if let Some(v) = req.is_nut_free {
            model.is_nut_free = Set(v);
        }
        if let Some(p) = req.is_public {
            model.is_public = Set(p);
        }
        model.updated_at = Set(now);

        let saved = self
            .db
            .transaction::<_, recipe::Model, AppError>(|txn| {
                Box::pin(async move {
                    let saved_recipe = model.update(txn).await?;

                    if let Some(ingredients) = req.ingredients {
                        // Delete existing ingredients
                        recipe_ingredient::Entity::delete_many()
                            .filter(recipe_ingredient::Column::RecipeId.eq(saved_recipe.id))
                            .exec(txn)
                            .await?;

                        for (i, req_ing) in ingredients.into_iter().enumerate() {
                            let ri_model = recipe_ingredient::ActiveModel {
                                recipe_id: Set(saved_recipe.id),
                                ingredient_id: Set(req_ing.ingredient_id),
                                quantity: Set(req_ing.quantity),
                                unit: Set(req_ing.unit),
                                notes: Set(req_ing.notes),
                                display_order: Set(i as i32),
                                ..Default::default()
                            };
                            ri_model.insert(txn).await?;
                        }
                    }

                    if let Some(steps) = req.steps {
                        // Delete existing steps
                        recipe_step::Entity::delete_many()
                            .filter(recipe_step::Column::RecipeId.eq(saved_recipe.id))
                            .exec(txn)
                            .await?;

                        for (i, req_step) in steps.into_iter().enumerate() {
                            let step_model = recipe_step::ActiveModel {
                                recipe_id: Set(saved_recipe.id),
                                step_number: Set((i + 1) as i32),
                                instruction: Set(req_step.instruction),
                                duration_min: Set(req_step.duration_min),
                                ..Default::default()
                            };
                            step_model.insert(txn).await?;
                        }
                    }

                    Ok(saved_recipe)
                })
            })
            .await
            .map_err(|e| match e {
                sea_orm::TransactionError::Connection(de) => AppError::from(de),
                sea_orm::TransactionError::Transaction(ae) => ae,
            })?;

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

    /// Upload an image for a recipe
    pub async fn upload_recipe_image(
        &self,
        user_id: Uuid,
        recipe_id: i64,
        image_bytes: Vec<u8>,
        file_ext: &str,
    ) -> Result<serde_json::Value, AppError> {
        let existing = recipe::Entity::find_by_id(recipe_id)
            .one(&self.db)
            .await?
            .ok_or(AppError::NotFound("Recipe".into()))?;

        if existing.author_id != Some(user_id) {
            return Err(AppError::Forbidden);
        }

        let file_name = format!("{}.{}", uuid::Uuid::new_v4(), file_ext);
        let object_key = format!("recipes/{}", file_name);

        let s3_client = self
            .s3_client
            .as_ref()
            .ok_or_else(|| AppError::Internal("S3 not configured".to_string()))?;
        let s3_bucket = self
            .s3_bucket
            .as_ref()
            .ok_or_else(|| AppError::Internal("S3 bucket not configured".to_string()))?;

        s3_client
            .put_object()
            .bucket(s3_bucket)
            .key(&object_key)
            .body(image_bytes.into())
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to upload image to S3: {}", e)))?;

        let url = if let Some(pub_url) = &self.s3_public_url {
            format!("{}/{}", pub_url, object_key)
        } else {
            // Fallback: assume the client handles it or it's mapped directly
            format!("/{}", object_key)
        };
        let url_clone = url.clone();

        self.db
            .transaction::<_, (), AppError>(|txn| {
                Box::pin(async move {
                    recipe_image::Entity::update_many()
                        .col_expr(recipe_image::Column::IsPrimary, Expr::val(false).into())
                        .filter(recipe_image::Column::RecipeId.eq(recipe_id))
                        .exec(txn)
                        .await?;

                    let img_model = recipe_image::ActiveModel {
                        recipe_id: Set(recipe_id),
                        url: Set(url_clone),
                        is_primary: Set(true),
                        ..Default::default()
                    };
                    img_model.insert(txn).await?;
                    Ok(())
                })
            })
            .await
            .map_err(|e| match e {
                sea_orm::TransactionError::Connection(de) => AppError::from(de),
                sea_orm::TransactionError::Transaction(ae) => ae,
            })?;

        Ok(serde_json::json!({
            "url": url,
            "message": "Image uploaded successfully"
        }))
    }

    /// List recipes created by this user
    pub async fn list_my_recipes(
        &self,
        user_id: Uuid,
        query: RecipeQuery,
    ) -> Result<PaginatedResponse<RecipeListItem>, AppError> {
        let page = query.page.unwrap_or(1).max(1);
        let per_page = query.per_page.unwrap_or(20).min(50);

        let mut condition = Condition::all().add(recipe::Column::AuthorId.eq(user_id));

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
            condition = condition.add(Expr::col(recipe::Column::Cuisine).ilike(cuisine));
        }
        if let Some(category) = &query.category {
            condition = condition.add(Expr::col(recipe::Column::Category).ilike(category));
        }
        if let Some(difficulty) = &query.difficulty {
            condition = condition.add(Expr::col(recipe::Column::Difficulty).ilike(difficulty));
        }

        if let Some(ref q) = query.q {
            let pattern = format!("%{}%", q);
            condition = condition.add(Expr::col(recipe::Column::Name).ilike(pattern));
        }

        let paginator = recipe::Entity::find()
            .filter(condition)
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

    /// Get full recipe detail by ID, caching/importing it locally from food-api if missing
    pub async fn get_recipe_or_import(&self, recipe_id: i64) -> Result<recipe::Model, AppError> {
        // 1. Try local lookup first
        let existing = recipe::Entity::find_by_id(recipe_id).one(&self.db).await?;

        if let Some(r) = existing {
            return Ok(r);
        }

        // 2. Fetch from food-api
        let path = format!("/api/v1/recipes/{}", recipe_id);
        let req = self.food_api_client.get(&path);
        let resp = req
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to reach food-api: {}", e)))?;

        let mut fs_recipe = resp.json::<RecipeDetail>().await.map_err(|e| {
            AppError::Internal(format!("Failed to parse recipe from food-api: {}", e))
        })?;

        // 3. Ensure all recipe ingredients are imported/cached locally in app-db first
        let ing_service = IngredientService::new(self.db.clone(), self.food_api_client.clone());
        let mut valid_ingredients = Vec::new();
        for ri in fs_recipe.ingredients {
            match ing_service.get_ingredient(ri.ingredient_id).await {
                Ok(_) => valid_ingredients.push(ri),
                Err(e) => {
                    tracing::warn!(
                        "Skipping ingredient {} for recipe {}: {}",
                        ri.ingredient_id,
                        fs_recipe.id,
                        e
                    );
                }
            }
        }
        fs_recipe.ingredients = valid_ingredients;

        // 4. Save recipe, steps, nutrition, and images locally in a transaction
        let txn_db = self.db.clone();
        let fs_recipe_clone = fs_recipe.clone();
        let now = Utc::now().fixed_offset();
        txn_db
            .transaction::<_, (), AppError>(move |txn| {
                Box::pin(async move {
                    if recipe::Entity::find_by_id(fs_recipe_clone.id)
                        .one(txn)
                        .await?
                        .is_none()
                    {
                        let rec_model = recipe::ActiveModel {
                            id: Set(fs_recipe_clone.id),
                            name: Set(fs_recipe_clone.name.clone()),
                            slug: Set(fs_recipe_clone.slug.clone()),
                            description: Set(fs_recipe_clone.description.clone()),
                            cuisine: Set(fs_recipe_clone.cuisine.clone()),
                            category: Set(fs_recipe_clone.category.clone()),
                            difficulty: Set(fs_recipe_clone.difficulty.clone()),
                            servings: Set(fs_recipe_clone.servings),
                            prep_time_min: Set(fs_recipe_clone.prep_time_min),
                            cook_time_min: Set(fs_recipe_clone.cook_time_min),
                            total_time_min: Set(fs_recipe_clone.total_time_min),
                            is_vegetarian: Set(fs_recipe_clone.is_vegetarian),
                            is_vegan: Set(fs_recipe_clone.is_vegan),
                            is_gluten_free: Set(fs_recipe_clone.is_gluten_free),
                            is_dairy_free: Set(fs_recipe_clone.is_dairy_free),
                            is_nut_free: Set(fs_recipe_clone.is_nut_free),
                            source_url: Set(fs_recipe_clone.source_url.clone()),
                            average_rating: Set(fs_recipe_clone.average_rating),
                            rating_count: Set(fs_recipe_clone.rating_count),
                            author_id: Set(None),
                            is_public: Set(true),
                            created_at: Set(now),
                            updated_at: Set(now),
                            ..Default::default()
                        };
                        rec_model.insert(txn).await?;

                        // Save ingredients
                        for ri in &fs_recipe_clone.ingredients {
                            let ri_model = recipe_ingredient::ActiveModel {
                                recipe_id: Set(fs_recipe_clone.id),
                                ingredient_id: Set(ri.ingredient_id),
                                quantity: Set(ri.quantity),
                                unit: Set(ri.unit.clone()),
                                quantity_grams: Set(ri.quantity_grams),
                                notes: Set(ri.notes.clone()),
                                display_order: Set(ri.display_order),
                                ..Default::default()
                            };
                            ri_model.insert(txn).await?;
                        }

                        // Save steps
                        for step in &fs_recipe_clone.steps {
                            let step_model = recipe_step::ActiveModel {
                                recipe_id: Set(fs_recipe_clone.id),
                                step_number: Set(step.step_number),
                                instruction: Set(step.instruction.clone()),
                                duration_min: Set(step.duration_min),
                                image_url: Set(step.image_url.clone()),
                                tip: Set(step.tip.clone()),
                                ..Default::default()
                            };
                            step_model.insert(txn).await?;
                        }

                        // Save images
                        for img in &fs_recipe_clone.images {
                            let img_model = recipe_image::ActiveModel {
                                recipe_id: Set(fs_recipe_clone.id),
                                url: Set(img.url.clone()),
                                image_type: Set(img.image_type.clone()),
                                is_primary: Set(img.is_primary),
                                width: Set(img.width),
                                height: Set(img.height),
                                ..Default::default()
                            };
                            img_model.insert(txn).await?;
                        }

                        // Save nutrition
                        if let Some(nut) = &fs_recipe_clone.nutrition {
                            let nut_model = recipe_nutrition::ActiveModel {
                                recipe_id: Set(fs_recipe_clone.id),
                                per_serving: Set(nut.per_serving),
                                calories: Set(nut.calories),
                                protein_g: Set(nut.protein_g),
                                carbs_g: Set(nut.carbs_g),
                                fat_g: Set(nut.fat_g),
                                fiber_g: Set(nut.fiber_g),
                                sugar_g: Set(nut.sugar_g),
                                sodium_mg: Set(nut.sodium_mg),
                                saturated_fat_g: Set(nut.saturated_fat_g),
                                calculated_at: Set(now),
                                ..Default::default()
                            };
                            nut_model.insert(txn).await?;
                        }
                    }
                    Ok(())
                })
            })
            .await
            .map_err(|e| match e {
                sea_orm::TransactionError::Connection(de) => AppError::from(de),
                sea_orm::TransactionError::Transaction(ae) => ae,
            })?;

        // Reload from local DB
        let r = recipe::Entity::find_by_id(recipe_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::Internal("Failed to load cached recipe model".into()))?;

        Ok(r)
    }

    /// Import a community recipe into the user's collection (as a private clone)
    pub async fn import_recipe(
        &self,
        user_id: Uuid,
        recipe_id: i64,
    ) -> Result<serde_json::Value, AppError> {
        // 1. Fetch source recipe details
        let source_recipe = self.get_recipe(recipe_id).await?;

        // 2. Generate slug
        use slug::slugify;
        let base_slug = slugify(&source_recipe.name);
        let slug = format!("{}-{}", base_slug, &uuid::Uuid::new_v4().to_string()[..8]);
        let now = Utc::now().fixed_offset();

        // 3. Start transaction
        let txn_db = self.db.clone();
        let saved = txn_db
            .transaction::<_, recipe::Model, AppError>(move |txn| {
                Box::pin(async move {
                    // Insert main recipe record
                    let rec_model = recipe::ActiveModel {
                        name: Set(source_recipe.name.clone()),
                        slug: Set(slug),
                        description: Set(source_recipe.description.clone()),
                        cuisine: Set(source_recipe.cuisine.clone()),
                        category: Set(source_recipe.category.clone()),
                        difficulty: Set(source_recipe.difficulty.clone()),
                        servings: Set(source_recipe.servings),
                        prep_time_min: Set(source_recipe.prep_time_min),
                        cook_time_min: Set(source_recipe.cook_time_min),
                        total_time_min: Set(source_recipe.total_time_min),
                        is_vegetarian: Set(source_recipe.is_vegetarian),
                        is_vegan: Set(source_recipe.is_vegan),
                        is_gluten_free: Set(source_recipe.is_gluten_free),
                        is_dairy_free: Set(source_recipe.is_dairy_free),
                        is_nut_free: Set(source_recipe.is_nut_free),
                        source_url: Set(source_recipe.source_url.clone()),
                        author_id: Set(Some(user_id)),
                        is_public: Set(false), // Imported recipes are private by default
                        rating_count: Set(0),
                        average_rating: Set(None),
                        created_at: Set(now),
                        updated_at: Set(now),
                        ..Default::default()
                    };

                    let saved_recipe = rec_model.insert(txn).await?;

                    // Copy ingredients
                    for ri in &source_recipe.ingredients {
                        let ri_model = recipe_ingredient::ActiveModel {
                            recipe_id: Set(saved_recipe.id),
                            ingredient_id: Set(ri.ingredient_id),
                            quantity: Set(ri.quantity),
                            unit: Set(ri.unit.clone()),
                            notes: Set(ri.notes.clone()),
                            display_order: Set(ri.display_order),
                            ..Default::default()
                        };
                        ri_model.insert(txn).await?;
                    }

                    // Copy steps
                    for step in &source_recipe.steps {
                        let step_model = recipe_step::ActiveModel {
                            recipe_id: Set(saved_recipe.id),
                            step_number: Set(step.step_number),
                            instruction: Set(step.instruction.clone()),
                            duration_min: Set(step.duration_min),
                            image_url: Set(step.image_url.clone()),
                            tip: Set(step.tip.clone()),
                            ..Default::default()
                        };
                        step_model.insert(txn).await?;
                    }

                    // Copy images
                    for img in &source_recipe.images {
                        let img_model = recipe_image::ActiveModel {
                            recipe_id: Set(saved_recipe.id),
                            url: Set(img.url.clone()),
                            image_type: Set(img.image_type.clone()),
                            is_primary: Set(img.is_primary),
                            width: Set(img.width),
                            height: Set(img.height),
                            ..Default::default()
                        };
                        img_model.insert(txn).await?;
                    }

                    // Copy nutrition
                    if let Some(ref nut) = source_recipe.nutrition {
                        let nut_model = recipe_nutrition::ActiveModel {
                            recipe_id: Set(saved_recipe.id),
                            calories: Set(nut.calories),
                            protein_g: Set(nut.protein_g),
                            carbs_g: Set(nut.carbs_g),
                            fat_g: Set(nut.fat_g),
                            fiber_g: Set(nut.fiber_g),
                            sugar_g: Set(nut.sugar_g),
                            sodium_mg: Set(nut.sodium_mg),
                            saturated_fat_g: Set(nut.saturated_fat_g),
                            per_serving: Set(nut.per_serving),
                            ..Default::default()
                        };
                        nut_model.insert(txn).await?;
                    }

                    Ok(saved_recipe)
                })
            })
            .await
            .map_err(|e| match e {
                sea_orm::TransactionError::Connection(de) => AppError::from(de),
                sea_orm::TransactionError::Transaction(ae) => ae,
            })?;

        Ok(serde_json::json!({
            "id": saved.id,
            "slug": saved.slug,
            "name": saved.name,
            "message": "Recipe imported successfully."
        }))
    }
}
