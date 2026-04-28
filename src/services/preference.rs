//! Preference Service — online learning of user taste preferences
//!
//! Uses incremental gradient updates to learn what each user likes.
//! Formula: new_weight = old_weight + learning_rate × (signal - old_weight)
//!
//! Signals:
//!   +1.0  = 5-star rating / added to favourites
//!   +0.6  = 4-star rating / cooked recipe
//!   +0.2  = 3-star rating
//!   -0.2  = 2-star rating
//!   -0.6  = 1-star rating
//!   -0.3  = skipped suggestion

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set,
};
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::entity::{
    user_preference, recipe, recipe_ingredient, ingredient, recipe_nutrition,
};
use crate::errors::AppError;

/// How quickly the model adapts (0.0 = never updates, 1.0 = instant overwrite)
const LEARNING_RATE: f64 = 0.1;

/// Signals that drive learning
#[derive(Debug, Clone, Copy)]
pub enum PreferenceSignal {
    Rated(i16),     // 1–5 star rating
    Cooked,         // User completed cooking this recipe
    Favourited,     // Added to favourites
    Skipped,        // User skipped a suggestion for this recipe
}

impl PreferenceSignal {
    fn value(self) -> f64 {
        match self {
            PreferenceSignal::Rated(5) => 1.0,
            PreferenceSignal::Rated(4) => 0.6,
            PreferenceSignal::Rated(3) => 0.2,
            PreferenceSignal::Rated(2) => -0.2,
            PreferenceSignal::Rated(1) => -0.6,
            PreferenceSignal::Rated(_) => 0.0,
            PreferenceSignal::Cooked => 0.5,
            PreferenceSignal::Favourited => 0.8,
            PreferenceSignal::Skipped => -0.3,
        }
    }
}

pub struct PreferenceService {
    db: DatabaseConnection,
}

