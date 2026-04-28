//! Meal Plan Service — AI-assisted weekly meal planning
//!
//! Scoring formula per recipe (weights sum to 100):
//!   ingredient_coverage × 30  — prefer recipes user already has ingredients for
//!   expiry_urgency      × 25  — prioritise ingredients close to expiry
//!   ml_preference       × 25  — learned user taste via PreferenceService
//!   nutrition_balance   × 12  — fill the week's nutritional gaps
//!   variety_bonus       ×  8  — penalise recently cooked recipes

use chrono::{Datelike, Duration, NaiveDate, Utc};
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    ActiveModelTrait, Set, PaginatorTrait,
};
use std::collections::HashMap;
use uuid::Uuid;

use crate::entity::{
    recipe, recipe_ingredient, recipe_nutrition, inventory_item, cooking_history,
    user_favorite, meal_plan, meal_plan_slot,
};
use crate::errors::AppError;
use crate::services::PreferenceService;

/// Ideal daily nutrition targets (per person)
const DAILY_CALORIES: f64 = 2000.0;
const DAILY_PROTEIN_G: f64 = 50.0;
const DAILY_CARBS_G: f64 = 275.0;
const DAILY_FAT_G: f64 = 78.0;
const DAILY_FIBER_G: f64 = 28.0;

/// Score for each candidate recipe
#[derive(Debug)]
struct RecipeScore {
    recipe_id: i64,
    total_score: f64,
    cuisine: Option<String>,
    category: Option<String>,
    total_time_min: Option<i32>,
}

pub struct MealPlanService {
    db: DatabaseConnection,
    preference_service: PreferenceService,
}

impl MealPlanService {
    pub fn new(db: DatabaseConnection) -> Self {
        let pref_db = db.clone();
        Self {
            db,
            preference_service: PreferenceService::new(pref_db),
        }
    }

    /// Generate a full week meal plan for a user and save it to the database.
    /// Generates 4 slots per day × 7 days: breakfast, lunch, dinner, snack.
    pub async fn generate_week_plan(
        &self,
        user_id: Uuid,
        household_size: i32,
        week_start: NaiveDate,
    ) -> Result<meal_plan::Model, AppError> {
        // ── 1. Load context data ──────────────────────────────────────────────

        let inventory = inventory_item::Entity::find()
            .filter(inventory_item::Column::UserId.eq(user_id))
            .all(&self.db)
            .await?;

        let user_ingredient_ids: std::collections::HashSet<i64> =
            inventory.iter().map(|i| i.ingredient_id).collect();

        let expiry_threshold = Utc::now().date_naive() + Duration::days(7);
        let expiring_ids: std::collections::HashSet<i64> = inventory
            .iter()
            .filter(|i| i.expiry_date.map(|d| d <= expiry_threshold).unwrap_or(false))
            .map(|i| i.ingredient_id)
            .collect();

        let two_weeks_ago = Utc::now().fixed_offset() - Duration::days(14);
        let recent_recipes: std::collections::HashSet<i64> = cooking_history::Entity::find()
            .filter(cooking_history::Column::UserId.eq(user_id))
            .filter(cooking_history::Column::CookedAt.gte(two_weeks_ago))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|h| h.recipe_id)
            .collect();

