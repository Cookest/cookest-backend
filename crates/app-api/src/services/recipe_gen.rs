//! AI-powered recipe generation service.
//!
//! Implements a **generate → score → refine** loop backed by a local Ollama
//! LLM.  Each iteration asks Ollama to generate a recipe, a second pass
//! scores it for palatability and nutrition balance, and if the score is
//! below [`SCORE_THRESHOLD`] the loop retries with the judge’s suggestions
//! as a critique prompt.  The loop is capped at [`MAX_ITERATIONS`] to bound
//! latency regardless of LLM quality.

use reqwest::Client;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

use crate::entity::{ingredient, inventory_item, user};
use crate::handlers::browse::FoodApiClient;
use crate::services::IngredientService;
use cookest_shared::errors::AppError;

/// Maximum number of generate → score → refine cycles before returning the
/// best result found so far.  3 was chosen as the sweet spot: empirically
/// a second pass improves quality noticeably, but a third pass yields
/// diminishing returns while adding ~3 × LLM latency.
const MAX_ITERATIONS: usize = 3;

/// Minimum weighted score (0–10) a generated recipe must reach before the
/// loop terminates early.  7.0 represents "good enough to serve a real user"
/// in our internal palatability rubric; below that, the recipe is retried
/// with the judge’s suggestions as a critique.
const SCORE_THRESHOLD: f32 = 7.0;

// ── Internal Ollama types ─────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
struct OllamaRecipeOutput {
    name: String,
    description: String,
    cuisine: String,
    difficulty: String,
    prep_minutes: u32,
    cook_minutes: u32,
    servings: u32,
    ingredients: Vec<GenIngredient>,
    steps: Vec<String>,
    macros_per_serving: GenMacros,
    tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaScoreOutput {
    palatability_score: f32,
    palatability_reason: String,
    suggestions: Vec<String>,
}

// ── Public request / response types ──────────────────────────────────────────

/// Request body for the recipe-generation endpoint.
#[derive(Debug, Deserialize)]
pub struct GenerateRecipeRequest {
    /// Restrict ingredients to what's in the user's pantry
    pub use_pantry: bool,
    /// Optional cuisine hint, e.g. "Portuguese", "Italian"
    pub cuisine_hint: Option<String>,
    /// Maximum total minutes (prep + cook)
    pub max_minutes: Option<u32>,
}

/// A single ingredient entry in the generated recipe, tagged with whether
/// it already exists in the user’s pantry so the UI can highlight
/// missing items.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GenIngredient {
    pub name: String,
    pub quantity: f64,
    pub unit: String,
    pub is_pantry_item: bool,
    /// Resolved master-catalog id, filled in after generation by matching `name`
    /// against the catalog. `None` means no catalog match (the recipe can't be
    /// saved until the user picks a substitute). Not produced by the LLM.
    #[serde(default)]
    pub ingredient_id: Option<i64>,
}

/// Macronutrient breakdown per serving, computed by the LLM and passed
/// through as-is (not independently verified against a nutrition DB).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GenMacros {
    pub calories: f64,
    pub protein_g: f64,
    pub carbs_g: f64,
    pub fat_g: f64,
    pub fiber_g: f64,
}

#[derive(Debug, Serialize)]
pub struct RecipeScore {
    /// Weighted overall score (0–10)
    pub overall: f32,
    /// LLM palatability judge (0–10, weight 50%)
    pub palatability: f32,
    /// Computed from macros (0–10, weight 30%)
    pub nutrition_balance: f32,
    /// Preference/allergy match (0–10, weight 20%)
    pub preference_match: f32,
    pub palatability_reason: String,
    /// How many generation iterations were needed
    pub iterations: u32,
}

/// Full recipe generation result returned to the client, including all
/// scoring dimensions so the UI can optionally display quality signals.
#[derive(Debug, Serialize)]
pub struct GeneratedRecipeResponse {
    pub name: String,
    pub description: String,
    pub cuisine: String,
    pub difficulty: String,
    pub prep_minutes: u32,
    pub cook_minutes: u32,
    pub servings: u32,
    pub ingredients: Vec<GenIngredient>,
    pub steps: Vec<String>,
    pub macros_per_serving: GenMacros,
    pub tags: Vec<String>,
    pub score: RecipeScore,
}

