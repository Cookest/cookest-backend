//! Chat Tools — AI-callable actions scoped to the authenticated user.
//!
//! SECURITY: Every tool function receives `user_id: Uuid` derived from the
//! validated JWT token. The AI cannot specify a different user_id.
//! All database reads/writes are filtered by this user_id.

use sea_orm::{ColumnTrait, Condition, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::entity::{ingredient, recipe, recipe_ingredient, recipe_nutrition, recipe_step};
use crate::services::{InventoryService, MealPlanService};
use cookest_shared::errors::AppError;

// ── Tool definitions ──────────────────────────────────────────────────────────

pub fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "search_recipes",
                "description": "Search for recipes by name, cuisine, dietary requirements, or cooking time. Always use this before suggesting a recipe to update the meal plan.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query":        { "type": "string",  "description": "Text search (name/keyword)" },
                        "cuisine":      { "type": "string",  "description": "e.g. Italian, Mexican, Japanese" },
                        "meal_type":    { "type": "string",  "description": "breakfast, lunch, dinner, or snack" },
                        "max_time_min": { "type": "integer", "description": "Max total cooking time in minutes" },
                        "vegetarian":   { "type": "boolean" },
                        "vegan":        { "type": "boolean" },
                        "gluten_free":  { "type": "boolean" },
                        "dairy_free":   { "type": "boolean" },
                        "limit":        { "type": "integer", "description": "Max results (default 5, max 8)" }
                    },
                    "required": []
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "get_meal_plan",
                "description": "Get the user's current week meal plan. Use this first to understand what's already planned before making changes.",
                "parameters": { "type": "object", "properties": {}, "required": [] }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "update_meal_plan_slot",
                "description": "Replace the recipe in a specific meal plan slot with a new recipe. Use search_recipes first to find a suitable recipe_id.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "day_of_week": { "type": "integer", "description": "0=Monday, 1=Tuesday, 2=Wednesday, 3=Thursday, 4=Friday, 5=Saturday, 6=Sunday" },
                        "meal_type":   { "type": "string",  "description": "breakfast, lunch, dinner, or snack" },
                        "recipe_id":   { "type": "integer", "description": "The ID of the recipe to assign" }
                    },
                    "required": ["day_of_week", "meal_type", "recipe_id"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "mark_meal_completed",
                "description": "Mark a meal as cooked/completed for today.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "day_of_week": { "type": "integer" },
                        "meal_type":   { "type": "string" }
                    },
                    "required": ["day_of_week", "meal_type"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "get_pantry",
                "description": "Get the user's current pantry/fridge inventory.",
                "parameters": { "type": "object", "properties": {}, "required": [] }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "add_to_pantry",
                "description": "Add an ingredient to the user's pantry.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "name":             { "type": "string" },
                        "quantity":         { "type": "number" },
                        "unit":             { "type": "string", "description": "g, kg, ml, l, pieces, etc." },
                        "storage_location": { "type": "string", "description": "fridge, freezer, pantry, etc." },
                        "expiry_date":      { "type": "string", "description": "YYYY-MM-DD format" }
                    },
                    "required": ["name", "quantity", "unit"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "remove_from_pantry",
                "description": "Remove an item from the user's pantry by its inventory item ID.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "item_id": { "type": "integer", "description": "The inventory item ID from get_pantry" }
                    },
                    "required": ["item_id"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "clear_meal_plan",
                "description": "Remove ALL recipes from the user's current week meal plan, leaving it completely empty so they can start fresh. Use this when the user explicitly asks to clear, reset, or start over their meal plan.",
                "parameters": { "type": "object", "properties": {}, "required": [] }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "get_recipe_details",
                "description": "Get full details of a recipe including ingredients, nutrition, and cooking steps.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "recipe_id": { "type": "integer" }
                    },
                    "required": ["recipe_id"]
                }
            }
        }),
    ]
}

// ── Tool dispatch ─────────────────────────────────────────────────────────────

pub struct ToolDispatch {
    db: DatabaseConnection,
}

impl ToolDispatch {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn execute(&self, user_id: Uuid, name: &str, args: Value) -> String {
        match name {
            "search_recipes"       => self.search_recipes(args).await,
            "get_meal_plan"        => self.get_meal_plan(user_id).await,
            "update_meal_plan_slot"=> self.update_meal_plan_slot(user_id, args).await,
            "mark_meal_completed"  => self.mark_meal_completed(user_id, args).await,
            "get_pantry"           => self.get_pantry(user_id).await,
            "add_to_pantry"        => self.add_to_pantry(user_id, args).await,
            "remove_from_pantry"   => self.remove_from_pantry(user_id, args).await,
            "clear_meal_plan"      => self.clear_meal_plan(user_id).await,
            "get_recipe_details"   => self.get_recipe_details(args).await,
            _ => format!("{{\"error\": \"Unknown tool: {}\"}}", name),
        }
    }

