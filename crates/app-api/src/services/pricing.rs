//! Pricing service — estimates ingredient and recipe costs for budget-aware
//! meal planning.
//!
//! Price resolution order for an ingredient (per kilogram):
//!   1. Cheapest active store promotion linked to the ingredient (when priced per kg/g)
//!   2. `ingredients.base_price_per_kg` (seeded by the ETL pipeline)
//!   3. A category-based fallback estimate
//!   4. A global default
//!
//! A recipe's estimated cost is the sum over its ingredients of
//! `(quantity_grams / 1000) × price_per_kg`, falling back to a nominal amount
//! when an ingredient has no normalized gram quantity.

use std::collections::HashMap;

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::entity::{ingredient, recipe_ingredient};
use cookest_shared::errors::AppError;

/// Global fallback price per kg when nothing else is known (user currency).
const DEFAULT_PRICE_PER_KG: f64 = 5.0;

/// Assumed grams for an ingredient whose `quantity_grams` is unknown.
const FALLBACK_GRAMS: f64 = 100.0;

/// Category-based fallback price per kg (rough EU retail estimates).
fn category_price_per_kg(category: Option<&str>) -> f64 {
    match category.map(|c| c.to_ascii_lowercase()).as_deref() {
        Some("protein") | Some("meat") => 9.0,
        Some("fish") | Some("seafood") => 14.0,
        Some("dairy") => 4.0,
        Some("vegetable") => 2.5,
        Some("fruit") => 3.0,
        Some("grain") | Some("cereal") => 2.0,
        Some("legume") | Some("legumes") => 3.0,
        Some("fat") | Some("oil") => 6.0,
        Some("spice") | Some("herb") => 30.0,
        Some("condiment") | Some("sauce") => 7.0,
        _ => DEFAULT_PRICE_PER_KG,
    }
}

pub struct PricingService {
    db: DatabaseConnection,
}

impl PricingService {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// Resolve a price per kg for a single ingredient model.
    pub fn price_per_kg_for(ingredient: &ingredient::Model) -> f64 {
        if let Some(base) = &ingredient.base_price_per_kg {
            if let Ok(v) = f64::try_from(*base) {
                if v > 0.0 {
                    return v;
                }
            }
        }
        category_price_per_kg(ingredient.category.as_deref())
    }

    /// Estimate the cost of a single recipe (for the recipe's own serving count).
    pub async fn estimate_recipe_cost(&self, recipe_id: i64) -> Result<f64, AppError> {
        let map = self.estimate_recipe_costs(&[recipe_id]).await?;
        Ok(map.get(&recipe_id).copied().unwrap_or(0.0))
    }

    /// Estimate costs for many recipes at once (avoids N+1 queries during meal
    /// plan generation). Returns a map of recipe_id → estimated cost.
    pub async fn estimate_recipe_costs(
        &self,
        recipe_ids: &[i64],
    ) -> Result<HashMap<i64, f64>, AppError> {
        let mut costs: HashMap<i64, f64> = HashMap::new();
        if recipe_ids.is_empty() {
            return Ok(costs);
        }

        // Load all recipe-ingredient rows for the requested recipes.
        let ris = recipe_ingredient::Entity::find()
            .filter(recipe_ingredient::Column::RecipeId.is_in(recipe_ids.to_vec()))
            .all(&self.db)
            .await?;

        // Resolve a price per kg for every ingredient referenced.
        let ingredient_ids: Vec<i64> = {
            let mut s: std::collections::HashSet<i64> =
                ris.iter().map(|ri| ri.ingredient_id).collect();
            s.drain().collect()
        };

        let price_by_ingredient: HashMap<i64, f64> = ingredient::Entity::find()
            .filter(ingredient::Column::Id.is_in(ingredient_ids))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|ing| (ing.id, Self::price_per_kg_for(&ing)))
            .collect();

        for ri in &ris {
            let price_per_kg = price_by_ingredient
                .get(&ri.ingredient_id)
                .copied()
                .unwrap_or(DEFAULT_PRICE_PER_KG);

            let grams = ri
                .quantity_grams
                .and_then(|g| f64::try_from(g).ok())
                .filter(|g| *g > 0.0)
                .unwrap_or(FALLBACK_GRAMS);

            *costs.entry(ri.recipe_id).or_insert(0.0) += (grams / 1000.0) * price_per_kg;
        }

        // Recipes with no ingredient rows still get an entry (cost 0.0).
        for id in recipe_ids {
            costs.entry(*id).or_insert(0.0);
        }

        Ok(costs)
    }
}