impl PreferenceService {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// Called on any user interaction with a recipe — updates preference vector
    pub async fn record_interaction(
        &self,
        user_id: Uuid,
        recipe_id: i64,
        signal: PreferenceSignal,
    ) -> Result<(), AppError> {
        let signal_value = signal.value();

        // Load or create the user's preference record
        let pref = self.get_or_create(user_id).await?;

        // Load the recipe's features
        let recipe = recipe::Entity::find_by_id(recipe_id)
            .one(&self.db)
            .await?
            .ok_or(AppError::NotFound("Recipe".into()))?;

        // Load recipe's ingredients
        let recipe_ingredients = recipe_ingredient::Entity::find()
            .filter(recipe_ingredient::Column::RecipeId.eq(recipe_id))
            .all(&self.db)
            .await?;

        let ingredient_ids: Vec<i64> = recipe_ingredients.iter().map(|ri| ri.ingredient_id).collect();
        let ingredients = ingredient::Entity::find()
            .filter(ingredient::Column::Id.is_in(ingredient_ids))
            .all(&self.db)
            .await?;

        // Load recipe nutrition
        let nutrition = recipe_nutrition::Entity::find()
            .filter(recipe_nutrition::Column::RecipeId.eq(recipe_id))
            .one(&self.db)
            .await?;

        // Parse existing weights from JSONB
        let mut cuisine_weights = json_to_map(&pref.cuisine_weights);
        let mut ingredient_weights = json_to_map(&pref.ingredient_weights);
        let mut macro_bias = json_to_map(&pref.macro_bias);
        let mut difficulty_weights = json_to_map(&pref.difficulty_weights);

        // ── Update cuisine weight ─────────────────────────────────────────────
        if let Some(cuisine) = &recipe.cuisine {
            update_weight(&mut cuisine_weights, cuisine, signal_value);
        }

        // ── Update difficulty weight ──────────────────────────────────────────
        if let Some(difficulty) = &recipe.difficulty {
            update_weight(&mut difficulty_weights, difficulty, signal_value);
        }

        // ── Update ingredient weights ─────────────────────────────────────────
        for ing in &ingredients {
            update_weight(&mut ingredient_weights, &ing.name, signal_value * 0.5);
        }

        // ── Update macro bias ─────────────────────────────────────────────────
        if let Some(n) = &nutrition {
            let total_calories = n.calories.unwrap_or_default();
            if total_calories > rust_decimal::Decimal::ZERO {
                let tc = f64::try_from(total_calories).unwrap_or(1.0);
                let protein = f64::try_from(n.protein_g.unwrap_or_default()).unwrap_or(0.0);
                let carbs = f64::try_from(n.carbs_g.unwrap_or_default()).unwrap_or(0.0);
                let fat = f64::try_from(n.fat_g.unwrap_or_default()).unwrap_or(0.0);

                // Normalise macros as 0.0–1.0 ratio of total calories
                let p_ratio = (protein * 4.0) / tc;
                let c_ratio = (carbs * 4.0) / tc;
                let f_ratio = (fat * 9.0) / tc;

                // Update macro bias towards the recipe's ratios, weighted by signal
                let bias_signal_p = if signal_value > 0.0 { p_ratio } else { -p_ratio };
                let bias_signal_c = if signal_value > 0.0 { c_ratio } else { -c_ratio };
                let bias_signal_f = if signal_value > 0.0 { f_ratio } else { -f_ratio };

                update_weight(&mut macro_bias, "protein", bias_signal_p * signal_value.abs());
                update_weight(&mut macro_bias, "carbs", bias_signal_c * signal_value.abs());
                update_weight(&mut macro_bias, "fat", bias_signal_f * signal_value.abs());
            }
        }

        // ── Update preferred_time_min (weighted rolling average) ─────────────
        let new_time = if let Some(time) = recipe.total_time_min {
            let count = pref.interaction_count as f64;
            let old = pref.preferred_time_min as f64;
            // Positive signals pull preferred time towards this recipe's time
            if signal_value > 0.0 {
                ((old * count + time as f64) / (count + 1.0)) as i32
            } else {
                pref.preferred_time_min
            }
        } else {
            pref.preferred_time_min
        };

        // ── Persist updated weights ───────────────────────────────────────────
        let now = Utc::now().fixed_offset();
        let mut active: user_preference::ActiveModel = pref.into();
        active.cuisine_weights = Set(map_to_json(cuisine_weights));
        active.ingredient_weights = Set(map_to_json(ingredient_weights));
        active.macro_bias = Set(map_to_json(macro_bias));
        active.difficulty_weights = Set(map_to_json(difficulty_weights));
        active.preferred_time_min = Set(new_time);
        active.interaction_count = Set({
            let count_val = match &active.interaction_count {
                sea_orm::ActiveValue::Set(v) => *v,
                sea_orm::ActiveValue::Unchanged(v) => *v,
                _ => 0,
            };
            count_val + 1
        });
        active.updated_at = Set(now);
        active.update(&self.db).await?;

        Ok(())
    }

    /// Score a recipe against a user's preference vector (0.0–1.0)
    pub async fn score_recipe_for_user(
        &self,
        user_id: Uuid,
        recipe_id: i64,
    ) -> Result<f64, AppError> {
        let pref = self.get_or_create(user_id).await?;

        let recipe = recipe::Entity::find_by_id(recipe_id)
            .one(&self.db)
            .await?
            .ok_or(AppError::NotFound("Recipe".into()))?;

        let cuisine_weights = json_to_map(&pref.cuisine_weights);
        let ingredient_weights = json_to_map(&pref.ingredient_weights);
        let difficulty_weights = json_to_map(&pref.difficulty_weights);

        let mut score = 0.0_f64;
        let mut components = 0;

        // Cuisine score
        if let Some(cuisine) = &recipe.cuisine {
            if let Some(w) = cuisine_weights.get(cuisine).and_then(|v| v.as_f64()) {
                score += w;
                components += 1;
            }
        }

        // Difficulty score
        if let Some(difficulty) = &recipe.difficulty {
            if let Some(w) = difficulty_weights.get(difficulty).and_then(|v| v.as_f64()) {
                score += w;
                components += 1;
            }
        }

        // Time preference score (how close to preferred time)
        if let Some(total_time) = recipe.total_time_min {
            let diff = (total_time - pref.preferred_time_min).abs() as f64;
            let time_score = 1.0 - (diff / 60.0).min(1.0); // max penalty at 60 min diff
            score += time_score;
            components += 1;
        }

        // Ingredient score (average of ingredient weights in this recipe)
        let recipe_ingredients = recipe_ingredient::Entity::find()
            .filter(recipe_ingredient::Column::RecipeId.eq(recipe_id))
            .all(&self.db)
            .await?;

        if !recipe_ingredients.is_empty() {
            let ingredient_ids: Vec<i64> = recipe_ingredients.iter().map(|ri| ri.ingredient_id).collect();
            let ingredients = ingredient::Entity::find()
                .filter(ingredient::Column::Id.is_in(ingredient_ids))
                .all(&self.db)
                .await?;

            let ing_score: f64 = ingredients.iter()
                .filter_map(|ing| ingredient_weights.get(&ing.name)?.as_f64())
                .sum::<f64>() / ingredients.len().max(1) as f64;

            score += ing_score;
            components += 1;
        }

        // Normalise to 0.0–1.0
        let raw = if components > 0 { score / components as f64 } else { 0.0 };
        Ok(raw.clamp(0.0, 1.0))
    }

