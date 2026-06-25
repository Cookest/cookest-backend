//! Embedding service — Ollama embeddings + pgvector similarity search over the
//! nutrition knowledge base (`knowledge_chunks`).
//!
//! Used to ground AI recipe and nutrition advice in open nutrition books (RAG).
//! Search degrades gracefully to an empty result when embeddings or the
//! knowledge base are unavailable, so chat keeps working without RAG.

use reqwest::Client;
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};
use serde::Serialize;

use cookest_shared::errors::AppError;

pub struct EmbeddingService {
    db: DatabaseConnection,
    http: Client,
    ollama_url: String,
    embed_model: String,
}

/// A retrieved nutrition-knowledge passage.
#[derive(Debug, Serialize)]
pub struct KnowledgeChunk {
    pub id: i64,
    pub source: String,
    pub title: Option<String>,
    pub content: String,
    /// Cosine similarity in [0, 1] (1 = identical).
    pub score: f64,
}

impl EmbeddingService {
    pub fn new(db: DatabaseConnection, ollama_url: String, embed_model: String) -> Self {
        Self {
            db,
            http: Client::new(),
            ollama_url,
            embed_model,
        }
    }

    /// Embed text via Ollama's `/api/embeddings` endpoint.
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>, AppError> {
        let body = serde_json::json!({ "model": self.embed_model, "prompt": text });
        let resp = self
            .http
            .post(format!("{}/api/embeddings", self.ollama_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("embedding request failed: {}", e)))?;
        if !resp.status().is_success() {
            return Err(AppError::Internal(format!(
                "embedding status {}",
                resp.status()
            )));
        }
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| AppError::Internal(format!("embedding decode: {}", e)))?;
        let arr = json["embedding"]
            .as_array()
            .ok_or_else(|| AppError::Internal("embedding field missing".into()))?;
        Ok(arr
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect())
    }

    /// Top-k most similar nutrition-knowledge chunks for a query.
    /// Never errors out the caller — returns an empty vec on any failure.
    pub async fn search(&self, query: &str, top_k: u64) -> Vec<KnowledgeChunk> {
        let embedding = match self.embed(query).await {
            Ok(e) if !e.is_empty() => e,
            _ => return vec![],
        };
        let literal = to_vector_literal(&embedding);
        let sql = format!(
            "SELECT id, source, title, content, 1 - (embedding <=> '{lit}'::vector) AS score \
             FROM knowledge_chunks WHERE embedding IS NOT NULL \
             ORDER BY embedding <=> '{lit}'::vector LIMIT {k}",
            lit = literal,
            k = top_k.clamp(1, 20),
        );
        let rows = match self
            .db
            .query_all(Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                sql,
            ))
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("nutrition knowledge search failed (RAG disabled): {}", e);
                return vec![];
            }
        };
        rows.into_iter()
            .map(|row| KnowledgeChunk {
                id: row.try_get("", "id").unwrap_or_default(),
                source: row.try_get("", "source").unwrap_or_default(),
                title: row.try_get("", "title").unwrap_or(None),
                content: row.try_get("", "content").unwrap_or_default(),
                score: row.try_get("", "score").unwrap_or(0.0),
            })
            .collect()
    }
}

/// Format a float vector as a pgvector text literal: `[0.1,0.2,...]`.
fn to_vector_literal(v: &[f32]) -> String {
    let mut s = String::with_capacity(v.len() * 8 + 2);
    s.push('[');
    for (i, f) in v.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&f.to_string());
    }
    s.push(']');
    s
}