    // ── Individual tool implementations ──────────────────────────────────────

    async fn search_recipes(&self, args: Value) -> String {
        let mut condition = Condition::all();

        if let Some(q) = args["query"].as_str() {
            if !q.is_empty() {
                condition = condition.add(recipe::Column::Name.like(format!("%{}%", q)));
            }
        }
        if let Some(cuisine) = args["cuisine"].as_str() {
            if !cuisine.is_empty() {
                condition = condition.add(recipe::Column::Cuisine.eq(cuisine));
            }
        }
        if let Some(meal_type) = args["meal_type"].as_str() {
            if !meal_type.is_empty() {
                condition = condition.add(recipe::Column::Category.eq(meal_type));
            }
        }
        if let Some(max_time) = args["max_time_min"].as_i64() {
            condition = condition.add(recipe::Column::TotalTimeMin.lte(max_time as i32));
        }
        if args["vegetarian"].as_bool() == Some(true) {
            condition = condition.add(recipe::Column::IsVegetarian.eq(true));
        }
        if args["vegan"].as_bool() == Some(true) {
            condition = condition.add(recipe::Column::IsVegan.eq(true));
        }
        if args["gluten_free"].as_bool() == Some(true) {
            condition = condition.add(recipe::Column::IsGlutenFree.eq(true));
        }
        if args["dairy_free"].as_bool() == Some(true) {
            condition = condition.add(recipe::Column::IsDairyFree.eq(true));
        }

        let limit = args["limit"].as_u64().unwrap_or(5).min(8) as usize;

        let recipes = match recipe::Entity::find()
            .filter(condition)
            .order_by_asc(recipe::Column::Name)
            .all(&self.db)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("search_recipes DB error: {}", e);
                return json!({"error": "Failed to search recipes"}).to_string();
            }
        };

        let results: Vec<Value> = recipes
            .into_iter()
            .take(limit)
            .map(|r| {
                json!({
                    "id": r.id,
                    "name": r.name,
                    "cuisine": r.cuisine,
                    "total_time_min": r.total_time_min,
                    "difficulty": r.difficulty,
                })
            })
            .collect();

        serde_json::to_string(&results).unwrap_or_else(|_| "[]".to_string())
    }

    async fn get_meal_plan(&self, user_id: Uuid) -> String {
        let svc = MealPlanService::new(self.db.clone());
        match svc.get_current_week_plan(user_id).await {
            Ok(None) => json!({
                "status": "no_plan",
                "message": "No meal plan for this week. The user can generate one."
            })
            .to_string(),
            Ok(Some(plan)) => {
                const DAY_NAMES: [&str; 7] =
                    ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
                let slots: Vec<Value> = plan
                    .slots
                    .iter()
                    .map(|s| {
                        json!({
                            "day": DAY_NAMES[s.day_of_week as usize % 7],
                            "day_of_week": s.day_of_week,
                            "meal_type": s.meal_type,
                            "recipe_id": s.recipe_id,
                            "recipe_name": s.recipe_name,
                            "is_completed": s.is_completed,
                        })
                    })
                    .collect();
                serde_json::to_string(&slots).unwrap_or_else(|_| "[]".to_string())
            }
            Err(e) => {
                tracing::error!("get_meal_plan tool error: {}", e);
                json!({"error": "Failed to get meal plan"}).to_string()
            }
        }
    }

    async fn update_meal_plan_slot(&self, user_id: Uuid, args: Value) -> String {
        let day_of_week = match args["day_of_week"].as_i64() {
            Some(d) => d as i16,
            None => return json!({"status": "error", "message": "Missing day_of_week"}).to_string(),
        };
        let meal_type = match args["meal_type"].as_str() {
            Some(m) => m.to_string(),
            None => return json!({"status": "error", "message": "Missing meal_type"}).to_string(),
        };
        let recipe_id = match args["recipe_id"].as_i64() {
            Some(r) => r,
            None => return json!({"status": "error", "message": "Missing recipe_id"}).to_string(),
        };

        const DAY_NAMES: [&str; 7] =
            ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

        let svc = MealPlanService::new(self.db.clone());
        match svc.update_slot_recipe(user_id, day_of_week, &meal_type, recipe_id).await {
            Ok(recipe_name) => json!({
                "status": "success",
                "message": format!(
                    "Updated {} {} to {}",
                    DAY_NAMES[day_of_week as usize % 7],
                    meal_type,
                    recipe_name
                ),
                "recipe_name": recipe_name,
            })
            .to_string(),
            Err(AppError::NotFound(ref msg)) if msg.contains("week") => json!({
                "status": "error",
                "message": "No meal plan for this week. Please generate a meal plan first."
            })
            .to_string(),
            Err(e) => {
                tracing::error!("update_meal_plan_slot tool error: {}", e);
                json!({"status": "error", "message": format!("{}", e)}).to_string()
            }
        }
    }

    async fn mark_meal_completed(&self, user_id: Uuid, args: Value) -> String {
        let day_of_week = match args["day_of_week"].as_i64() {
            Some(d) => d as i16,
            None => return json!({"status": "error", "message": "Missing day_of_week"}).to_string(),
        };
        let meal_type = match args["meal_type"].as_str() {
            Some(m) => m.to_string(),
            None => return json!({"status": "error", "message": "Missing meal_type"}).to_string(),
        };

        let svc = MealPlanService::new(self.db.clone());
        match svc.mark_slot_completed(user_id, day_of_week, &meal_type).await {
            Ok(()) => json!({"status": "success", "message": "Meal marked as completed"}).to_string(),
            Err(AppError::NotFound(ref msg)) if msg.contains("week") => json!({
                "status": "error",
                "message": "No meal plan for this week."
            })
            .to_string(),
            Err(AppError::NotFound(ref msg)) => {
                json!({"status": "error", "message": format!("Not found: {}", msg)}).to_string()
            }
            Err(e) => {
                tracing::error!("mark_meal_completed tool error: {}", e);
                json!({"status": "error", "message": format!("{}", e)}).to_string()
            }
        }
    }

    async fn get_pantry(&self, user_id: Uuid) -> String {
        let svc = InventoryService::new(self.db.clone());
        match svc.list(user_id).await {
            Ok(items) if items.is_empty() => {
                json!({"status": "empty", "message": "Pantry is empty."}).to_string()
            }
            Ok(items) => {
                let result: Vec<Value> = items
                    .iter()
                    .map(|item| {
                        let display_name = item
                            .custom_name
                            .as_deref()
                            .unwrap_or(&item.ingredient_name);
                        json!({
                            "id": item.id,
                            "name": display_name,
                            "quantity": item.quantity.to_string(),
                            "unit": item.unit,
                            "expiry_date": item.expiry_date.map(|d| d.to_string()),
                            "days_until_expiry": item.days_until_expiry,
                            "expiry_warning": item.expiry_warning,
                        })
                    })
                    .collect();
                serde_json::to_string(&result).unwrap_or_else(|_| "[]".to_string())
            }
            Err(e) => {
                tracing::error!("get_pantry tool error: {}", e);
                json!({"error": "Failed to get pantry"}).to_string()
            }
        }
    }

    async fn add_to_pantry(&self, user_id: Uuid, args: Value) -> String {
        let name = match args["name"].as_str() {
            Some(n) => n.to_string(),
            None => return json!({"status": "error", "message": "Missing name"}).to_string(),
        };
        let quantity = match args["quantity"].as_f64() {
            Some(q) => q,
            None => return json!({"status": "error", "message": "Missing quantity"}).to_string(),
        };
        let unit = match args["unit"].as_str() {
            Some(u) => u.to_string(),
            None => return json!({"status": "error", "message": "Missing unit"}).to_string(),
        };
        let storage_location = args["storage_location"].as_str().map(|s| s.to_string());
        let expiry_date = args["expiry_date"]
            .as_str()
            .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());

        let svc = InventoryService::new(self.db.clone());
        match svc
            .quick_add(user_id, name.clone(), quantity, unit.clone(), storage_location, expiry_date)
            .await
        {
            Ok(item) => json!({
                "status": "success",
                "message": format!("Added {} {} {} to pantry", quantity, unit, name),
                "id": item.id,
            })
            .to_string(),
            Err(e) => {
                tracing::error!("add_to_pantry tool error: {}", e);
                json!({"status": "error", "message": format!("{}", e)}).to_string()
            }
        }
    }

    async fn remove_from_pantry(&self, user_id: Uuid, args: Value) -> String {
        let item_id = match args["item_id"].as_i64() {
            Some(id) => id,
            None => return json!({"status": "error", "message": "Missing item_id"}).to_string(),
        };

        let svc = InventoryService::new(self.db.clone());
        match svc.delete(user_id, item_id).await {
            Ok(()) => json!({"status": "success", "message": "Item removed from pantry"}).to_string(),
            Err(AppError::NotFound(_)) => {
                json!({"status": "error", "message": "Item not found in your pantry"}).to_string()
            }
            Err(e) => {
                tracing::error!("remove_from_pantry tool error: {}", e);
                json!({"status": "error", "message": format!("{}", e)}).to_string()
            }
        }
    }

    async fn get_recipe_details(&self, args: Value) -> String {
        let recipe_id = match args["recipe_id"].as_i64() {
            Some(id) => id,
            None => return json!({"error": "Missing recipe_id"}).to_string(),
        };

        let r = match recipe::Entity::find_by_id(recipe_id).one(&self.db).await {
            Ok(Some(r)) => r,
            Ok(None) => return json!({"error": "Recipe not found"}).to_string(),
            Err(e) => {
                tracing::error!("get_recipe_details DB error: {}", e);
                return json!({"error": "Failed to get recipe"}).to_string();
            }
        };

        // Load recipe ingredients with names
        let recipe_ings = match recipe_ingredient::Entity::find()
            .filter(recipe_ingredient::Column::RecipeId.eq(recipe_id))
            .order_by_asc(recipe_ingredient::Column::DisplayOrder)
            .all(&self.db)
            .await
        {
            Ok(ri) => ri,
            Err(_) => vec![],
        };

        let ingredient_ids: Vec<i64> = recipe_ings.iter().map(|ri| ri.ingredient_id).collect();
        let ingredient_names: std::collections::HashMap<i64, String> = if ingredient_ids.is_empty() {
            std::collections::HashMap::new()
        } else {
            match ingredient::Entity::find()
                .filter(ingredient::Column::Id.is_in(ingredient_ids))
                .all(&self.db)
                .await
            {
                Ok(ings) => ings.into_iter().map(|i| (i.id, i.name)).collect(),
                Err(_) => std::collections::HashMap::new(),
            }
        };

        let ingredients: Vec<Value> = recipe_ings
            .iter()
            .map(|ri| {
                let name = ingredient_names
                    .get(&ri.ingredient_id)
                    .cloned()
                    .unwrap_or_default();
                json!({
                    "name": name,
                    "quantity": ri.quantity.map(|q| q.to_string()),
                    "unit": ri.unit,
                })
            })
            .collect();

        // Load cooking steps
        let steps = match recipe_step::Entity::find()
            .filter(recipe_step::Column::RecipeId.eq(recipe_id))
            .order_by_asc(recipe_step::Column::StepNumber)
            .all(&self.db)
            .await
        {
            Ok(s) => s,
            Err(_) => vec![],
        };

        let steps_json: Vec<Value> = steps
            .iter()
            .map(|s| {
                json!({
                    "step_number": s.step_number,
                    "instruction": s.instruction,
                })
            })
            .collect();

        // Load nutrition
        let nutrition = match recipe_nutrition::Entity::find()
            .filter(recipe_nutrition::Column::RecipeId.eq(recipe_id))
            .one(&self.db)
            .await
        {
            Ok(n) => n,
            Err(_) => None,
        };

        let nutrition_json = nutrition.map(|n| {
            json!({
                "calories":  n.calories.map(|c| c.to_string()),
                "protein_g": n.protein_g.map(|p| p.to_string()),
                "carbs_g":   n.carbs_g.map(|c| c.to_string()),
                "fat_g":     n.fat_g.map(|f| f.to_string()),
            })
        });

        json!({
            "id": r.id,
            "name": r.name,
            "cuisine": r.cuisine,
            "total_time_min": r.total_time_min,
            "servings": r.servings,
            "difficulty": r.difficulty,
            "ingredients": ingredients,
            "nutrition": nutrition_json,
            "steps": steps_json,
        })
        .to_string()
    }

    async fn clear_meal_plan(&self, user_id: Uuid) -> String {
        use chrono::{Datelike, Duration, Utc};
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        use crate::entity::{meal_plan, meal_plan_slot};

        let today = Utc::now().date_naive();
        let days_since_monday = today.weekday().num_days_from_monday() as i64;
        let week_start = today - Duration::days(days_since_monday);

        let plan = meal_plan::Entity::find()
            .filter(meal_plan::Column::UserId.eq(user_id))
            .filter(meal_plan::Column::WeekStart.eq(week_start))
            .one(&self.db)
            .await;

        match plan {
            Ok(Some(p)) => {
                let res = meal_plan_slot::Entity::delete_many()
                    .filter(meal_plan_slot::Column::MealPlanId.eq(p.id))
                    .exec(&self.db)
                    .await;

                match res {
                    Ok(r) => json!({
                        "success": true,
                        "slots_removed": r.rows_affected
                    })
                    .to_string(),
                    Err(e) => json!({"error": e.to_string()}).to_string(),
                }
            }
            Ok(None) => json!({
                "success": true,
                "slots_removed": 0,
                "note": "No meal plan exists for this week"
            })
            .to_string(),
            Err(e) => json!({"error": e.to_string()}).to_string(),
        }
    }
}
