//! Ingredient service — supports Local (SeaORM), FatSecret, and Hybrid data sources

use std::sync::Arc;
use std::str::FromStr;
use rust_decimal::Decimal;

use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect};
use sea_orm::sea_query::{Expr, extension::postgres::PgExpr};

use crate::config::FoodDataSource;
use crate::errors::AppError;
use crate::models::ingredient::*;
use crate::models::recipe::PaginatedResponse;
use crate::services::FatSecretClient;
use crate::entity::ingredient::{Entity as IngredientEntity, Column as IngredientCol};
use crate::entity::ingredient_nutrient::Entity as NutrientEntity;
use crate::entity::portion_size::Entity as PortionEntity;
use crate::entity::recipe_ingredient::Entity as RecipeIngredientEntity;

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

    /// Helper to resolve the active food data source dynamically from the DB settings table
    async fn get_active_source(&self) -> FoodDataSource {
        use sea_orm::{ConnectionTrait, Statement};
        let query = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT value FROM system_settings WHERE key = 'food_data_source'",
            vec![],
        );
        if let Ok(Some(row)) = self.db.query_one(query).await {
            if let Ok(val) = row.try_get::<String>("", "value") {
                match val.as_str() {
                    "local" => return FoodDataSource::Local,
                    "fatsecret" => return FoodDataSource::FatSecret,
                    "openfoodfacts" => return FoodDataSource::OpenFoodFacts,
                    "hybrid" => return FoodDataSource::Hybrid,
                    _ => {}
                }
            }
        }
        self.source.clone()
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
        let active_source = self.get_active_source().await;

        match active_source {
            FoodDataSource::Local | FoodDataSource::OpenFoodFacts => {
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
                // Case-insensitive substring match so "chick" finds "Chicken".
                q = q.filter(Expr::col(IngredientCol::Name).ilike(format!("%{}%", search)));
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
                image_url: r.image_url,
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
                        image_url: None,
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
        let active_source = self.get_active_source().await;
        match active_source {
            FoodDataSource::Local => self.get_ingredient_local(id).await,
            FoodDataSource::FatSecret => self.get_ingredient_fatsecret(id).await,
            FoodDataSource::OpenFoodFacts => self.get_ingredient_openfoodfacts(id).await,
            FoodDataSource::Hybrid => {
                match self.get_ingredient_local(id).await {
                    Ok(detail) => {
                        // If it has a barcode (off_id) but no image, fetch image dynamically from Open Food Facts API and save it!
                        if detail.image_url.is_none() {
                            if let Ok(ing) = IngredientEntity::find_by_id(id).one(&self.db).await {
                                if let Some(ing) = ing {
                                    if let Some(barcode) = &ing.off_id {
                                        use crate::services::openfoodfacts::OpenFoodFactsClient;
                                        let off_client = OpenFoodFactsClient::new();
                                        if let Ok(off_detail) = off_client.get_by_barcode(barcode, id).await {
                                            if let Some(img) = &off_detail.image_url {
                                                use sea_orm::ActiveModelTrait;
                                                let mut active: crate::entity::ingredient::ActiveModel = ing.into();
                                                active.image_url = sea_orm::Set(Some(img.clone()));
                                                let _ = active.update(&self.db).await;
                                                let mut updated_detail = detail;
                                                updated_detail.image_url = Some(img.clone());
                                                return Ok(updated_detail);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Ok(detail)
                    }
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
            image_url: ingredient.image_url,
            nutrients,
            portions,
        })
    }

    async fn get_ingredient_openfoodfacts(&self, id: i64) -> Result<IngredientDetail, AppError> {
        let detail = self.get_ingredient_local(id).await?;
        if detail.image_url.is_none() {
            if let Ok(ing) = IngredientEntity::find_by_id(id).one(&self.db).await {
                if let Some(ing) = ing {
                    if let Some(barcode) = &ing.off_id {
                        use crate::services::openfoodfacts::OpenFoodFactsClient;
                        let off_client = OpenFoodFactsClient::new();
                        if let Ok(off_detail) = off_client.get_by_barcode(barcode, id).await {
                            if let Some(img) = &off_detail.image_url {
                                use sea_orm::ActiveModelTrait;
                                let mut active: crate::entity::ingredient::ActiveModel = ing.into();
                                active.image_url = sea_orm::Set(Some(img.clone()));
                                let _ = active.update(&self.db).await;
                                let mut updated_detail = detail;
                                updated_detail.image_url = Some(img.clone());
                                return Ok(updated_detail);
                            }
                        }
                    }
                }
            }
        }
        Ok(detail)
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
            image_url: None,
            nutrients,
            portions,
        })
    }

    // -------------------------------------------------------------------------
    // get_by_barcode
    // -------------------------------------------------------------------------

    /// Resolve a barcode to an ingredient detail
    pub async fn get_by_barcode(&self, barcode: &str) -> Result<IngredientDetail, AppError> {
        let active_source = self.get_active_source().await;
        match active_source {
            FoodDataSource::Local => {
                self.get_by_barcode_local(barcode).await
            }
            FoodDataSource::FatSecret => {
                self.get_by_barcode_fatsecret(barcode).await
            }
            FoodDataSource::OpenFoodFacts => {
                // Try local DB first (off_id column)
                match self.get_by_barcode_local(barcode).await {
                    Ok(detail) => return Ok(detail),
                    Err(AppError::NotFound(_)) => {}
                    Err(e) => return Err(e),
                }

                // Fetch live from OpenFoodFacts and save to DB
                use crate::services::openfoodfacts::OpenFoodFactsClient;
                let off_client = OpenFoodFactsClient::new();
                let detail = off_client.get_by_barcode(barcode, 0).await?;

                // Save to local DB so it can be searched
                let now = chrono::Utc::now().fixed_offset();
                let ing_row = sea_orm::ConnectionTrait::query_one(&self.db, sea_orm::Statement::from_sql_and_values(
                    sea_orm::DatabaseBackend::Postgres,
                    "INSERT INTO ingredients (name, category, off_id, image_url, created_at) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (name) DO UPDATE SET off_id = EXCLUDED.off_id RETURNING id",
                    [detail.name.clone().into(), detail.category.clone().into(), Some(barcode).into(), detail.image_url.clone().into(), now.into()],
                )).await?.ok_or_else(|| AppError::Internal("Failed to insert OFF ingredient".to_string()))?;

                let ingredient_id: i64 = ing_row.try_get("", "id")?;

                if let Some(ref n) = detail.nutrients {
                    let _ = sea_orm::ConnectionTrait::execute(&self.db, sea_orm::Statement::from_sql_and_values(
                        sea_orm::DatabaseBackend::Postgres,
                        "INSERT INTO ingredient_nutrients (ingredient_id, calories, protein_g, carbs_g, fat_g, fiber_g, sugar_g, sodium_mg, saturated_fat_g) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) ON CONFLICT (ingredient_id) DO NOTHING",
                        [ingredient_id.into(), n.calories.into(), n.protein_g.into(), n.carbs_g.into(), n.fat_g.into(), n.fiber_g.into(), n.sugar_g.into(), n.sodium_mg.into(), n.saturated_fat_g.into()],
                    )).await;
                }

                let mut updated_detail = detail;
                updated_detail.id = ingredient_id;
                Ok(updated_detail)
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
                    Ok(mut detail) => {
                        // Save to local DB so it can be searched
                        let now = chrono::Utc::now().fixed_offset();
                        if let Ok(Some(ing_row)) = sea_orm::ConnectionTrait::query_one(&self.db, sea_orm::Statement::from_sql_and_values(
                            sea_orm::DatabaseBackend::Postgres,
                            "INSERT INTO ingredients (name, category, off_id, image_url, created_at) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (name) DO UPDATE SET off_id = EXCLUDED.off_id RETURNING id",
                            [detail.name.clone().into(), detail.category.clone().into(), Some(barcode).into(), detail.image_url.clone().into(), now.into()],
                        )).await {
                            if let Ok(ingredient_id) = ing_row.try_get::<i64>("", "id") {
                                if let Some(ref n) = detail.nutrients {
                                    let _ = sea_orm::ConnectionTrait::execute(&self.db, sea_orm::Statement::from_sql_and_values(
                                        sea_orm::DatabaseBackend::Postgres,
                                        "INSERT INTO ingredient_nutrients (ingredient_id, calories, protein_g, carbs_g, fat_g, fiber_g, sugar_g, sodium_mg, saturated_fat_g) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) ON CONFLICT (ingredient_id) DO NOTHING",
                                        [ingredient_id.into(), n.calories.into(), n.protein_g.into(), n.carbs_g.into(), n.fat_g.into(), n.fiber_g.into(), n.sugar_g.into(), n.sodium_mg.into(), n.saturated_fat_g.into()],
                                    )).await;
                                }
                                detail.id = ingredient_id;
                            }
                        }
                        return Ok(detail);
                    }
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

    // -------------------------------------------------------------------------
    // admin CRUD — always operate on the LOCAL master catalog, regardless of
    // the active read source. The catalog is the single source of truth.
    // -------------------------------------------------------------------------

    /// List the distinct, non-empty ingredient categories (for filter dropdowns).
    pub async fn list_categories(&self) -> Result<Vec<String>, AppError> {
        use sea_orm::{ConnectionTrait, Statement};
        let rows = self
            .db
            .query_all(Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                "SELECT DISTINCT category FROM ingredients \
                 WHERE category IS NOT NULL AND category <> '' ORDER BY category"
                    .to_string(),
            ))
            .await?;
        let categories = rows
            .into_iter()
            .filter_map(|r| r.try_get::<String>("", "category").ok())
            .collect();
        Ok(categories)
    }

    /// Create a new catalog ingredient. Rejects duplicate names (the catalog is canonical).
    pub async fn create_ingredient(
        &self,
        req: CreateIngredientRequest,
    ) -> Result<IngredientDetail, AppError> {
        use sea_orm::{ActiveModelTrait, Set};

        let name = req.name.trim().to_string();
        if name.is_empty() {
            return Err(AppError::BadRequest("Ingredient name is required".to_string()));
        }

        if IngredientEntity::find()
            .filter(IngredientCol::Name.eq(&name))
            .one(&self.db)
            .await?
            .is_some()
        {
            return Err(AppError::Conflict(format!("Ingredient '{}' already exists", name)));
        }

        let now = chrono::Utc::now().fixed_offset();
        let model = crate::entity::ingredient::ActiveModel {
            name: Set(name),
            category: Set(req.category),
            fdc_id: Set(req.fdc_id),
            off_id: Set(req.off_id),
            image_url: Set(req.image_url),
            created_at: Set(now),
            ..Default::default()
        };
        let saved = model.insert(&self.db).await?;

        if let Some(n) = req.nutrients {
            self.upsert_nutrients(saved.id, &n).await?;
        }

        self.get_ingredient_local(saved.id).await
    }

    /// Update an existing catalog ingredient. Only provided fields are changed.
    pub async fn update_ingredient(
        &self,
        id: i64,
        req: UpdateIngredientRequest,
    ) -> Result<IngredientDetail, AppError> {
        use sea_orm::{ActiveModelTrait, Set};

        let existing = IngredientEntity::find_by_id(id)
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Ingredient {}", id)))?;

        let mut active: crate::entity::ingredient::ActiveModel = existing.into();

        if let Some(name) = req.name {
            let name = name.trim().to_string();
            if name.is_empty() {
                return Err(AppError::BadRequest("Ingredient name cannot be empty".to_string()));
            }
            // Disallow colliding with a different ingredient's name.
            if let Some(other) = IngredientEntity::find()
                .filter(IngredientCol::Name.eq(&name))
                .one(&self.db)
                .await?
            {
                if other.id != id {
                    return Err(AppError::Conflict(format!("Ingredient '{}' already exists", name)));
                }
            }
            active.name = Set(name);
        }
        if let Some(category) = req.category {
            active.category = Set(Some(category));
        }
        if let Some(image_url) = req.image_url {
            active.image_url = Set(Some(image_url));
        }
        if let Some(fdc_id) = req.fdc_id {
            active.fdc_id = Set(Some(fdc_id));
        }
        if let Some(off_id) = req.off_id {
            active.off_id = Set(Some(off_id));
        }
        active.update(&self.db).await?;

        if let Some(n) = req.nutrients {
            self.upsert_nutrients(id, &n).await?;
        }

        self.get_ingredient_local(id).await
    }

    /// Delete a catalog ingredient. Blocked (409) when still referenced by any recipe.
    pub async fn delete_ingredient(&self, id: i64) -> Result<(), AppError> {
        IngredientEntity::find_by_id(id)
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Ingredient {}", id)))?;

        let ref_count = RecipeIngredientEntity::find()
            .filter(crate::entity::recipe_ingredient::Column::IngredientId.eq(id))
            .count(&self.db)
            .await?;
        if ref_count > 0 {
            return Err(AppError::Conflict(format!(
                "Ingredient is used by {} recipe(s) and cannot be deleted",
                ref_count
            )));
        }

        IngredientEntity::delete_by_id(id).exec(&self.db).await?;
        Ok(())
    }

    /// Upsert the per-100g nutrient row for an ingredient.
    async fn upsert_nutrients(
        &self,
        ingredient_id: i64,
        n: &IngredientNutrientDetail,
    ) -> Result<(), AppError> {
        use sea_orm::{ConnectionTrait, Statement};
        self.db
            .execute(Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Postgres,
                "INSERT INTO ingredient_nutrients \
                   (ingredient_id, calories, protein_g, carbs_g, fat_g, fiber_g, sugar_g, \
                    sodium_mg, saturated_fat_g, cholesterol_mg) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) \
                 ON CONFLICT (ingredient_id) DO UPDATE SET \
                   calories=EXCLUDED.calories, protein_g=EXCLUDED.protein_g, carbs_g=EXCLUDED.carbs_g, \
                   fat_g=EXCLUDED.fat_g, fiber_g=EXCLUDED.fiber_g, sugar_g=EXCLUDED.sugar_g, \
                   sodium_mg=EXCLUDED.sodium_mg, saturated_fat_g=EXCLUDED.saturated_fat_g, \
                   cholesterol_mg=EXCLUDED.cholesterol_mg",
                [
                    ingredient_id.into(),
                    n.calories.into(),
                    n.protein_g.into(),
                    n.carbs_g.into(),
                    n.fat_g.into(),
                    n.fiber_g.into(),
                    n.sugar_g.into(),
                    n.sodium_mg.into(),
                    n.saturated_fat_g.into(),
                    n.cholesterol_mg.into(),
                ],
            ))
            .await?;
        Ok(())
    }
}