        let favourite_ids: std::collections::HashSet<i64> = user_favorite::Entity::find()
            .filter(user_favorite::Column::UserId.eq(user_id))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|f| f.recipe_id)
            .collect();

        let all_recipes = recipe::Entity::find()
            .order_by_asc(recipe::Column::Id)
            .all(&self.db)
            .await?;

        let all_recipe_ids: Vec<i64> = all_recipes.iter().map(|r| r.id).collect();
        let all_recipe_ingredients = recipe_ingredient::Entity::find()
            .filter(recipe_ingredient::Column::RecipeId.is_in(all_recipe_ids.clone()))
            .all(&self.db)
            .await?;

        let mut ingredients_by_recipe: HashMap<i64, Vec<i64>> = HashMap::new();
        for ri in &all_recipe_ingredients {
            ingredients_by_recipe
                .entry(ri.recipe_id)
                .or_default()
                .push(ri.ingredient_id);
        }

        let nutrition_by_recipe: HashMap<i64, _> = recipe_nutrition::Entity::find()
            .filter(recipe_nutrition::Column::RecipeId.is_in(all_recipe_ids))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|n| (n.recipe_id, n))
            .collect();

        // ── 2. Score every recipe ─────────────────────────────────────────────

        let mut scored: Vec<RecipeScore> = Vec::new();

        let weekly_calories = 0.0_f64;
        let weekly_protein = 0.0_f64;

        for recipe in &all_recipes {
            if recent_recipes.contains(&recipe.id) {
                continue;
            }

            let recipe_ing_ids = ingredients_by_recipe
                .get(&recipe.id)
                .cloned()
                .unwrap_or_default();

            let total_ings = recipe_ing_ids.len().max(1);
            let owned_ings = recipe_ing_ids
                .iter()
                .filter(|id| user_ingredient_ids.contains(id))
                .count();
            let ingredient_coverage = owned_ings as f64 / total_ings as f64;

            let expiring_used = recipe_ing_ids
                .iter()
                .filter(|id| expiring_ids.contains(id))
                .count();
            let expiry_urgency = (expiring_used as f64 / total_ings as f64).min(1.0);

            let ml_preference = self
                .preference_service
                .score_recipe_for_user(user_id, recipe.id)
                .await
                .unwrap_or(0.5);

            let variety_bonus = if recent_recipes.contains(&recipe.id) {
                -0.3
            } else if favourite_ids.contains(&recipe.id) {
                0.1
            } else {
                0.0
            };

            let nutrition_balance = if let Some(n) = nutrition_by_recipe.get(&recipe.id) {
                let scale = household_size as f64 / recipe.servings.max(1) as f64;
                let cal = f64::try_from(n.calories.unwrap_or_default()).unwrap_or(0.0) * scale;
                let pro = f64::try_from(n.protein_g.unwrap_or_default()).unwrap_or(0.0) * scale;

                let cal_gap = (DAILY_CALORIES * 7.0 - weekly_calories).max(0.0);
                let pro_gap = (DAILY_PROTEIN_G * 7.0 - weekly_protein).max(0.0);

                let cal_score = (cal / cal_gap.max(1.0)).min(1.0);
                let pro_score = (pro / pro_gap.max(1.0)).min(1.0);
                (cal_score + pro_score) / 2.0
            } else {
                0.5
            };

            let total_score = (ingredient_coverage * 0.30)
                + (expiry_urgency * 0.25)
                + (ml_preference * 0.25)
                + (nutrition_balance * 0.12)
                + (variety_bonus * 0.08);

            scored.push(RecipeScore {
                recipe_id: recipe.id,
                total_score,
                cuisine: recipe.cuisine.clone(),
                category: recipe.category.clone(),
                total_time_min: recipe.total_time_min,
            });
        }

        scored.sort_by(|a, b| b.total_score.partial_cmp(&a.total_score).unwrap());

        // ── 3. Greedy selection — 28 slots (4 per day × 7 days) ──────────────
        // Meal types: breakfast, lunch, dinner, snack

        let mut selected: Vec<(i64, u8, &str)> = Vec::new();
        let mut used_recipe_ids: std::collections::HashSet<i64> = std::collections::HashSet::new();
        let mut used_cuisines_today: HashMap<u8, String> = HashMap::new();

        let meal_slots: &[(&str, fn(&Option<String>, &str) -> bool)] = &[
            ("breakfast", |cat, mt| Self::fits_meal_type_static(cat, mt)),
            ("lunch",     |cat, mt| Self::fits_meal_type_static(cat, mt)),
            ("dinner",    |cat, mt| Self::fits_meal_type_static(cat, mt)),
            ("snack",     |cat, mt| Self::fits_meal_type_static(cat, mt)),
        ];

        for day in 0u8..7 {
            for (meal_type, fits) in meal_slots {
                // Avoid same cuisine for lunch and dinner on same day
                let avoid_cuisine = used_cuisines_today.get(&day).cloned();

                if let Some(recipe) = scored.iter().find(|r| {
                    !used_recipe_ids.contains(&r.recipe_id)
                        && fits(&r.category, meal_type)
                        && !(meal_type == &"dinner"
                            && avoid_cuisine.as_ref() == r.cuisine.as_ref())
                }) {
                    selected.push((recipe.recipe_id, day, meal_type));
                    used_recipe_ids.insert(recipe.recipe_id);
                    if meal_type == &"lunch" {
                        if let Some(cuisine) = &recipe.cuisine {
                            used_cuisines_today.insert(day, cuisine.clone());
                        }
                    }
                }
            }
        }

        // ── 4. Save meal plan + slots to database ─────────────────────────────

        let now = Utc::now().fixed_offset();

        if let Some(existing) = meal_plan::Entity::find()
            .filter(meal_plan::Column::UserId.eq(user_id))
            .filter(meal_plan::Column::WeekStart.eq(week_start))
            .one(&self.db)
            .await?
        {
            meal_plan_slot::Entity::delete_many()
                .filter(meal_plan_slot::Column::MealPlanId.eq(existing.id))
                .exec(&self.db)
                .await?;

            meal_plan::Entity::delete_by_id(existing.id)
                .exec(&self.db)
                .await?;
        }

        let plan = meal_plan::ActiveModel {
            user_id: Set(user_id),
            week_start: Set(week_start),
            is_ai_generated: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };

        let saved_plan = plan.insert(&self.db).await?;

        for (recipe_id, day, meal_type) in selected {
            let slot = meal_plan_slot::ActiveModel {
                meal_plan_id: Set(saved_plan.id),
                recipe_id: Set(Some(recipe_id)),
                day_of_week: Set(day as i16),
                meal_type: Set(meal_type.to_string()),
                servings_override: Set(Some(household_size)),
                is_completed: Set(false),
                ..Default::default()
            };
            slot.insert(&self.db).await?;
        }

        Ok(saved_plan)
    }

    /// Heuristic: which meal types fit a recipe's category (static version for closures)
    fn fits_meal_type_static(category: &Option<String>, meal_type: &str) -> bool {
        match (category.as_deref(), meal_type) {
            (Some("breakfast"), "breakfast") => true,
            (Some("breakfast"), _) => false,
            (Some("dessert"), "snack") => true,
            (Some("dessert"), _) => false,
            (Some("snack"), "snack") => true,
            (Some("snack"), _) => false,
            (_, "snack") => false,
            (_, "breakfast") => false,
            _ => true,
        }
    }

    /// Heuristic: which meal types fit a recipe's category
    fn fits_meal_type(&self, category: &Option<String>, meal_type: &str) -> bool {
        Self::fits_meal_type_static(category, meal_type)
    }

    /// Get the current week's meal plan with full recipe details per slot
    pub async fn get_current_plan(
        &self,
        user_id: Uuid,
    ) -> Result<Option<serde_json::Value>, AppError> {
        let today = Utc::now().date_naive();
        let days_since_monday = today.weekday().num_days_from_monday() as i64;
        let week_start = today - chrono::Duration::days(days_since_monday);

        let plan = meal_plan::Entity::find()
            .filter(meal_plan::Column::UserId.eq(user_id))
            .filter(meal_plan::Column::WeekStart.eq(week_start))
            .one(&self.db)
            .await?;

        let Some(plan) = plan else {
            return Ok(None);
        };

        Ok(Some(self.plan_to_json(&plan).await?))
    }

    /// List all meal plans for a user (newest first, paginated)
    pub async fn list_plans(
        &self,
        user_id: Uuid,
        page: u64,
        per_page: u64,
    ) -> Result<serde_json::Value, AppError> {
        let paginator = meal_plan::Entity::find()
            .filter(meal_plan::Column::UserId.eq(user_id))
            .order_by_desc(meal_plan::Column::WeekStart)
            .paginate(&self.db, per_page);

        let total = paginator.num_items().await?;
        let plans = paginator.fetch_page(page.saturating_sub(1)).await?;

        let mut plan_list = Vec::new();
        for p in plans {
            plan_list.push(serde_json::json!({
                "id": p.id,
                "week_start": p.week_start,
                "is_ai_generated": p.is_ai_generated,
                "created_at": p.created_at,
            }));
        }

        Ok(serde_json::json!({
            "total": total,
            "page": page,
            "per_page": per_page,
            "plans": plan_list,
        }))
    }

    /// Get a specific meal plan by ID (must belong to user)
    pub async fn get_plan(
        &self,
        user_id: Uuid,
        plan_id: i64,
    ) -> Result<serde_json::Value, AppError> {
        let plan = meal_plan::Entity::find_by_id(plan_id)
            .one(&self.db)
            .await?
            .filter(|p| p.user_id == user_id)
            .ok_or(AppError::NotFound("Meal plan".into()))?;

        self.plan_to_json(&plan).await
    }

    /// Delete a meal plan and all its slots
    pub async fn delete_plan(&self, user_id: Uuid, plan_id: i64) -> Result<(), AppError> {
        let plan = meal_plan::Entity::find_by_id(plan_id)
            .one(&self.db)
            .await?
            .filter(|p| p.user_id == user_id)
            .ok_or(AppError::NotFound("Meal plan".into()))?;

        meal_plan_slot::Entity::delete_many()
            .filter(meal_plan_slot::Column::MealPlanId.eq(plan.id))
            .exec(&self.db)
            .await?;

        meal_plan::Entity::delete_by_id(plan.id)
            .exec(&self.db)
            .await?;

        Ok(())
    }

    /// Swap the recipe in a slot (or mark it as a flex day by passing recipe_id=null)
    pub async fn swap_slot(
        &self,
        user_id: Uuid,
        plan_id: i64,
        slot_id: i64,
        recipe_id: Option<i64>,
        flex_type: Option<String>,
        energy_level: Option<String>,
    ) -> Result<serde_json::Value, AppError> {
        meal_plan::Entity::find_by_id(plan_id)
            .one(&self.db)
            .await?
            .filter(|p| p.user_id == user_id)
            .ok_or(AppError::NotFound("Meal plan".into()))?;

        let slot = meal_plan_slot::Entity::find_by_id(slot_id)
            .one(&self.db)
            .await?
            .filter(|s| s.meal_plan_id == plan_id)
            .ok_or(AppError::NotFound("Slot".into()))?;

        let mut active: meal_plan_slot::ActiveModel = slot.into();
        active.recipe_id = Set(recipe_id);
        active.is_flex = Set(recipe_id.is_none());
        if let Some(ft) = flex_type {
            active.flex_type = Set(Some(ft));
        }
        if let Some(el) = energy_level {
            active.energy_level = Set(Some(el));
        }
        let updated = active.update(&self.db).await?;

        Ok(serde_json::json!({
            "id": updated.id,
            "recipe_id": updated.recipe_id,
            "is_flex": updated.is_flex,
            "flex_type": updated.flex_type,
            "energy_level": updated.energy_level,
            "meal_type": updated.meal_type,
            "day_of_week": updated.day_of_week,
        }))
    }

    /// Mark a slot as a flex/relief day (clears recipe, sets flex metadata)
    pub async fn mark_slot_flex(
        &self,
        user_id: Uuid,
        plan_id: i64,
        slot_id: i64,
        flex_type: String,
        energy_level: Option<String>,
    ) -> Result<(), AppError> {
        self.swap_slot(user_id, plan_id, slot_id, None, Some(flex_type), energy_level)
            .await?;
        Ok(())
    }

    /// Weekly nutrition summary: macro totals for all slots vs user goals
    pub async fn get_nutrition_summary(
        &self,
        user_id: Uuid,
        plan_id: i64,
    ) -> Result<serde_json::Value, AppError> {
        let plan = meal_plan::Entity::find_by_id(plan_id)
            .one(&self.db)
            .await?
            .filter(|p| p.user_id == user_id)
            .ok_or(AppError::NotFound("Meal plan".into()))?;

        let slots = meal_plan_slot::Entity::find()
            .filter(meal_plan_slot::Column::MealPlanId.eq(plan.id))
            .all(&self.db)
            .await?;

        let recipe_ids: Vec<i64> = slots.iter().filter_map(|s| s.recipe_id).collect();

        let nutrition: HashMap<i64, _> = recipe_nutrition::Entity::find()
            .filter(recipe_nutrition::Column::RecipeId.is_in(recipe_ids))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|n| (n.recipe_id, n))
            .collect();

        let recipes: HashMap<i64, recipe::Model> = recipe::Entity::find()
            .filter(recipe::Column::Id.is_in(slots.iter().filter_map(|s| s.recipe_id).collect::<Vec<_>>()))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|r| (r.id, r))
            .collect();

        let mut totals = NutritionTotals::default();

        for slot in &slots {
            let Some(rid) = slot.recipe_id else { continue };
            let Some(n) = nutrition.get(&rid) else { continue };
            let servings = slot.servings_override.unwrap_or(1);
            let recipe_servings = recipes.get(&rid).map(|r| r.servings).unwrap_or(1).max(1);
            let scale = servings as f64 / recipe_servings as f64;

            totals.calories  += f64::try_from(n.calories.unwrap_or_default()).unwrap_or(0.0) * scale;
            totals.protein_g += f64::try_from(n.protein_g.unwrap_or_default()).unwrap_or(0.0) * scale;
            totals.carbs_g   += f64::try_from(n.carbs_g.unwrap_or_default()).unwrap_or(0.0) * scale;
            totals.fat_g     += f64::try_from(n.fat_g.unwrap_or_default()).unwrap_or(0.0) * scale;
            totals.fiber_g   += f64::try_from(n.fiber_g.unwrap_or_default()).unwrap_or(0.0) * scale;
        }

        let slot_count = slots.len().max(1) as f64;

        Ok(serde_json::json!({
            "week_start": plan.week_start,
            "totals": {
                "calories":  totals.calories,
                "protein_g": totals.protein_g,
                "carbs_g":   totals.carbs_g,
                "fat_g":     totals.fat_g,
                "fiber_g":   totals.fiber_g,
            },
            "daily_average": {
                "calories":  totals.calories / 7.0,
                "protein_g": totals.protein_g / 7.0,
                "carbs_g":   totals.carbs_g / 7.0,
                "fat_g":     totals.fat_g / 7.0,
                "fiber_g":   totals.fiber_g / 7.0,
            },
            "goals": {
                "calories":  DAILY_CALORIES * 7.0,
                "protein_g": DAILY_PROTEIN_G * 7.0,
                "carbs_g":   DAILY_CARBS_G * 7.0,
                "fat_g":     DAILY_FAT_G * 7.0,
                "fiber_g":   DAILY_FIBER_G * 7.0,
            },
            "percent_of_goal": {
                "calories":  (totals.calories  / (DAILY_CALORIES  * 7.0) * 100.0).round(),
                "protein_g": (totals.protein_g / (DAILY_PROTEIN_G * 7.0) * 100.0).round(),
                "carbs_g":   (totals.carbs_g   / (DAILY_CARBS_G   * 7.0) * 100.0).round(),
                "fat_g":     (totals.fat_g     / (DAILY_FAT_G     * 7.0) * 100.0).round(),
                "fiber_g":   (totals.fiber_g   / (DAILY_FIBER_G   * 7.0) * 100.0).round(),
            },
            "slots_with_data": slot_count as usize,
        }))
    }

    /// Generate shopping list: ingredients needed minus what's in inventory
    pub async fn get_shopping_list(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<serde_json::Value>, AppError> {
        use crate::entity::{ingredient, recipe_ingredient};

        let today = Utc::now().date_naive();
        let days_since_monday = today.weekday().num_days_from_monday() as i64;
        let week_start = today - chrono::Duration::days(days_since_monday);

        let plan = meal_plan::Entity::find()
            .filter(meal_plan::Column::UserId.eq(user_id))
            .filter(meal_plan::Column::WeekStart.eq(week_start))
            .one(&self.db)
            .await?;

        let Some(plan) = plan else {
            return Ok(vec![]);
        };

        let slots = meal_plan_slot::Entity::find()
            .filter(meal_plan_slot::Column::MealPlanId.eq(plan.id))
            .filter(meal_plan_slot::Column::IsCompleted.eq(false))
            .all(&self.db)
            .await?;

        let recipe_ids: Vec<i64> = slots.iter().filter_map(|s| s.recipe_id).collect();

        let required_ingredients = recipe_ingredient::Entity::find()
            .filter(recipe_ingredient::Column::RecipeId.is_in(recipe_ids))
            .all(&self.db)
            .await?;

        let mut needed: HashMap<i64, rust_decimal::Decimal> = HashMap::new();
        for ri in &required_ingredients {
            if let Some(qty) = ri.quantity_grams {
                *needed.entry(ri.ingredient_id).or_default() += qty;
            }
        }

        let inventory: HashMap<i64, rust_decimal::Decimal> =
            inventory_item::Entity::find()
                .filter(inventory_item::Column::UserId.eq(user_id))
                .all(&self.db)
                .await?
                .into_iter()
                .map(|i| (i.ingredient_id, i.quantity))
                .collect();

        let ingredient_ids: Vec<i64> = needed.keys().cloned().collect();
        let names: HashMap<i64, String> = ingredient::Entity::find()
            .filter(ingredient::Column::Id.is_in(ingredient_ids))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|i| (i.id, i.name))
            .collect();

        let mut list = Vec::new();
        for (ingredient_id, needed_qty) in &needed {
            let have = inventory.get(ingredient_id).cloned().unwrap_or_default();
            if *needed_qty > have {
                let to_buy = needed_qty - have;
                list.push(serde_json::json!({
                    "ingredient_id": ingredient_id,
                    "name": names.get(ingredient_id).cloned().unwrap_or_default(),
                    "needed_grams": needed_qty,
                    "have_grams": have,
                    "to_buy_grams": to_buy,
                    "in_inventory": have > rust_decimal::Decimal::ZERO,
                }));
            }
        }

        list.sort_by(|a, b| {
            a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or(""))
        });

        Ok(list)
    }

    /// Mark a meal plan slot as completed
    pub async fn mark_slot_complete(
        &self,
        user_id: Uuid,
        plan_id: i64,
        slot_id: i64,
    ) -> Result<(), AppError> {
        meal_plan::Entity::find_by_id(plan_id)
            .one(&self.db)
            .await?
            .filter(|p| p.user_id == user_id)
            .ok_or(AppError::NotFound("Meal plan".into()))?;

        let slot = meal_plan_slot::Entity::find_by_id(slot_id)
            .one(&self.db)
            .await?
            .ok_or(AppError::NotFound("Slot".into()))?;

        let mut active: meal_plan_slot::ActiveModel = slot.into();
        active.is_completed = Set(true);
        active.update(&self.db).await?;

        Ok(())
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    async fn plan_to_json(&self, plan: &meal_plan::Model) -> Result<serde_json::Value, AppError> {
        let slots = meal_plan_slot::Entity::find()
            .filter(meal_plan_slot::Column::MealPlanId.eq(plan.id))
            .order_by_asc(meal_plan_slot::Column::DayOfWeek)
            .all(&self.db)
            .await?;

        let recipe_ids: Vec<i64> = slots.iter().filter_map(|s| s.recipe_id).collect();
        let recipes: HashMap<i64, recipe::Model> = recipe::Entity::find()
            .filter(recipe::Column::Id.is_in(recipe_ids))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|r| (r.id, r))
            .collect();

        let slot_json: Vec<serde_json::Value> = slots
            .into_iter()
            .map(|s| {
                let r = s.recipe_id.and_then(|rid| recipes.get(&rid));
                serde_json::json!({
                    "id": s.id,
                    "day_of_week": s.day_of_week,
                    "meal_type": s.meal_type,
                    "is_completed": s.is_completed,
                    "is_flex": s.is_flex,
                    "flex_type": s.flex_type,
                    "energy_level": s.energy_level,
                    "servings": s.servings_override,
                    "recipe": r.map(|r| serde_json::json!({
                        "id": r.id,
                        "name": r.name,
                        "cuisine": r.cuisine,
                        "category": r.category,
                        "total_time_min": r.total_time_min,
                        "difficulty": r.difficulty,
                        "average_rating": r.average_rating,
                    }))
                })
            })
            .collect();

        Ok(serde_json::json!({
            "id": plan.id,
            "week_start": plan.week_start,
            "is_ai_generated": plan.is_ai_generated,
            "slots": slot_json,
        }))
    }
}

#[derive(Default)]
struct NutritionTotals {
    calories: f64,
    protein_g: f64,
    carbs_g: f64,
    fat_g: f64,
    fiber_g: f64,
}