/// A generated ingredient the client wants to persist. `ingredient_id` must be
/// present (resolved against the catalog) — recipes only reference preset items.
#[derive(Debug, Deserialize)]
pub struct SaveGenIngredient {
    pub name: String,
    pub ingredient_id: Option<i64>,
    pub quantity: Option<f64>,
    pub unit: Option<String>,
}

/// Request body for saving a generated recipe into the user's own recipes.
#[derive(Debug, Deserialize)]
pub struct SaveGeneratedRecipeRequest {
    pub name: String,
    pub description: Option<String>,
    pub cuisine: Option<String>,
    pub difficulty: Option<String>,
    pub prep_minutes: Option<i32>,
    pub cook_minutes: Option<i32>,
    pub servings: Option<i32>,
    pub ingredients: Vec<SaveGenIngredient>,
    pub steps: Vec<String>,
    pub is_public: Option<bool>,
}

// ── Service ───────────────────────────────────────────────────────────────────

/// Generates personalised recipes via Ollama using the user’s pantry,
/// dietary restrictions, and cooking skill as context.
///
/// One instance is created per request (or shared as `Arc`) because it
/// holds only immutable config alongside the connection pool.
pub struct RecipeGenService {
    db: DatabaseConnection,
    food_api_client: FoodApiClient,
    http: Client,
    ollama_url: String,
    model: String,
}

