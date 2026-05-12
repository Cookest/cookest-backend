//! Grocery Scan Service — AI-powered grocery detection via Ollama llava
//!
//! Sends a base64-encoded image to Ollama's multimodal model (llava by default)
//! and parses the structured JSON response into a list of detected grocery items.

use base64::{Engine, engine::general_purpose::STANDARD as B64};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use cookest_shared::errors::AppError;

// ── Ollama multimodal types ───────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct OllamaVisionRequest {
    model: String,
    messages: Vec<OllamaVisionMessage>,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct OllamaVisionMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    images: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct OllamaVisionResponse {
    message: OllamaVisionMessageInner,
}

#[derive(Debug, Deserialize)]
struct OllamaVisionMessageInner {
    content: String,
}

// ── Public types ──────────────────────────────────────────────────────────────

/// A single grocery item detected from an image scan
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DetectedGroceryItem {
    pub name: String,
    pub quantity: f64,
    pub unit: String,
    pub category: Option<String>,
    /// Suggested storage location (fridge / pantry / freezer)
    pub storage_location: Option<String>,
}

/// Complete response from the scan endpoint
#[derive(Debug, Serialize)]
pub struct ScanResponse {
    pub items: Vec<DetectedGroceryItem>,
    /// How many items were detected
    pub count: usize,
}

/// Request to bulk-add items from a scan result
#[derive(Debug, Deserialize)]
pub struct BulkAddItem {
    pub name: String,
    pub quantity: f64,
    pub unit: String,
    pub storage_location: Option<String>,
    pub expiry_date: Option<String>,
}

// ── Service ───────────────────────────────────────────────────────────────────

pub struct ScanService {
    http: Client,
    ollama_url: String,
    model: String,
}

impl ScanService {
    pub fn new() -> Self {
        let ollama_url = std::env::var("OLLAMA_URL")
            .unwrap_or_else(|_| "http://localhost:11434".to_string());
        // Prefer a dedicated vision model env var, fall back to OLLAMA_MODEL, then llava
        let model = std::env::var("OLLAMA_VISION_MODEL")
            .or_else(|_| std::env::var("OLLAMA_MODEL"))
            .unwrap_or_else(|_| "llava".to_string());

        Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(90))
                .build()
                .unwrap_or_default(),
            ollama_url,
            model,
        }
    }

    /// Analyse an image and return detected grocery items.
    /// `image_bytes` can be JPEG, PNG, or WebP.
    pub async fn scan_groceries(&self, image_bytes: Vec<u8>) -> Result<ScanResponse, AppError> {
        let b64_image = B64.encode(&image_bytes);

        let prompt = concat!(
            "You are a grocery item detector. Look at this image and identify every food or grocery item visible.\n\n",
            "Return ONLY a JSON array — no markdown, no explanation, no preamble. Just raw JSON like:\n",
            r#"[{"name":"Whole Milk","quantity":1.0,"unit":"l","category":"dairy","storage_location":"fridge"},{"name":"Tomatoes","quantity":4.0,"unit":"pcs","category":"produce","storage_location":"fridge"}]"#,
            "\n\nRules:\n",
            "- name: English, singular, specific (e.g. 'Chicken Breast', not 'Meat')\n",
            "- quantity: numeric best-estimate (use 1.0 if unknown)\n",
            "- unit: one of pcs, g, kg, ml, l, pack, bottle, can, bag, box\n",
            "- category: one of produce, dairy, meat, seafood, bakery, beverages, condiments, snacks, frozen, pantry, spices\n",
            "- storage_location: one of fridge, pantry, freezer\n",
            "- Only include real food/grocery items you can clearly see\n",
            "- If the image contains many items, list all of them\n",
            "Return only the JSON array, nothing else."
        );

        let request = OllamaVisionRequest {
            model: self.model.clone(),
            messages: vec![OllamaVisionMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
                images: vec![b64_image],
            }],
            stream: false,
        };

        let resp = self
            .http
            .post(format!("{}/api/chat", self.ollama_url))
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                tracing::error!("Ollama vision request failed: {}", e);
                AppError::Internal("AI scan service unavailable".into())
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            tracing::error!("Ollama vision returned {}: {}", status, body);
            return Err(AppError::Internal("AI scan service returned an error".into()));
        }

        let ollama_resp: OllamaVisionResponse = resp.json().await.map_err(|e| {
            tracing::error!("Failed to parse Ollama vision response: {}", e);
            AppError::Internal("Failed to parse AI scan response".into())
        })?;

        let raw = ollama_resp.message.content;
        tracing::debug!("Ollama raw scan response: {}", raw);

        let items = Self::parse_grocery_items(&raw);
        let count = items.len();

        Ok(ScanResponse { items, count })
    }

    fn parse_grocery_items(raw: &str) -> Vec<DetectedGroceryItem> {
        // Strip markdown code fences if present
        let cleaned = raw
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        // Find JSON array boundaries
        let json_str = match (cleaned.find('['), cleaned.rfind(']')) {
            (Some(start), Some(end)) if end > start => &cleaned[start..=end],
            _ => {
                tracing::warn!("No JSON array found in scan response: {}", cleaned);
                return vec![];
            }
        };

        match serde_json::from_str::<Vec<Value>>(json_str) {
            Ok(arr) => arr
                .iter()
                .filter_map(|v| {
                    let name = v.get("name")?.as_str()?.trim().to_string();
                    if name.is_empty() {
                        return None;
                    }
                    let quantity = v
                        .get("quantity")
                        .and_then(|q| q.as_f64())
                        .unwrap_or(1.0)
                        .max(0.01);
                    let unit = v
                        .get("unit")
                        .and_then(|u| u.as_str())
                        .unwrap_or("pcs")
                        .to_string();
                    let category = v
                        .get("category")
                        .and_then(|c| c.as_str())
                        .map(|s| s.to_string());
                    let storage_location = v
                        .get("storage_location")
                        .and_then(|s| s.as_str())
                        .map(|s| s.to_string());
                    Some(DetectedGroceryItem {
                        name,
                        quantity,
                        unit,
                        category,
                        storage_location,
                    })
                })
                .collect(),
            Err(e) => {
                tracing::warn!("Failed to parse grocery JSON: {} — raw: {}", e, json_str);
                vec![]
            }
        }
    }
}
