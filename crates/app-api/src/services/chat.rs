//! Chat Service — AI assistant powered by Ollama
//!
//! Features:
//! - Context-aware system prompt built from user's inventory, preferences, meal plan
//! - Persistent sessions and message history
//! - Full message history sent to Ollama per request (stateless LLM with stateful DB)
//! - Agentic tool-calling loop (up to MAX_TOOL_ROUNDS) using Ollama's native tools API
//! - Configurable Ollama model (defaults to "llama3.2")

use chrono::Utc;
use reqwest::Client;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, Set,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::{
    chat_message, chat_session,
    cooking_history, inventory_item, meal_plan, meal_plan_slot, recipe,
    user, ingredient,
};
use crate::services::chat_tools::{tool_definitions, ToolDispatch};
use cookest_shared::errors::AppError;

const MAX_TOOL_ROUNDS: usize = 6;

// ── Ollama API types ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct OllamaRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OllamaMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OllamaToolCall>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OllamaToolCall {
    pub function: OllamaToolCallFunction,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OllamaToolCallFunction {
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct OllamaResponse {
    message: OllamaMessage,
    #[serde(default)]
    eval_count: Option<i32>,
}

// ── Public request/response types ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub message: String,
    /// Omit to start a new session
    pub session_id: Option<i64>,
    /// Optionally pin a recipe for cooking guidance
    pub recipe_id: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub session_id: i64,
    pub message_id: i64,
    pub reply: String,
    pub tokens_used: Option<i32>,
    /// Tool names called during this response (e.g. ["search_recipes","update_meal_plan_slot"])
    pub actions_taken: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SessionListItem {
    pub id: i64,
    pub title: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct MessageItem {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

// ── Service ───────────────────────────────────────────────────────────────────

pub struct ChatService {
    db: DatabaseConnection,
    http: Client,
    ollama_url: String,
    model: String,
}

impl ChatService {
    pub fn new(db: DatabaseConnection) -> Self {
        let ollama_url = std::env::var("OLLAMA_URL")
            .unwrap_or_else(|_| "http://localhost:11434".to_string());
        let model = std::env::var("OLLAMA_MODEL")
            .unwrap_or_else(|_| "llama3.2".to_string());

        Self {
            db,
            http: Client::new(),
            ollama_url,
            model,
        }
    }

    /// Send a message — creates or continues a session, returns AI reply.
    /// Runs a tool-calling loop (up to MAX_TOOL_ROUNDS) so the AI can read
    /// and modify the user's meal plan, pantry, and recipes via natural language.
    pub async fn chat(
        &self,
        user_id: Uuid,
        req: ChatRequest,
    ) -> Result<ChatResponse, AppError> {
        let now = Utc::now().fixed_offset();

        // ── 1. Get or create session ──────────────────────────────────────────
        let session = match req.session_id {
            Some(id) => {
                chat_session::Entity::find_by_id(id)
                    .one(&self.db)
                    .await?
                    .filter(|s| s.user_id == user_id)
                    .ok_or(AppError::NotFound("Chat session".into()))?
            }
            None => {
                let title = self.generate_title(&req.message);
                let new_session = chat_session::ActiveModel {
                    user_id: Set(user_id),
                    current_recipe_id: Set(req.recipe_id),
                    title: Set(Some(title)),
                    created_at: Set(now),
                    updated_at: Set(now),
                    ..Default::default()
                };
                new_session.insert(&self.db).await?
            }
        };

        // ── 2. Build system prompt from user context ──────────────────────────
        let system_prompt = self.build_system_prompt(user_id, session.current_recipe_id).await?;

        // ── 3. Load message history for this session ──────────────────────────
        let history = chat_message::Entity::find()
            .filter(chat_message::Column::SessionId.eq(session.id))
            .order_by_asc(chat_message::Column::CreatedAt)
            .all(&self.db)
            .await?;

        // ── 4. Build initial Ollama messages array ────────────────────────────
        let mut ollama_messages: Vec<OllamaMessage> = Vec::with_capacity(history.len() + 2);

        ollama_messages.push(OllamaMessage {
            role: "system".to_string(),
            content: system_prompt,
            tool_calls: None,
        });

        for msg in &history {
            if msg.role != "system" {
                ollama_messages.push(OllamaMessage {
                    role: msg.role.clone(),
                    content: msg.content.clone(),
                    tool_calls: None,
                });
            }
        }

        ollama_messages.push(OllamaMessage {
            role: "user".to_string(),
            content: req.message.clone(),
            tool_calls: None,
        });

        // ── 5. Save user message to DB ────────────────────────────────────────
        let user_msg = chat_message::ActiveModel {
            session_id: Set(session.id),
            role: Set("user".to_string()),
            content: Set(req.message.clone()),
            tokens_used: Set(None),
            created_at: Set(now),
            ..Default::default()
        };
        user_msg.insert(&self.db).await?;

        // ── 6. Tool-calling loop ──────────────────────────────────────────────
        let tools = ToolDispatch::new(self.db.clone());
        let mut reply = String::new();
        let mut tokens: Option<i32> = None;
        let mut actions_taken: Vec<String> = Vec::new();

        for _round in 0..MAX_TOOL_ROUNDS {
            let ollama_req = OllamaRequest {
                model: self.model.clone(),
                messages: ollama_messages.clone(),
                stream: false,
                tools: Some(tool_definitions()),
            };

            let resp = self
                .http
                .post(format!("{}/api/chat", self.ollama_url))
                .json(&ollama_req)
                .send()
                .await
                .map_err(|e| {
                    tracing::error!("Ollama request failed: {}", e);
                    AppError::Internal("AI service unavailable".into())
                })?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                tracing::error!("Ollama error {}: {}", status, body);
                return Err(AppError::Internal("AI service returned an error".into()));
            }

            let ollama_resp: OllamaResponse = resp.json().await.map_err(|e| {
                tracing::error!("Failed to parse Ollama response: {}", e);
                AppError::Internal("Failed to parse AI response".into())
            })?;

            tokens = ollama_resp.eval_count;

            match ollama_resp.message.tool_calls {
                Some(ref calls) if !calls.is_empty() => {
                    // Add assistant message (with tool_calls) so Ollama tracks context
                    ollama_messages.push(ollama_resp.message.clone());

                    for call in calls {
                        let name = &call.function.name;
                        let args = call.function.arguments.clone();
                        tracing::info!("AI tool call: {} {:?}", name, args);
                        actions_taken.push(name.clone());

                        let result = tools.execute(user_id, name, args).await;

                        ollama_messages.push(OllamaMessage {
                            role: "tool".to_string(),
                            content: result,
                            tool_calls: None,
                        });
                    }
                }
                _ => {
                    // No tool calls — this is the final response
                    reply = ollama_resp.message.content.clone();
                    break;
                }
            }
        }

        if reply.is_empty() {
            reply = "I've completed the requested actions.".to_string();
        }

        // ── 7. Save assistant reply to DB ─────────────────────────────────────
        let reply_msg = chat_message::ActiveModel {
            session_id: Set(session.id),
            role: Set("assistant".to_string()),
            content: Set(reply.clone()),
            tokens_used: Set(tokens),
            created_at: Set(Utc::now().fixed_offset()),
            ..Default::default()
        };
        let saved_reply = reply_msg.insert(&self.db).await?;

        let mut active_session: chat_session::ActiveModel = session.into();
        active_session.updated_at = Set(Utc::now().fixed_offset());
        active_session.update(&self.db).await?;

        Ok(ChatResponse {
            session_id: saved_reply.session_id,
            message_id: saved_reply.id,
            reply,
            tokens_used: tokens,
            actions_taken,
        })
    }

    /// List all chat sessions for a user
    pub async fn list_sessions(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<SessionListItem>, AppError> {
        let sessions = chat_session::Entity::find()
            .filter(chat_session::Column::UserId.eq(user_id))
            .order_by_desc(chat_session::Column::UpdatedAt)
            .all(&self.db)
            .await?;

        Ok(sessions
            .into_iter()
            .map(|s| SessionListItem {
                id: s.id,
                title: s.title,
                created_at: s.created_at.to_rfc3339(),
                updated_at: s.updated_at.to_rfc3339(),
            })
            .collect())
    }

    /// Get all messages in a session
    pub async fn get_messages(
        &self,
        user_id: Uuid,
        session_id: i64,
    ) -> Result<Vec<MessageItem>, AppError> {
        chat_session::Entity::find_by_id(session_id)
            .one(&self.db)
            .await?
            .filter(|s| s.user_id == user_id)
            .ok_or(AppError::NotFound("Chat session".into()))?;

        let messages = chat_message::Entity::find()
            .filter(chat_message::Column::SessionId.eq(session_id))
            .filter(chat_message::Column::Role.ne("system"))
            .order_by_asc(chat_message::Column::CreatedAt)
            .all(&self.db)
            .await?;

        Ok(messages
            .into_iter()
            .map(|m| MessageItem {
                id: m.id,
                role: m.role,
                content: m.content,
                created_at: m.created_at.to_rfc3339(),
            })
            .collect())
    }

    /// Delete a session (and all its messages via cascade)
    pub async fn delete_session(
        &self,
        user_id: Uuid,
        session_id: i64,
    ) -> Result<(), AppError> {
        let session = chat_session::Entity::find_by_id(session_id)
            .one(&self.db)
            .await?
            .filter(|s| s.user_id == user_id)
            .ok_or(AppError::NotFound("Chat session".into()))?;

        chat_session::Entity::delete_by_id(session.id)
            .exec(&self.db)
            .await?;

        Ok(())
    }

    // ── Context builder ───────────────────────────────────────────────────────

    async fn build_system_prompt(
        &self,
        user_id: Uuid,
        recipe_id: Option<i64>,
    ) -> Result<String, AppError> {
        let mut ctx = String::with_capacity(4096);

        ctx.push_str(
            "You are Cookest AI, a personal cooking assistant. \
             You help users plan meals, cook recipes, manage their kitchen inventory, \
             and make healthy food choices. Be concise, practical, and friendly.\n\n"
        );

        // User profile (dietary restrictions, allergies)
        if let Some(user) = user::Entity::find_by_id(user_id).one(&self.db).await? {
            if let Some(list) = user.dietary_restrictions {
                if !list.is_empty() {
                    ctx.push_str(&format!("User dietary restrictions: {}.\n", list.join(", ")));
                }
            }
            if let Some(list) = user.allergies {
                if !list.is_empty() {
                    ctx.push_str(&format!("User allergies (NEVER suggest these): {}.\n", list.join(", ")));
                }
            }
            ctx.push_str(&format!("Household size: {} people.\n", user.household_size));
        }

        // Current inventory
        let inventory = inventory_item::Entity::find()
            .filter(inventory_item::Column::UserId.eq(user_id))
            .all(&self.db)
            .await?;

        if !inventory.is_empty() {
            let ingredient_ids: Vec<i64> = inventory.iter().map(|i| i.ingredient_id).collect();
            let ingredients: std::collections::HashMap<i64, String> = ingredient::Entity::find()
                .filter(ingredient::Column::Id.is_in(ingredient_ids))
                .all(&self.db)
                .await?
                .into_iter()
                .map(|i| (i.id, i.name))
                .collect();

            let items: Vec<String> = inventory
                .iter()
                .map(|i| {
                    let name = ingredients
                        .get(&i.ingredient_id)
                        .cloned()
                        .unwrap_or_else(|| i.custom_name.clone().unwrap_or_default());
                    format!("{} ({} {})", name, i.quantity, i.unit)
                })
                .collect();

            ctx.push_str(&format!(
                "\nCurrent pantry/fridge inventory:\n{}\n",
                items.join(", ")
            ));

            let expiry_threshold = Utc::now().date_naive() + chrono::Duration::days(5);
            let expiring: Vec<String> = inventory
                .iter()
                .filter(|i| {
                    i.expiry_date.map(|d| d <= expiry_threshold).unwrap_or(false)
                })
                .map(|i| {
                    ingredients
                        .get(&i.ingredient_id)
                        .cloned()
                        .unwrap_or_else(|| i.custom_name.clone().unwrap_or_default())
                })
                .collect();

            if !expiring.is_empty() {
                ctx.push_str(&format!(
                    "⚠️  Expiring within 5 days (prioritise using these): {}.\n",
                    expiring.join(", ")
                ));
            }
        } else {
            ctx.push_str("\nThe user's inventory is currently empty.\n");
        }

        // This week's meal plan summary
        let today = Utc::now().date_naive();
        use chrono::Datelike;
        let days_since_monday = today.weekday().num_days_from_monday() as i64;
        let week_start = today - chrono::Duration::days(days_since_monday);

        if let Some(plan) = meal_plan::Entity::find()
            .filter(meal_plan::Column::UserId.eq(user_id))
            .filter(meal_plan::Column::WeekStart.eq(week_start))
            .one(&self.db)
            .await?
        {
            let slots = meal_plan_slot::Entity::find()
                .filter(meal_plan_slot::Column::MealPlanId.eq(plan.id))
                .all(&self.db)
                .await?;

            if !slots.is_empty() {
                let recipe_ids: Vec<i64> = slots.iter().filter_map(|s| s.recipe_id).collect();
                let recipes: std::collections::HashMap<i64, String> = recipe::Entity::find()
                    .filter(recipe::Column::Id.is_in(recipe_ids))
                    .all(&self.db)
                    .await?
                    .into_iter()
                    .map(|r| (r.id, r.name))
                    .collect();

                let day_names = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
                let plan_lines: Vec<String> = slots
                    .iter()
                    .map(|s| {
                        let day = day_names[s.day_of_week as usize % 7];
                        let recipe_name = s.recipe_id
                            .and_then(|rid| recipes.get(&rid))
                            .cloned()
                            .unwrap_or_else(|| "Flex day".to_string());
                        let done = if s.is_completed { "✓" } else { "" };
                        format!("{} {}: {} {}", day, s.meal_type, recipe_name, done)
                    })
                    .collect();

                ctx.push_str(&format!("\nThis week's meal plan:\n{}\n", plan_lines.join("\n")));
            }
        }

        // Recent cooking history (last 3)
        let recent_cooked = cooking_history::Entity::find()
            .filter(cooking_history::Column::UserId.eq(user_id))
            .order_by_desc(cooking_history::Column::CookedAt)
            .all(&self.db)
            .await?;

        let recent_cooked: Vec<_> = recent_cooked.into_iter().take(3).collect();
        if !recent_cooked.is_empty() {
            let recipe_ids: Vec<i64> = recent_cooked.iter().map(|h| h.recipe_id).collect();
            let names: std::collections::HashMap<i64, String> = recipe::Entity::find()
                .filter(recipe::Column::Id.is_in(recipe_ids))
                .all(&self.db)
                .await?
                .into_iter()
                .map(|r| (r.id, r.name))
                .collect();

            let history_str: Vec<String> = recent_cooked
                .iter()
                .map(|h| names.get(&h.recipe_id).cloned().unwrap_or_default())
                .collect();

            ctx.push_str(&format!(
                "\nRecently cooked: {}.\n",
                history_str.join(", ")
            ));
        }

        // If pinned to a specific recipe, include its details
        if let Some(rid) = recipe_id {
            if let Some(r) = recipe::Entity::find_by_id(rid).one(&self.db).await? {
                ctx.push_str(&format!(
                    "\nThe user is currently cooking: '{}' (serves {}, ~{} min, {} difficulty).\
                     \nFocus your assistance on helping them cook this dish successfully.\n",
                    r.name,
                    r.servings,
                    r.total_time_min.unwrap_or(0),
                    r.difficulty.as_deref().unwrap_or("unknown"),
                ));
            }
        }

        // ── Capabilities and behaviour rules ──────────────────────────────────
        ctx.push_str(
            "\n## YOUR CAPABILITIES\n\
             \n\
             You have access to tools that let you read and modify the user's data:\n\
             - search_recipes: Find recipes by cuisine, dietary needs, time, etc.\n\
             - get_meal_plan: See the full current week meal plan\n\
             - update_meal_plan_slot: Change a recipe in a specific meal slot\n\
             - mark_meal_completed: Mark a meal as cooked\n\
             - get_pantry: See inventory\n\
             - add_to_pantry / remove_from_pantry: Manage inventory\n\
             - get_recipe_details: Full recipe info including ingredients and nutrition\n\
             \n\
             ## HOW TO HANDLE MEAL PLAN CHANGES\n\
             \n\
             When a user asks to change something in their meal plan:\n\
             1. FIRST call get_meal_plan to see the current state\n\
             2. THEN call search_recipes with the requested cuisine/type/constraint\n\
             3. Pick the BEST matching recipe (prefer one that matches their pantry)\n\
             4. Tell the user EXACTLY what you're going to change: \"I'll replace [current recipe] on [day] [meal_type] with [new recipe]. Shall I go ahead?\"\n\
             5. ONLY call update_meal_plan_slot after the user confirms (says \"yes\", \"sure\", \"go ahead\", \"do it\", etc.) OR if their original message was clearly an unambiguous direct command (e.g. \"change Wednesday dinner to pasta\")\n\
             \n\
             ## DAY AND MEAL MAPPING\n\
             - Monday = 0, Tuesday = 1, Wednesday = 2, Thursday = 3, Friday = 4, Saturday = 5, Sunday = 6\n\
             - Meal types: breakfast, lunch, dinner, snack\n\
             - \"supper\" = dinner, \"brekkie\"/\"breakfast\" = breakfast, \"brunch\" = lunch, \"tea\" = snack\n\
             \n\
             ## BEHAVIOUR RULES\n\
             - Be FLEXIBLE: if the user debates your suggestion, take their feedback seriously and offer alternatives\n\
             - Never be stuck: always have a plan B and C\n\
             - If a search returns nothing, broaden the criteria and try again\n\
             - NEVER make up recipe names — always use IDs returned by search_recipes\n\
             - For the meal plan, always work with real data from get_meal_plan\n\
             - If no meal plan exists for the week, tell the user they need to generate one first\n\
             - Respect user's dietary restrictions and allergies at ALL times (they are in your context)\n\
             - Keep responses concise but warm — max 3-4 sentences for routine changes, more for complex questions\n"
        );

        Ok(ctx)
    }

    /// Auto-generate a session title from the first message
    fn generate_title(&self, message: &str) -> String {
        let truncated: String = message.chars().take(50).collect();
        if message.len() > 50 {
            format!("{}…", truncated)
        } else {
            truncated
        }
    }
}