impl RecipeGenService {
    /// Construct a service instance.
    ///
    /// Reads `OLLAMA_URL` (default `http://localhost:11434`) and
    /// `OLLAMA_MODEL` (default `llama3.1:8b`) from the environment so the
    /// same binary can target different models per environment.
    /// The HTTP client uses a 180-second timeout to accommodate slow
    /// local hardware that may take minutes for a full generation pass.
    pub fn new(db: DatabaseConnection, food_api_client: FoodApiClient) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(180))
            .build()
            .unwrap_or_default();
        Self {
            db,
            food_api_client,
            http,
            ollama_url: std::env::var("OLLAMA_URL")
                .unwrap_or_else(|_| "http://localhost:11434".to_string()),
            model: std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "llama3.1:8b".to_string()),
        }
    }

    /// Ingredient resolver for mapping generated names to catalog ids.
    fn ingredient_service(&self) -> IngredientService {
        IngredientService::new(self.db.clone(), self.food_api_client.clone())
    }

    /// Best-effort: resolve each generated ingredient name to a master-catalog id
    /// so the recipe can later be saved referencing preset ingredients. Unmatched
    /// items keep `ingredient_id = None` (surfaced to the user, blocked on save).
    async fn map_ingredients_to_catalog(&self, ingredients: &mut [GenIngredient]) {
        let ing_svc = self.ingredient_service();
        for ing in ingredients.iter_mut() {
            match ing_svc.resolve_by_name(&ing.name).await {
                Ok(id) => ing.ingredient_id = id,
                Err(e) => {
                    tracing::warn!("ingredient mapping failed for '{}': {}", ing.name, e);
                    ing.ingredient_id = None;
                }
            }
        }
    }

    /// Generate a personalised recipe for the given user.
    ///
    /// Runs the generate → score → refine loop: if the first attempt scores
    /// below [`SCORE_THRESHOLD`], the judge’s suggestions are fed back as a
    /// critique and a new recipe is requested.  Stops early on a passing
    /// score or after [`MAX_ITERATIONS`] attempts.
    ///
    /// # Errors
    /// Returns `AppError` if the DB lookup, Ollama HTTP call, or JSON
    /// parsing fails at any point.
    pub async fn generate(
        &self,
        user_id: Uuid,
        req: GenerateRecipeRequest,
    ) -> Result<GeneratedRecipeResponse, AppError> {
        let (user_ctx, pantry_items, dietary_restrictions, allergies, skill, household_size) =
            self.fetch_user_context(user_id, req.use_pantry).await?;

        let mut iteration = 0u32;
        let mut critique: Option<String> = None;
        let mut suggestions: Vec<String> = Vec::new();

        loop {
            iteration += 1;

            let gen_prompt = self.build_generation_prompt(
                &pantry_items,
                &dietary_restrictions,
                &allergies,
                &skill,
                household_size,
                req.use_pantry,
                &req.cuisine_hint,
                req.max_minutes,
                critique.as_deref(),
                &suggestions,
            );

            let recipe = self.call_generate(&gen_prompt).await?;

            let nutrition_balance = compute_nutrition_score(&recipe.macros_per_serving);
            let preference_match = compute_preference_score(
                &recipe.tags,
                &recipe.ingredients,
                &dietary_restrictions,
                &allergies,
            );

            let score_prompt = self.build_score_prompt(&recipe, &user_ctx);
            let score_output = self.call_score(&score_prompt).await?;

            let palatability = score_output.palatability_score.clamp(0.0, 10.0);
            let overall = palatability * 0.5 + nutrition_balance * 0.3 + preference_match * 0.2;

            tracing::info!(
                iteration,
                overall,
                palatability,
                nutrition_balance,
                preference_match,
                "Recipe gen iteration score"
            );

            if overall >= SCORE_THRESHOLD || iteration >= MAX_ITERATIONS as u32 {
                let mut ingredients = recipe.ingredients;
                // Map generated ingredient names onto the preset catalog so the
                // result can be saved with real ingredient ids.
                self.map_ingredients_to_catalog(&mut ingredients).await;
                return Ok(GeneratedRecipeResponse {
                    name: recipe.name,
                    description: recipe.description,
                    cuisine: recipe.cuisine,
                    difficulty: recipe.difficulty,
                    prep_minutes: recipe.prep_minutes,
                    cook_minutes: recipe.cook_minutes,
                    servings: recipe.servings,
                    ingredients,
                    steps: recipe.steps,
                    macros_per_serving: recipe.macros_per_serving,
                    tags: recipe.tags,
                    score: RecipeScore {
                        overall,
                        palatability,
                        nutrition_balance,
                        preference_match,
                        palatability_reason: score_output.palatability_reason,
                        iterations: iteration,
                    },
                });
            }

            critique = Some(score_output.palatability_reason);
            suggestions = score_output.suggestions;
        }
    }

    // ── Ollama calls ──────────────────────────────────────────────────────────

    async fn call_generate(&self, prompt: &str) -> Result<OllamaRecipeOutput, AppError> {
        let payload = serde_json::json!({
            "model": self.model,
            "prompt": prompt,
            "stream": false,
            "format": "json"
        });

        let resp = self
            .http
            .post(format!("{}/api/generate", self.ollama_url))
            .json(&payload)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Ollama generate request failed: {e}")))?;

        let body: serde_json::Value = resp.json().await.map_err(|e| {
            AppError::Internal(format!("Ollama generate response parse failed: {e}"))
        })?;

        let raw = body["response"]
            .as_str()
            .ok_or_else(|| AppError::Internal("Ollama returned empty response field".into()))?;

        serde_json::from_str(raw).map_err(|e| {
            AppError::Internal(format!("Recipe JSON schema parse failed: {e}\nRaw: {raw}"))
        })
    }

    async fn call_score(&self, prompt: &str) -> Result<OllamaScoreOutput, AppError> {
        let payload = serde_json::json!({
            "model": self.model,
            "prompt": prompt,
            "stream": false,
            "format": "json"
        });

        let resp = self
            .http
            .post(format!("{}/api/generate", self.ollama_url))
            .json(&payload)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Ollama score request failed: {e}")))?;

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| AppError::Internal(format!("Ollama score response parse failed: {e}")))?;

        let raw = body["response"]
            .as_str()
            .ok_or_else(|| AppError::Internal("Ollama score returned empty response".into()))?;

        serde_json::from_str(raw).or_else(|e| {
            tracing::warn!("Score JSON parse failed ({e}), using neutral fallback");
            Ok(OllamaScoreOutput {
                palatability_score: 7.0,
                palatability_reason: "Score estimated — evaluation model parse error.".into(),
                suggestions: vec![],
            })
        })
    }

    // ── Context helpers ───────────────────────────────────────────────────────

    async fn fetch_user_context(
        &self,
        user_id: Uuid,
        use_pantry: bool,
    ) -> Result<(String, Vec<String>, Vec<String>, Vec<String>, String, u32), AppError> {
        let mut user_ctx = String::new();
        let mut dietary_restrictions = Vec::new();
        let mut allergies = Vec::new();
        let mut skill = "intermediate".to_string();
        let mut household_size = 2u32;

        if let Some(user) = user::Entity::find_by_id(user_id).one(&self.db).await? {
            dietary_restrictions = user.dietary_restrictions.unwrap_or_default();
            allergies = user.allergies.unwrap_or_default();
            if let Some(s) = &user.cooking_skill_level {
                skill = s.clone();
            }
            household_size = user.household_size as u32;

            if !dietary_restrictions.is_empty() {
                user_ctx.push_str(&format!(
                    "Dietary restrictions: {}. ",
                    dietary_restrictions.join(", ")
                ));
            }
            if !allergies.is_empty() {
                user_ctx.push_str(&format!(
                    "Allergies (forbidden): {}. ",
                    allergies.join(", ")
                ));
            }
            user_ctx.push_str(&format!(
                "Cooking skill: {skill}. Household: {household_size} people."
            ));
        }

        let pantry_items = if use_pantry {
            let inventory = inventory_item::Entity::find()
                .filter(inventory_item::Column::UserId.eq(user_id))
                .all(&self.db)
                .await?;

            if !inventory.is_empty() {
                let ids: Vec<i64> = inventory.iter().map(|i| i.ingredient_id).collect();
                ingredient::Entity::find()
                    .filter(ingredient::Column::Id.is_in(ids))
                    .all(&self.db)
                    .await?
                    .into_iter()
                    .map(|i| i.name)
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        Ok((
            user_ctx,
            pantry_items,
            dietary_restrictions,
            allergies,
            skill,
            household_size,
        ))
    }

    // ── Prompt builders ───────────────────────────────────────────────────────

    fn build_generation_prompt(
        &self,
        pantry_items: &[String],
        dietary_restrictions: &[String],
        allergies: &[String],
        skill: &str,
        household_size: u32,
        use_pantry: bool,
        cuisine_hint: &Option<String>,
        max_minutes: Option<u32>,
        critique: Option<&str>,
        suggestions: &[String],
    ) -> String {
        let mut prompt = String::with_capacity(2048);

        prompt.push_str(
            "You are a professional chef and nutritionist. \
             Generate exactly ONE recipe as a valid JSON object.\n\n",
        );

        prompt.push_str("=== CONSTRAINTS ===\n");
        if let Some(cuisine) = cuisine_hint {
            prompt.push_str(&format!("Cuisine style: {cuisine}\n"));
        }
        if let Some(max) = max_minutes {
            prompt.push_str(&format!(
                "Maximum total cooking time: {max} minutes (prep_minutes + cook_minutes must be <= {max})\n"
            ));
        }
        prompt.push_str(&format!("Target servings: {household_size} people\n"));
        prompt.push_str(&format!("Cook's skill level: {skill}\n"));

        if !dietary_restrictions.is_empty() {
            prompt.push_str(&format!(
                "Dietary restrictions (MUST strictly follow): {}\n",
                dietary_restrictions.join(", ")
            ));
        }
        if !allergies.is_empty() {
            prompt.push_str(&format!(
                "Allergens (NEVER include in any ingredient): {}\n",
                allergies.join(", ")
            ));
        }

        if use_pantry && !pantry_items.is_empty() {
            prompt.push_str(&format!(
                "\n=== AVAILABLE PANTRY ===\n{}\n\
                 The recipe MUST primarily use these ingredients. \
                 You may add small amounts of common basics (salt, pepper, oil, water) \
                 but do not introduce major new ingredients not in this list.\n",
                pantry_items.join(", ")
            ));
        }

        if let Some(critique_text) = critique {
            prompt.push_str(&format!(
                "\n=== IMPROVEMENT REQUIRED ===\n\
                 The previous attempt scored below threshold.\n\
                 Critique: \"{critique_text}\"\n"
            ));
            if !suggestions.is_empty() {
                prompt.push_str(&format!(
                    "Specific improvements needed: {}\n",
                    suggestions.join("; ")
                ));
            }
            prompt.push_str("Generate a BETTER recipe that directly addresses these issues.\n");
        }

        prompt.push_str(
            r#"
=== REQUIRED JSON SCHEMA ===
Return ONLY a JSON object — no markdown, no extra text, no explanation. Use this exact schema:
{
  "name": "Recipe Name",
  "description": "2-3 sentence appetizing description of the dish",
  "cuisine": "Portuguese",
  "difficulty": "beginner",
  "prep_minutes": 10,
  "cook_minutes": 20,
  "servings": 2,
  "ingredients": [
    {"name": "ingredient name", "quantity": 100.0, "unit": "g", "is_pantry_item": true}
  ],
  "steps": [
    "Step 1: detailed instruction with technique.",
    "Step 2: next instruction."
  ],
  "macros_per_serving": {
    "calories": 350.0,
    "protein_g": 20.0,
    "carbs_g": 45.0,
    "fat_g": 12.0,
    "fiber_g": 5.0
  },
  "tags": ["vegetarian", "quick", "dinner"]
}

Rules:
- difficulty: one of "beginner", "intermediate", "advanced"
- tags: include dietary labels (vegetarian, vegan, gluten-free, etc.) and meal type (breakfast, lunch, dinner, snack, dessert)
- All numeric fields must be numbers (not strings)
- steps: at least 3 clear, detailed cooking steps
- macros_per_serving: realistic nutritional estimates per single serving
"#,
        );

        prompt
    }

    fn build_score_prompt(&self, recipe: &OllamaRecipeOutput, user_ctx: &str) -> String {
        let recipe_json = serde_json::to_string_pretty(recipe).unwrap_or_default();
        format!(
            r#"You are a food critic and nutritionist evaluating a recipe.

User profile: {user_ctx}

Recipe to evaluate:
{recipe_json}

Evaluate this recipe and return ONLY a JSON object — no markdown, no extra text:
{{
  "palatability_score": 8.5,
  "palatability_reason": "One honest sentence explaining the score, focusing on flavor balance, ingredient harmony, and eating appeal.",
  "suggestions": ["Specific improvement 1", "Specific improvement 2"]
}}

Scoring guide for palatability_score (1–10):
- 9–10: Exceptional — would impress at a dinner party
- 7–8: Very good — solid home cooking people enjoy
- 5–6: Decent but unremarkable — technically correct but lacks excitement
- 3–4: Has problems — bland, odd combinations, or poor balance
- 1–2: Would not eat — fundamentally flawed

Consider: flavor balance, ingredient harmony, cultural authenticity, visual appeal, and overall desirability.
Be honest but fair. Two suggestions for improvement are always welcome even for high scores."#
        )
    }
}

// ── Scoring helpers ───────────────────────────────────────────────────────────

fn compute_nutrition_score(macros: &GenMacros) -> f32 {
    if macros.calories <= 0.0 {
        return 5.0;
    }
    let cal = macros.calories;
    let protein_pct = (macros.protein_g * 4.0) / cal * 100.0;
    let fat_pct = (macros.fat_g * 9.0) / cal * 100.0;
    let carbs_pct = (macros.carbs_g * 4.0) / cal * 100.0;

    let mut score = 10.0f32;

    // Protein: ideal 15–35 %
    if protein_pct < 8.0 {
        score -= 2.5;
    } else if protein_pct < 15.0 {
        score -= 1.0;
    }

    // Fat: ideal 20–40 %
    if fat_pct > 55.0 {
        score -= 2.5;
    } else if fat_pct > 45.0 {
        score -= 1.0;
    }

    // Carbs: flag very high carb ratio
    if carbs_pct > 80.0 {
        score -= 1.5;
    }

    // Calorie density per serving
    if cal > 900.0 {
        score -= 2.0;
    } else if cal > 700.0 {
        score -= 1.0;
    }

    // Fibre bonus/penalty
    if macros.fiber_g >= 5.0 {
        score += 0.5;
    } else if macros.fiber_g < 2.0 {
        score -= 0.5;
    }

    score.clamp(0.0, 10.0)
}

fn compute_preference_score(
    tags: &[String],
    ingredients: &[GenIngredient],
    restrictions: &[String],
    allergies: &[String],
) -> f32 {
    // Hard fail on allergy violation
    let ingredient_names: Vec<String> = ingredients.iter().map(|i| i.name.to_lowercase()).collect();
    for allergy in allergies {
        let a = allergy.to_lowercase();
        if ingredient_names.iter().any(|n| n.contains(&a)) {
            return 0.0;
        }
    }

    // No restrictions means neutral-good match
    if restrictions.is_empty() {
        return 8.0;
    }

    let tags_lower: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();
    let matched = restrictions
        .iter()
        .filter(|r| {
            let r = r.to_lowercase();
            tags_lower.iter().any(|t| t.contains(&r))
        })
        .count();

    let ratio = matched as f32 / restrictions.len() as f32;
    (5.0 + ratio * 5.0).clamp(0.0, 10.0)
}
