use actix_web::{post, web, HttpResponse};
use rust_decimal::Decimal;
use std::str::FromStr;
use std::sync::Arc;

use crate::middleware::auth::AuthenticatedUser;
use crate::models::recipe::{
    CreateRecipeIngredientRequest, CreateRecipeRequest, CreateRecipeStepRequest,
};
use crate::services::recipe_gen::{
    GenerateRecipeRequest, RecipeGenService, SaveGeneratedRecipeRequest,
};
use crate::services::RecipeService;
use cookest_shared::errors::AppError;

/// POST /api/recipe-ai/generate
///
/// Generate a new recipe using AI.  The generate→score→refine loop runs
/// silently on the server (up to 3 iterations) and returns the best result.
/// Generated ingredients are mapped to the master catalog (`ingredient_id`).
///
/// NB: this lives under `/api/recipe-ai` (not `/api/recipes`) because the public
/// `/api/recipes` scope is registered first and would otherwise shadow it.
#[post("/api/recipe-ai/generate")]
pub async fn generate_recipe(
    user: AuthenticatedUser,
    service: web::Data<Arc<RecipeGenService>>,
    body: web::Json<GenerateRecipeRequest>,
) -> Result<HttpResponse, AppError> {
    let result = service.generate(user.id, body.into_inner()).await?;
    Ok(HttpResponse::Ok().json(result))
}

/// POST /api/recipe-ai/save
///
/// Persist a generated recipe into the user's own recipes. Every ingredient must
/// already be resolved to a preset catalog id (recipes never store free text);
/// any unmatched ingredient must be substituted first.
#[post("/api/recipe-ai/save")]
pub async fn save_generated_recipe(
    user: AuthenticatedUser,
    recipe_service: web::Data<Arc<RecipeService>>,
    body: web::Json<SaveGeneratedRecipeRequest>,
) -> Result<HttpResponse, AppError> {
    let body = body.into_inner();

    let unmatched: Vec<String> = body
        .ingredients
        .iter()
        .filter(|i| i.ingredient_id.is_none())
        .map(|i| i.name.clone())
        .collect();
    if !unmatched.is_empty() {
        return Err(AppError::BadRequest(format!(
            "These ingredients are not in the catalog and need a substitute before saving: {}",
            unmatched.join(", ")
        )));
    }

    let ingredients: Vec<CreateRecipeIngredientRequest> = body
        .ingredients
        .into_iter()
        .filter_map(|i| {
            i.ingredient_id.map(|id| CreateRecipeIngredientRequest {
                ingredient_id: id,
                quantity: i
                    .quantity
                    .and_then(|q| Decimal::from_str(&q.to_string()).ok()),
                unit: i.unit,
                notes: None,
            })
        })
        .collect();

    let steps: Vec<CreateRecipeStepRequest> = body
        .steps
        .into_iter()
        .map(|s| CreateRecipeStepRequest {
            instruction: s,
            duration_min: None,
        })
        .collect();

    let req = CreateRecipeRequest {
        name: body.name,
        description: body.description,
        cuisine: body.cuisine,
        category: None,
        difficulty: body.difficulty,
        servings: body.servings,
        prep_time_min: body.prep_minutes,
        cook_time_min: body.cook_minutes,
        is_vegetarian: None,
        is_vegan: None,
        is_gluten_free: None,
        is_dairy_free: None,
        is_nut_free: None,
        is_public: body.is_public,
        ingredients: Some(ingredients),
        steps: Some(steps),
    };

    let result = recipe_service.create_recipe(user.id, req).await?;
    Ok(HttpResponse::Created().json(result))
}

pub fn configure_recipe_gen(cfg: &mut web::ServiceConfig) {
    cfg.service(generate_recipe);
    cfg.service(save_generated_recipe);
}
