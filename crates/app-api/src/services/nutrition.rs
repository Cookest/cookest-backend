//! Nutrition AI service — RAG-grounded "what to buy" and recipe-idea suggestions.
//!
//! Combines the user's pantry, dietary profile, and health goals with retrieved
//! nutrition-book passages (RAG), then asks the local Ollama model for concrete,
//! cited suggestions. Allergies are always passed as hard constraints.

use reqwest::Client;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

use crate::entity::{ingredient, inventory_item, user};
use crate::services::embeddings::EmbeddingService;
use cookest_shared::errors::AppError;

#[derive(Debug, Deserialize)]
pub struct WhatToBuyRequest {
    /// Optional focus, e.g. "more protein", "iron", "heart-healthy"
    pub goal: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RecipeSuggestRequest {
    pub count: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BuySuggestion {
    pub item: String,
    pub reason: String,
    #[serde(default)]
    pub nutrient: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WhatToBuyResponse {
    pub suggestions: Vec<BuySuggestion>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RecipeIdea {
    pub name: String,
    pub description: String,
    pub why: String,
    #[serde(default)]
    pub key_nutrients: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RecipeSuggestionsResponse {
    pub suggestions: Vec<RecipeIdea>,
}

pub struct NutritionService {
    db: DatabaseConnection,
    http: Client,
    ollama_url: String,
    model: String,
    embeddings: EmbeddingService,
}

impl NutritionService {
    pub fn new(db: DatabaseConnection) -> Self {
        let ollama_url = std::env::var("OLLAMA_URL")
            .unwrap_or_else(|_| "http://localhost:11434".to_string());
        let model = std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "llama3.1:8b".to_string());
        let embed_model =
            std::env::var("OLLAMA_EMBED_MODEL").unwrap_or_else(|_| "nomic-embed-text".to_string());
        let http = Client::builder()
            .timeout(Duration::from_secs(180))
            .build()
            .unwrap_or_default();
        Self {
            embeddings: EmbeddingService::new(db.clone(), ollama_url.clone(), embed_model),
            db,
            http,
            ollama_url,
            model,
        }
    }

    /// (pantry item names, dietary restrictions, allergies, health goals)
    async fn profile_and_pantry(
        &self,
        user_id: Uuid,
    ) -> Result<(Vec<String>, Vec<String>, Vec<String>, Vec<String>), AppError> {
        let (mut dietary, mut allergies, mut goals) = (vec![], vec![], vec![]);
        if let Some(u) = user::Entity::find_by_id(user_id).one(&self.db).await? {
            dietary = u.dietary_restrictions.unwrap_or_default();
            allergies = u.allergies.unwrap_or_default();
            goals = u.health_goals.unwrap_or_default();
        }
        let inv = inventory_item::Entity::find()
            .filter(inventory_item::Column::UserId.eq(user_id))
            .all(&self.db)
            .await?;
        let pantry = if inv.is_empty() {
            vec![]
        } else {
            let ids: Vec<i64> = inv.iter().map(|i| i.ingredient_id).collect();
            ingredient::Entity::find()
                .filter(ingredient::Column::Id.is_in(ids))
                .all(&self.db)
                .await?
                .into_iter()
                .map(|i| i.name)
                .collect()
        };
        Ok((pantry, dietary, allergies, goals))
    }

    async fn knowledge_block(&self, query: &str) -> String {
        let top_k: u64 = std::env::var("RAG_TOP_K")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5);
        let chunks = self.embeddings.search(query, top_k).await;
        if chunks.is_empty() {
            return String::new();
        }
        let mut s = String::from("\n=== NUTRITION KNOWLEDGE (cite sources when used) ===\n");
        for c in &chunks {
            let src = c.title.clone().unwrap_or_else(|| c.source.clone());
            s.push_str(&format!("[{}] {}\n", src, c.content.trim()));
        }
        s
    }

    async fn call_json(&self, prompt: &str) -> Result<serde_json::Value, AppError> {
        let payload = serde_json::json!({
            "model": self.model, "prompt": prompt, "stream": false, "format": "json"
        });
        let resp = self
            .http
            .post(format!("{}/api/generate", self.ollama_url))
            .json(&payload)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Ollama request failed: {e}")))?;
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| AppError::Internal(format!("Ollama parse failed: {e}")))?;
        let raw = body["response"]
            .as_str()
            .ok_or_else(|| AppError::Internal("empty Ollama response".into()))?;
        serde_json::from_str(raw)
            .map_err(|e| AppError::Internal(format!("AI JSON parse failed: {e}")))
    }

    /// Suggest groceries to buy that fill nutritional gaps given the pantry/goals.
    pub async fn what_to_buy(
        &self,
        user_id: Uuid,
        goal: Option<String>,
    ) -> Result<WhatToBuyResponse, AppError> {
        let (pantry, dietary, allergies, goals) = self.profile_and_pantry(user_id).await?;
        let goal_text = goal.unwrap_or_else(|| "balanced, nutritious eating".to_string());
        let knowledge = self.knowledge_block(&format!("what foods to buy for {goal_text}")).await;
        let prompt = format!(
            "You are a registered-dietitian-style shopping advisor.\n\
             Pantry already has: {pantry}.\n\
             Dietary restrictions (must respect): {dietary}.\n\
             Allergies (NEVER suggest): {allergies}.\n\
             Health goals: {goals}.\n\
             Shopping focus: {goal_text}.\n{knowledge}\n\
             Suggest 5-8 specific grocery items that fill nutritional gaps given the pantry and goals, \
             favouring nutrient density and variety. Include at least one nutritious item the user likely hasn't tried. \
             Return ONLY JSON: {{\"suggestions\":[{{\"item\":\"\",\"reason\":\"\",\"nutrient\":\"\"}}]}}",
            pantry = list_or_none(&pantry),
            dietary = list_or_none(&dietary),
            allergies = list_or_none(&allergies),
            goals = list_or_none(&goals),
        );
        let v = self.call_json(&prompt).await?;
        serde_json::from_value(v)
            .map_err(|e| AppError::Internal(format!("what_to_buy schema: {e}")))
    }

    /// Suggest nutrition-aware recipe ideas, nudging the user to try new things.
    pub async fn recipe_suggestions(
        &self,
        user_id: Uuid,
        count: u32,
    ) -> Result<RecipeSuggestionsResponse, AppError> {
        let (pantry, dietary, allergies, goals) = self.profile_and_pantry(user_id).await?;
        let count = count.clamp(1, 8);
        let knowledge = self.knowledge_block("healthy balanced recipes nutrient variety").await;
        let prompt = format!(
            "You are a creative chef and nutritionist.\n\
             Pantry: {pantry}.\n\
             Dietary restrictions (must respect): {dietary}.\n\
             Allergies (NEVER include): {allergies}.\n\
             Health goals: {goals}.\n{knowledge}\n\
             Suggest {count} recipe ideas that fit the goals, prioritise the pantry where possible, \
             and encourage trying something new and nutritious. For each, explain why it benefits the \
             user and which key nutrients it provides. \
             Return ONLY JSON: {{\"suggestions\":[{{\"name\":\"\",\"description\":\"\",\"why\":\"\",\"key_nutrients\":[\"\"]}}]}}",
            pantry = list_or_none(&pantry),
            dietary = list_or_none(&dietary),
            allergies = list_or_none(&allergies),
            goals = list_or_none(&goals),
        );
        let v = self.call_json(&prompt).await?;
        serde_json::from_value(v)
            .map_err(|e| AppError::Internal(format!("recipe_suggestions schema: {e}")))
    }
}

fn list_or_none(v: &[String]) -> String {
    if v.is_empty() {
        "none".to_string()
    } else {
        v.join(", ")
    }
}
