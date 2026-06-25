//! Ingredient service — searches ingredients via food-api and caches/details them locally

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set,
    TransactionTrait,
};

use crate::entity::{ingredient, ingredient_nutrient, portion_size};
use crate::handlers::browse::FoodApiClient;
use crate::models::ingredient::*;
use crate::models::recipe::PaginatedResponse;
use cookest_shared::errors::AppError;

pub struct IngredientService {
    db: DatabaseConnection,
    food_api_client: FoodApiClient,
}

impl IngredientService {
    pub fn new(db: DatabaseConnection, food_api_client: FoodApiClient) -> Self {
        Self {
            db,
            food_api_client,
        }
    }

    /// Search ingredients (used for inventory autocomplete) — proxies to food-api
    pub async fn search(
        &self,
        query: IngredientQuery,
    ) -> Result<PaginatedResponse<IngredientListItem>, AppError> {
        let q = query.q.unwrap_or_default();
        let page = query.page.unwrap_or(1);
        let per_page = query.per_page.unwrap_or(20);

        let path = format!(
            "/api/v1/ingredients?q={}&page={}&per_page={}",
            q, page, per_page
        );
        let req = self.food_api_client.get(&path);

        let resp = req.send().await.map_err(|e| {
            AppError::Internal(format!("Failed to search ingredients via food-api: {}", e))
        })?;

        let result = resp
            .json::<PaginatedResponse<IngredientListItem>>()
            .await
            .map_err(|e| {
                AppError::Internal(format!(
                    "Failed to parse search results from food-api: {}",
                    e
                ))
            })?;

        Ok(result)
    }

    /// Get full ingredient detail with nutrients and portions, mirroring it locally if missing.
    pub async fn get_ingredient(&self, id: i64) -> Result<IngredientDetail, AppError> {
        // 1. Try local lookup first.
        if let Some(detail) = self.read_local_detail(id).await? {
            return Ok(detail);
        }

        // 2. Fetch from the master catalog (food-api) and mirror it locally.
        let detail = self.fetch_food_api_ingredient(id).await?;
        self.insert_mirror(&detail).await?;
        Ok(detail)
    }

    /// Ensure a local mirror row exists for the given master catalog id, fetching
    /// and mirroring it from food-api when absent. Returns the canonical id.
    ///
    /// This is the single resolution point used by the pantry, recipes, and AI so
    /// every reference points at the master catalog (never a free-text junk row).
    /// `NotFound` if the id does not exist in the master catalog.
    pub async fn ensure_local_mirror(&self, food_id: i64) -> Result<i64, AppError> {
        if ingredient::Entity::find_by_id(food_id)
            .one(&self.db)
            .await?
            .is_some()
        {
            return Ok(food_id);
        }
        let detail = self.fetch_food_api_ingredient(food_id).await?;
        self.insert_mirror(&detail).await?;
        Ok(detail.id)
    }

    /// Resolve a free-text ingredient name to a catalog id by searching the master,
    /// preferring an exact (case-insensitive) name match, else the top result, then
    /// mirroring it locally. Returns `None` when nothing in the catalog matches.
    /// Never creates a new ingredient — the catalog is preset.
    pub async fn resolve_by_name(&self, name: &str) -> Result<Option<i64>, AppError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }

        let result = self
            .search(IngredientQuery {
                q: Some(trimmed.to_string()),
                category: None,
                page: Some(1),
                per_page: Some(20),
            })
            .await?;

        if result.data.is_empty() {
            return Ok(None);
        }

        let lower = trimmed.to_lowercase();
        let chosen = result
            .data
            .iter()
            .find(|i| i.name.to_lowercase() == lower)
            .or_else(|| result.data.first())
            .map(|i| i.id);

        match chosen {
            Some(id) => {
                self.ensure_local_mirror(id).await?;
                Ok(Some(id))
            }
            None => Ok(None),
        }
    }

    /// Build the full detail for an ingredient that already exists in the local mirror.
    async fn read_local_detail(&self, id: i64) -> Result<Option<IngredientDetail>, AppError> {
        let Some(ing) = ingredient::Entity::find_by_id(id).one(&self.db).await? else {
            return Ok(None);
        };

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

        Ok(Some(IngredientDetail {
            id: ing.id,
            name: ing.name,
            category: ing.category,
            image_url: ing.image_url,
            nutrients,
            portions,
        }))
    }

    /// Fetch a single ingredient from the master catalog (food-api).
    async fn fetch_food_api_ingredient(&self, id: i64) -> Result<IngredientDetail, AppError> {
        let path = format!("/api/v1/ingredients/{}", id);
        let resp = self
            .food_api_client
            .get(&path)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to reach food-api: {}", e)))?;

        if !resp.status().is_success() {
            return Err(AppError::NotFound(format!(
                "Ingredient {} not found in catalog",
                id
            )));
        }

        resp.json::<IngredientDetail>().await.map_err(|e| {
            AppError::Internal(format!(
                "Failed to parse ingredient detail from food-api: {}",
                e
            ))
        })
    }

    /// Upsert a mirror row (same id as the master) plus its nutrients and portions.
    /// All ingredient inserts in app-db go through here using an explicit id, so the
    /// BIGSERIAL sequence is never used for ingredients and cannot collide.
    async fn insert_mirror(&self, detail: &IngredientDetail) -> Result<(), AppError> {
        let detail = detail.clone();
        self.db
            .transaction::<_, (), AppError>(move |txn| {
                Box::pin(async move {
                    if ingredient::Entity::find_by_id(detail.id)
                        .one(txn)
                        .await?
                        .is_some()
                    {
                        return Ok(());
                    }

                    let ing_model = ingredient::ActiveModel {
                        id: Set(detail.id),
                        name: Set(detail.name.clone()),
                        category: Set(detail.category.clone()),
                        image_url: Set(detail.image_url.clone()),
                        created_at: Set(Utc::now().fixed_offset()),
                        ..Default::default()
                    };
                    ing_model.insert(txn).await?;

                    if let Some(nut) = &detail.nutrients {
                        let nut_model = ingredient_nutrient::ActiveModel {
                            ingredient_id: Set(detail.id),
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

                    for p in &detail.portions {
                        let p_model = portion_size::ActiveModel {
                            ingredient_id: Set(detail.id),
                            description: Set(p.description.clone()),
                            weight_grams: Set(p.weight_grams),
                            unit: Set(p.unit.clone()),
                            ..Default::default()
                        };
                        p_model.insert(txn).await?;
                    }
                    Ok(())
                })
            })
            .await
            .map_err(|e| match e {
                sea_orm::TransactionError::Connection(de) => AppError::from(de),
                sea_orm::TransactionError::Transaction(ae) => ae,
            })
    }
}
