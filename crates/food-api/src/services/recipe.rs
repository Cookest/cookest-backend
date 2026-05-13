//! Recipe service — queries recipes with filtering, pagination, and full detail loads

use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    PaginatorTrait, Condition, ActiveModelTrait, Set, Order,
    ConnectionTrait, Statement, TryGetable,
};
use chrono::Utc;

use crate::entity::{
    recipe, recipe_ingredient, recipe_step, recipe_image, recipe_nutrition, ingredient,
};
use crate::errors::AppError;
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
        let per_page = query.per_page.unwrap_or(20).min(100);

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
        if query.nut_free == Some(true) {
            condition = condition.add(recipe::Column::IsNutFree.eq(true));
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
        if let Some(language) = &query.language {
            condition = condition.add(recipe::Column::Language.eq(language));
        }
        if let Some(source_site) = &query.source_site {
            condition = condition.add(recipe::Column::SourceSite.eq(source_site));
        }
        if let Some(max_time) = query.max_time {
            condition = condition.add(recipe::Column::TotalTimeMin.lte(max_time));
        }

        // Full-text search on name using ILIKE (pg_trgm handles performance)
        if let Some(ref q) = query.q {
            let pattern = format!("%{}%", q);
            condition = condition.add(recipe::Column::Name.like(pattern));
        }

        // Sort order
        let desc = query.order.as_deref() == Some("desc");
        let ord = if desc { Order::Desc } else { Order::Asc };

        let base_query = recipe::Entity::find().filter(condition);
        let sorted_query = match query.sort.as_deref().unwrap_or("name") {
            "rating"  => base_query.order_by(recipe::Column::AverageRating, if desc { Order::Desc } else { Order::Desc }),
            "time"    => base_query.order_by(recipe::Column::TotalTimeMin, ord),
            "created" => base_query.order_by(recipe::Column::CreatedAt, if desc { Order::Desc } else { Order::Desc }),
            _         => base_query.order_by(recipe::Column::Name, ord),
        };

        let paginator = sorted_query.paginate(&self.db, per_page);

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
                    language: r.language,
                    tags: r.tags,
                    source_site: r.source_site,
                    average_rating: r.average_rating,
                    rating_count: r.rating_count,
                    primary_image_url: primary_image,
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

        self._build_detail(recipe).await
    }

    /// Get recipe by slug
    pub async fn get_recipe_by_slug(&self, slug: &str) -> Result<RecipeDetail, AppError> {
        let recipe = recipe::Entity::find()
            .filter(recipe::Column::Slug.eq(slug))
            .one(&self.db)
            .await?
            .ok_or(AppError::NotFound("Recipe".into()))?;

        self._build_detail(recipe).await
    }

    /// Internal helper: build RecipeDetail from a recipe model
    async fn _build_detail(&self, recipe: recipe::Model) -> Result<RecipeDetail, AppError> {
        let id = recipe.id;

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
                local_path: img.local_path,
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
                cholesterol_mg: n.cholesterol_mg,
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
            source_site: recipe.source_site,
            language: recipe.language,
            tags: recipe.tags,
            average_rating: recipe.average_rating,
            rating_count: recipe.rating_count,
            ingredients,
            steps,
            images,
            nutrition,
        })
    }

    /// Return N random recipes (optionally filtered by language/category/dietary)
    pub async fn random_recipes(
        &self,
        query: RandomQuery,
    ) -> Result<Vec<RecipeListItem>, AppError> {
        let count = query.count.unwrap_or(5).min(20) as i64;

        let mut wheres: Vec<String> = vec!["r.is_public = TRUE".to_string()];
        if let Some(lang) = &query.language {
            wheres.push(format!("r.language = '{}'", lang.replace('\'', "''")));
        }
        if let Some(cat) = &query.category {
            wheres.push(format!("r.category = '{}'", cat.replace('\'', "''")));
        }
        if query.vegetarian == Some(true) { wheres.push("r.is_vegetarian = TRUE".to_string()); }
        if query.vegan == Some(true) { wheres.push("r.is_vegan = TRUE".to_string()); }

        let where_clause = wheres.join(" AND ");
        let sql = format!(
            r#"SELECT r.id, r.name, r.slug, r.cuisine, r.category, r.difficulty,
                      r.servings, r.total_time_min, r.is_vegetarian, r.is_vegan,
                      r.is_gluten_free, r.is_dairy_free, r.language, r.tags,
                      r.source_site, r.average_rating, r.rating_count,
                      (SELECT url FROM recipe_images ri WHERE ri.recipe_id = r.id AND ri.is_primary LIMIT 1) AS primary_image_url
               FROM recipes r
               WHERE {where_clause}
               ORDER BY RANDOM()
               LIMIT {count}"#
        );

        let rows = self.db
            .query_all(Statement::from_string(sea_orm::DatabaseBackend::Postgres, sql))
            .await?;

        let items = rows.into_iter().map(|row| {
            RecipeListItem {
                id:               row.try_get("", "id").unwrap_or(0),
                name:             row.try_get("", "name").unwrap_or_default(),
                slug:             row.try_get("", "slug").unwrap_or_default(),
                cuisine:          row.try_get("", "cuisine").ok(),
                category:         row.try_get("", "category").ok(),
                difficulty:       row.try_get("", "difficulty").ok(),
                servings:         row.try_get("", "servings").unwrap_or(2),
                total_time_min:   row.try_get("", "total_time_min").ok(),
                is_vegetarian:    row.try_get("", "is_vegetarian").unwrap_or(false),
                is_vegan:         row.try_get("", "is_vegan").unwrap_or(false),
                is_gluten_free:   row.try_get("", "is_gluten_free").unwrap_or(false),
                is_dairy_free:    row.try_get("", "is_dairy_free").unwrap_or(false),
                language:         row.try_get("", "language").unwrap_or_else(|_| "en".into()),
                tags:             row.try_get("", "tags").ok(),
                source_site:      row.try_get("", "source_site").ok(),
                average_rating:   row.try_get("", "average_rating").ok(),
                rating_count:     row.try_get("", "rating_count").unwrap_or(0),
                primary_image_url: row.try_get("", "primary_image_url").ok(),
            }
        }).collect();

        Ok(items)
    }

    /// Scale a recipe's ingredient quantities to a target serving count
    pub async fn scale_recipe(
        &self,
        id: i64,
        target_servings: i32,
    ) -> Result<RecipeDetail, AppError> {
        if target_servings < 1 || target_servings > 100 {
            return Err(AppError::BadRequest("servings must be between 1 and 100".into()));
        }
        let mut detail = self.get_recipe(id).await?;
        let original = detail.servings;

        if original > 0 && original != target_servings {
            let factor = rust_decimal::Decimal::new(target_servings as i64, 0)
                / rust_decimal::Decimal::new(original as i64, 0);
            for ing in &mut detail.ingredients {
                if let Some(q) = ing.quantity {
                    ing.quantity = Some((q * factor).round_dp(2));
                }
                if let Some(qg) = ing.quantity_grams {
                    ing.quantity_grams = Some((qg * factor).round_dp(1));
                }
            }
            detail.servings = target_servings;
        }

        Ok(detail)
    }

    /// Get database statistics
    pub async fn get_stats(&self) -> Result<StatsResponse, AppError> {

        let count_row = self.db.query_one(Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            r#"SELECT
                (SELECT COUNT(*) FROM recipes)        AS total_recipes,
                (SELECT COUNT(*) FROM ingredients)    AS total_ingredients,
                (SELECT COUNT(*) FROM recipe_images)  AS total_images,
                (SELECT COUNT(DISTINCT recipe_id) FROM recipe_nutrition) AS recipes_with_nutrition,
                (SELECT COUNT(DISTINCT recipe_id) FROM recipe_steps)     AS recipes_with_steps
            "#.to_string(),
        )).await?.ok_or(AppError::Internal("Stats query failed".into()))?;

        let total_recipes: i64          = count_row.try_get("", "total_recipes").unwrap_or(0);
        let total_ingredients: i64      = count_row.try_get("", "total_ingredients").unwrap_or(0);
        let total_images: i64           = count_row.try_get("", "total_images").unwrap_or(0);
        let recipes_with_nutrition: i64 = count_row.try_get("", "recipes_with_nutrition").unwrap_or(0);
        let recipes_with_steps: i64     = count_row.try_get("", "recipes_with_steps").unwrap_or(0);

        let lang_rows = self.db.query_all(Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT COALESCE(language,'unknown') AS field, COUNT(*) AS count FROM recipes GROUP BY language ORDER BY count DESC".to_string(),
        )).await?;
        let by_language = lang_rows.into_iter().map(|r| CountByField {
            field: r.try_get("", "field").unwrap_or_default(),
            count: r.try_get("", "count").unwrap_or(0),
        }).collect();

        let cat_rows = self.db.query_all(Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT COALESCE(category,'unknown') AS field, COUNT(*) AS count FROM recipes GROUP BY category ORDER BY count DESC".to_string(),
        )).await?;
        let by_category = cat_rows.into_iter().map(|r| CountByField {
            field: r.try_get("", "field").unwrap_or_default(),
            count: r.try_get("", "count").unwrap_or(0),
        }).collect();

        let src_rows = self.db.query_all(Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT COALESCE(source_site,'unknown') AS field, COUNT(*) AS count FROM recipes GROUP BY source_site ORDER BY count DESC LIMIT 20".to_string(),
        )).await?;
        let by_source = src_rows.into_iter().map(|r| CountByField {
            field: r.try_get("", "field").unwrap_or_default(),
            count: r.try_get("", "count").unwrap_or(0),
        }).collect();

        Ok(StatsResponse {
            total_recipes,
            total_ingredients,
            total_images,
            recipes_with_nutrition,
            recipes_with_steps,
            by_language,
            by_category,
            by_source,
        })
    }

    /// Get distinct cuisines (non-null, sorted by count desc)
    pub async fn get_cuisines(&self) -> Result<Vec<CountByField>, AppError> {
        let rows = self.db.query_all(Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT cuisine AS field, COUNT(*) AS count FROM recipes WHERE cuisine IS NOT NULL GROUP BY cuisine ORDER BY count DESC".to_string(),
        )).await?;
        Ok(rows.into_iter().map(|r| CountByField {
            field: r.try_get("", "field").unwrap_or_default(),
            count: r.try_get("", "count").unwrap_or(0),
        }).collect())
    }

    /// Get distinct categories sorted by count
    pub async fn get_categories(&self) -> Result<Vec<CountByField>, AppError> {
        let rows = self.db.query_all(Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT COALESCE(category,'unknown') AS field, COUNT(*) AS count FROM recipes GROUP BY category ORDER BY count DESC".to_string(),
        )).await?;
        Ok(rows.into_iter().map(|r| CountByField {
            field: r.try_get("", "field").unwrap_or_default(),
            count: r.try_get("", "count").unwrap_or(0),
        }).collect())
    }

    /// Find recipes that contain a given ingredient (name search)
    pub async fn recipes_by_ingredient(
        &self,
        query: ByIngredientQuery,
    ) -> Result<PaginatedResponse<RecipeListItem>, AppError> {
        let page = query.page.unwrap_or(1).max(1);
        let per_page = query.per_page.unwrap_or(20).min(100);
        let offset = (page - 1) * per_page;
        let name_pattern = format!("%{}%", query.name.to_lowercase().replace('\'', "''"));

        let count_sql = format!(
            r#"SELECT COUNT(DISTINCT r.id) AS total FROM recipes r
               JOIN recipe_ingredients ri ON ri.recipe_id = r.id
               JOIN ingredients ing ON ing.id = ri.ingredient_id
               WHERE LOWER(ing.name) LIKE '{name_pattern}'"#
        );
        let count_row = self.db.query_one(Statement::from_string(
            sea_orm::DatabaseBackend::Postgres, count_sql,
        )).await?.ok_or(AppError::Internal("Count query failed".into()))?;
        let total: i64 = count_row.try_get("", "total").unwrap_or(0);

        let data_sql = format!(
            r#"SELECT DISTINCT ON (r.id) r.id, r.name, r.slug, r.cuisine, r.category,
                      r.difficulty, r.servings, r.total_time_min, r.is_vegetarian,
                      r.is_vegan, r.is_gluten_free, r.is_dairy_free, r.language,
                      r.tags, r.source_site, r.average_rating, r.rating_count,
                      (SELECT url FROM recipe_images ri2 WHERE ri2.recipe_id = r.id AND ri2.is_primary LIMIT 1) AS primary_image_url
               FROM recipes r
               JOIN recipe_ingredients ri ON ri.recipe_id = r.id
               JOIN ingredients ing ON ing.id = ri.ingredient_id
               WHERE LOWER(ing.name) LIKE '{name_pattern}'
               ORDER BY r.id, r.name
               LIMIT {per_page} OFFSET {offset}"#
        );
        let rows = self.db.query_all(Statement::from_string(
            sea_orm::DatabaseBackend::Postgres, data_sql,
        )).await?;

        let items = rows.into_iter().map(|row| RecipeListItem {
            id:               row.try_get("", "id").unwrap_or(0),
            name:             row.try_get("", "name").unwrap_or_default(),
            slug:             row.try_get("", "slug").unwrap_or_default(),
            cuisine:          row.try_get("", "cuisine").ok(),
            category:         row.try_get("", "category").ok(),
            difficulty:       row.try_get("", "difficulty").ok(),
            servings:         row.try_get("", "servings").unwrap_or(2),
            total_time_min:   row.try_get("", "total_time_min").ok(),
            is_vegetarian:    row.try_get("", "is_vegetarian").unwrap_or(false),
            is_vegan:         row.try_get("", "is_vegan").unwrap_or(false),
            is_gluten_free:   row.try_get("", "is_gluten_free").unwrap_or(false),
            is_dairy_free:    row.try_get("", "is_dairy_free").unwrap_or(false),
            language:         row.try_get("", "language").unwrap_or_else(|_| "en".into()),
            tags:             row.try_get("", "tags").ok(),
            source_site:      row.try_get("", "source_site").ok(),
            average_rating:   row.try_get("", "average_rating").ok(),
            rating_count:     row.try_get("", "rating_count").unwrap_or(0),
            primary_image_url: row.try_get("", "primary_image_url").ok(),
        }).collect();

        Ok(PaginatedResponse {
            data: items,
            total: total as u64,
            page,
            per_page,
            total_pages: (total as f64 / per_page as f64).ceil() as u64,
        })
    }

    /// Create a recipe (no user_id — food-api recipes are managed via API key permissions)
    pub async fn create_recipe(
        &self,
        req: CreateRecipeRequest,
    ) -> Result<serde_json::Value, AppError> {
        use slug::slugify;
        let now = Utc::now().fixed_offset();
        let base_slug = slugify(&req.name);
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
            language: Set("en".to_string()),
            author_id: Set(None),
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
            "message": "Recipe created. Use POST /api/v1/recipes/:id/ingredients and /steps to add content."
        }))
    }

    /// Update a recipe by ID (no ownership check — food-api uses API key permissions)
    pub async fn update_recipe(
        &self,
        recipe_id: i64,
        req: UpdateRecipeRequest,
    ) -> Result<serde_json::Value, AppError> {
        let existing = recipe::Entity::find_by_id(recipe_id)
            .one(&self.db)
            .await?
            .ok_or(AppError::NotFound("Recipe".into()))?;

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

    /// Delete a recipe by ID (no ownership check — food-api uses API key permissions)
    pub async fn delete_recipe(&self, recipe_id: i64) -> Result<(), AppError> {
        recipe::Entity::find_by_id(recipe_id)
            .one(&self.db)
            .await?
            .ok_or(AppError::NotFound("Recipe".into()))?;

        recipe::Entity::delete_by_id(recipe_id)
            .exec(&self.db)
            .await?;

        Ok(())
    }
}