    /// Get or create a user's preference record (initialised with neutral weights)
    /// Get a user's current taste preference weights
    pub async fn get_preferences(&self, user_id: Uuid) -> Result<serde_json::Value, AppError> {
        let pref = self.get_or_create(user_id).await?;
        Ok(serde_json::json!({
            "user_id": pref.user_id,
            "cuisine_weights": pref.cuisine_weights,
            "ingredient_weights": pref.ingredient_weights,
            "macro_bias": pref.macro_bias,
            "difficulty_weights": pref.difficulty_weights,
            "preferred_time_min": pref.preferred_time_min,
            "interaction_count": pref.interaction_count,
            "updated_at": pref.updated_at,
        }))
    }

    /// Reset all preference weights to neutral (0.0) — effectively re-bootstraps learning
    pub async fn reset_preferences(&self, user_id: Uuid) -> Result<(), AppError> {
        let now = Utc::now().fixed_offset();
        // Delete and re-insert to cleanly reset
        user_preference::Entity::delete_by_id(user_id)
            .exec(&self.db)
            .await
            .ok(); // ignore not-found

        let new_pref = user_preference::ActiveModel {
            user_id: Set(user_id),
            cuisine_weights: Set(json!({})),
            ingredient_weights: Set(json!({})),
            macro_bias: Set(json!({"protein": 0.0, "carbs": 0.0, "fat": 0.0})),
            difficulty_weights: Set(json!({"easy": 0.0, "medium": 0.0, "hard": 0.0})),
            preferred_time_min: Set(30),
            interaction_count: Set(0),
            updated_at: Set(now),
        };
        new_pref.insert(&self.db).await?;
        Ok(())
    }

    pub async fn get_or_create(
        &self,
        user_id: Uuid,
    ) -> Result<user_preference::Model, AppError> {
        if let Some(pref) = user_preference::Entity::find_by_id(user_id)
            .one(&self.db)
            .await?
        {
            return Ok(pref);
        }

        // Bootstrap with neutral (0.0) weights
        let now = Utc::now().fixed_offset();
        let new_pref = user_preference::ActiveModel {
            user_id: Set(user_id),
            cuisine_weights: Set(json!({})),
            ingredient_weights: Set(json!({})),
            macro_bias: Set(json!({"protein": 0.0, "carbs": 0.0, "fat": 0.0})),
            difficulty_weights: Set(json!({"easy": 0.0, "medium": 0.0, "hard": 0.0})),
            preferred_time_min: Set(30),
            interaction_count: Set(0),
            updated_at: Set(now),
        };

        Ok(new_pref.insert(&self.db).await?)
    }
}

// ── Helper functions ──────────────────────────────────────────────────────────

/// Apply incremental gradient update
fn update_weight(map: &mut Map<String, Value>, key: &str, signal: f64) {
    let old = map.get(key).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let new = old + LEARNING_RATE * (signal - old);
    // Clamp to [-1.0, 1.0]
    let clamped = new.clamp(-1.0, 1.0);
    map.insert(key.to_string(), Value::from(
        (clamped * 1000.0).round() / 1000.0 // round to 3 decimal places
    ));
}

fn json_to_map(val: &Value) -> Map<String, Value> {
    val.as_object().cloned().unwrap_or_default()
}

fn map_to_json(map: Map<String, Value>) -> Value {
    Value::Object(map)
}
