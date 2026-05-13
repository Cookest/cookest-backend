//! Ingredient service — search and detail with nutrients

use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    PaginatorTrait, Condition,
};

use crate::entity::{ingredient, ingredient_nutrient, portion_size};
use crate::errors::AppError;
use crate::models::ingredient::*;
use crate::models::recipe::PaginatedResponse;

pub struct IngredientService {
    db: DatabaseConnection,
}

impl IngredientService {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// Search ingredients (used for inventory autocomplete)
    pub async fn search(
        &self,
        query: IngredientQuery,
    ) -> Result<PaginatedResponse<IngredientListItem>, AppError> {
        let page = query.page.unwrap_or(1).max(1);
        let per_page = query.per_page.unwrap_or(20).min(100);

        let mut condition = Condition::all();

        if let Some(ref q) = query.q {
            let pattern = format!("%{}%", q);
            condition = condition.add(ingredient::Column::Name.like(pattern));
        }
        if let Some(ref category) = query.category {
            condition = condition.add(ingredient::Column::Category.eq(category));
        }

        let paginator = ingredient::Entity::find()
            .filter(condition)
            .order_by_asc(ingredient::Column::Name)
            .paginate(&self.db, per_page);

        let total = paginator.num_items().await?;
        let items = paginator
            .fetch_page(page - 1)
            .await?
            .into_iter()
            .map(|ing| IngredientListItem {
                id: ing.id,
                name: ing.name,
                category: ing.category,
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

    /// Get full ingredient detail with nutrients and portions
    pub async fn get_ingredient(&self, id: i64) -> Result<IngredientDetail, AppError> {
        let ing = ingredient::Entity::find_by_id(id)
            .one(&self.db)
            .await?
            .ok_or(AppError::NotFound("Ingredient".into()))?;

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

        Ok(IngredientDetail {
            id: ing.id,
            name: ing.name,
            category: ing.category,
            nutrients,
            portions,
        })
    }
}
