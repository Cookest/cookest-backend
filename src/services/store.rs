//! Store service — manages stores, PDF uploads, AI processing, and price comparison
//!
//! PDF processing pipeline:
//! 1. Admin uploads PDF → saved to disk → job created (status=pending)
//! 2. Background tokio task: pdftoppm converts each page to PNG
//! 3. Each PNG is base64-encoded and sent to Ollama llava vision model
//! 4. Structured JSON extracted → inserted into staging table (store_promotion_candidates)
//! 5. Admin reviews candidates → approves to store_promotions
//! 6. store_promotion_ingredients links promotions to known ingredients via pg_trgm

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chrono::Utc;
use rust_decimal::Decimal;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait,
    QueryFilter, Set,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::entity::{
    pdf_processing_job::{self, ActiveModel as JobActiveModel, Entity as PdfJob},
    store::{self, ActiveModel as StoreActiveModel, Entity as Store},
    store_promotion::{self, ActiveModel as PromotionActiveModel, Entity as StorePromotion},
    store_promotion_candidate::{self, ActiveModel as CandidateActiveModel, Entity as Candidate},
};
use crate::errors::AppError;

#[derive(Debug, Serialize, Deserialize)]
pub struct StoreResponse {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub website: Option<String>,
    pub logo_url: Option<String>,
    pub country: Option<String>,
    pub city: Option<String>,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
}

impl From<store::Model> for StoreResponse {
    fn from(m: store::Model) -> Self {
        Self {
            id: m.id,
            name: m.name,
            slug: m.slug,
            website: m.website,
            logo_url: m.logo_url,
            country: m.country,
            city: m.city,
            lat: m.lat,
            lng: m.lng,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateStoreRequest {
    pub name: String,
    pub slug: String,
    pub website: Option<String>,
    pub logo_url: Option<String>,
    pub country: Option<String>,
    pub city: Option<String>,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct JobStatusResponse {
    pub id: Uuid,
    pub store_id: Uuid,
    pub status: String,
    pub retry_count: i32,
    pub error: Option<String>,
    pub created_at: sea_orm::prelude::DateTimeWithTimeZone,
    pub processed_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
}

impl From<pdf_processing_job::Model> for JobStatusResponse {
    fn from(m: pdf_processing_job::Model) -> Self {
        Self {
            id: m.id,
            store_id: m.store_id,
            status: m.status,
            retry_count: m.retry_count,
            error: m.error,
            created_at: m.created_at,
            processed_at: m.processed_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PromotionResponse {
    pub id: Uuid,
    pub store_id: Uuid,
    pub product_name: String,
    pub brand: Option<String>,
    pub original_price: Option<Decimal>,
    pub discounted_price: Decimal,
    pub discount_pct: Option<Decimal>,
    pub unit: Option<String>,
    pub valid_from: Option<sea_orm::prelude::DateTimeWithTimeZone>,
    pub valid_until: Option<sea_orm::prelude::DateTimeWithTimeZone>,
    pub confidence: Option<Decimal>,
}

impl From<store_promotion::Model> for PromotionResponse {
    fn from(m: store_promotion::Model) -> Self {
        Self {
            id: m.id,
            store_id: m.store_id,
            product_name: m.product_name,
            brand: m.brand,
            original_price: m.original_price,
            discounted_price: m.discounted_price,
            discount_pct: m.discount_pct,
            unit: m.unit,
            valid_from: m.valid_from,
            valid_until: m.valid_until,
            confidence: m.confidence,
        }
    }
}

pub struct StoreService {
    db: DatabaseConnection,
    pdf_upload_dir: PathBuf,
    ollama_url: String,
    ollama_model: String,
}

impl StoreService {
    pub fn new(
        db: DatabaseConnection,
        pdf_upload_dir: PathBuf,
        ollama_url: String,
        ollama_model: String,
    ) -> Self {
        Self { db, pdf_upload_dir, ollama_url, ollama_model }
    }

    // ── Stores ──────────────────────────────────────────────────────────────

    pub async fn list_stores(&self) -> Result<Vec<StoreResponse>, AppError> {
        let stores = Store::find().all(&self.db).await?;
        Ok(stores.into_iter().map(StoreResponse::from).collect())
    }

    pub async fn create_store(&self, req: CreateStoreRequest) -> Result<StoreResponse, AppError> {
        let now = Utc::now().fixed_offset();
        let model = StoreActiveModel {
            id: Set(Uuid::new_v4()),
            name: Set(req.name),
            slug: Set(req.slug),
            website: Set(req.website),
            logo_url: Set(req.logo_url),
            country: Set(req.country),
            city: Set(req.city),
            lat: Set(req.lat),
            lng: Set(req.lng),
            created_at: Set(now),
            updated_at: Set(now),
        };
        let inserted = model.insert(&self.db).await?;
        Ok(StoreResponse::from(inserted))
    }

    // ── PDF upload & async processing ──────────────────────────────────────

    /// Save a PDF to disk and create a processing job.
    /// Returns the job — actual processing happens in a spawned background task.
    pub async fn create_pdf_job(
        &self,
        store_id: Uuid,
        pdf_bytes: Vec<u8>,
        original_filename: &str,
    ) -> Result<JobStatusResponse, AppError> {
        // Validate store exists
        Store::find_by_id(store_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound("Store".to_string()))?;

        // Sanitize filename — keep only alphanumeric + extension
        let ext = std::path::Path::new(original_filename)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("pdf");
        if !["pdf"].contains(&ext.to_lowercase().as_str()) {
            return Err(AppError::Validation(
                validator::ValidationErrors::new(),
            ));
        }

        let file_name = format!("{}.pdf", Uuid::new_v4());
        let file_path = self.pdf_upload_dir.join(&file_name);

        std::fs::write(&file_path, &pdf_bytes)
            .map_err(|e| AppError::Internal(format!("Failed to save PDF: {}", e)))?;

        let now = Utc::now().fixed_offset();
        let job = JobActiveModel {
            id: Set(Uuid::new_v4()),
            store_id: Set(store_id),
            file_path: Set(file_path.to_string_lossy().to_string()),
            status: Set("pending".to_string()),
            error: Set(None),
            retry_count: Set(0),
            started_at: Set(None),
            heartbeat_at: Set(None),
            processed_at: Set(None),
            created_at: Set(now),
        };
        let inserted = job.insert(&self.db).await?;
        Ok(JobStatusResponse::from(inserted))
    }

    /// Spawn the background PDF processing task for a job.
    /// Call this immediately after `create_pdf_job`.
    pub fn spawn_pdf_processing(&self, job_id: Uuid) {
        let db = self.db.clone();
        let ollama_url = self.ollama_url.clone();
        let ollama_model = self.ollama_model.clone();

        tokio::spawn(async move {
            if let Err(e) = process_pdf_job(&db, job_id, &ollama_url, &ollama_model).await {
                tracing::error!("PDF job {} failed: {:?}", job_id, e);
                // Mark job as failed
                if let Ok(Some(job)) = PdfJob::find_by_id(job_id).one(&db).await {
                    let mut active: JobActiveModel = job.into();
                    active.status = Set("failed".to_string());
                    active.error = Set(Some(format!("{:?}", e)));
                    active.processed_at = Set(Some(Utc::now().fixed_offset()));
                    let _ = active.update(&db).await;
                }
            }
        });
    }

    pub async fn list_jobs(&self, store_id: Uuid) -> Result<Vec<JobStatusResponse>, AppError> {
        let jobs = PdfJob::find()
            .filter(pdf_processing_job::Column::StoreId.eq(store_id))
            .all(&self.db)
            .await?;
        Ok(jobs.into_iter().map(JobStatusResponse::from).collect())
    }

    pub async fn list_candidates(&self, store_id: Uuid) -> Result<Vec<store_promotion_candidate::Model>, AppError> {
        let candidates = Candidate::find()
            .filter(store_promotion_candidate::Column::StoreId.eq(store_id))
            .filter(store_promotion_candidate::Column::ReviewStatus.eq("pending"))
            .all(&self.db)
            .await?;
        Ok(candidates)
    }

    /// Approve a candidate — moves it to store_promotions
    pub async fn approve_candidate(
        &self,
        candidate_id: Uuid,
        reviewer_id: Uuid,
    ) -> Result<PromotionResponse, AppError> {
        let candidate = Candidate::find_by_id(candidate_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound("Candidate".to_string()))?;

        let now = Utc::now().fixed_offset();

        // Mark candidate as approved
        let mut active_cand: CandidateActiveModel = candidate.clone().into();
        active_cand.review_status = Set("approved".to_string());
        active_cand.reviewed_by = Set(Some(reviewer_id));
        active_cand.reviewed_at = Set(Some(now));
        active_cand.update(&self.db).await?;

        // Create published promotion
        let promotion = PromotionActiveModel {
            id: Set(Uuid::new_v4()),
            store_id: Set(candidate.store_id),
            product_name: Set(candidate.product_name),
            brand: Set(candidate.brand),
            original_price: Set(candidate.original_price),
            discounted_price: Set(candidate.discounted_price),
            discount_pct: Set(candidate.discount_pct),
            unit: Set(candidate.unit),
            valid_from: Set(candidate.valid_from),
            valid_until: Set(candidate.valid_until),
            is_active: Set(true),
            source_pdf_url: Set(None),
            confidence: Set(candidate.confidence),
            created_at: Set(now),
            updated_at: Set(now),
        };
        let inserted = promotion.insert(&self.db).await?;
        Ok(PromotionResponse::from(inserted))
    }

    /// Reject a candidate
    pub async fn reject_candidate(&self, candidate_id: Uuid, reviewer_id: Uuid) -> Result<(), AppError> {
        let candidate = Candidate::find_by_id(candidate_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound("Candidate".to_string()))?;

        let now = Utc::now().fixed_offset();
        let mut active: CandidateActiveModel = candidate.into();
        active.review_status = Set("rejected".to_string());
        active.reviewed_by = Set(Some(reviewer_id));
        active.reviewed_at = Set(Some(now));
        active.update(&self.db).await?;

        Ok(())
    }

    /// Get active promotions for an ingredient (Pro feature — price comparison)
    pub async fn get_promotions_for_ingredient(
        &self,
        ingredient_id: i64,
    ) -> Result<Vec<PromotionResponse>, AppError> {
        use sea_orm::Statement;
        // Join via store_promotion_ingredients with similarity >= 0.5
        let sql = r#"
            SELECT sp.*
            FROM store_promotions sp
            INNER JOIN store_promotion_ingredients spi ON spi.promotion_id = sp.id
            WHERE spi.ingredient_id = $1
              AND sp.is_active = TRUE
              AND (sp.valid_until IS NULL OR sp.valid_until > NOW())
            ORDER BY sp.discounted_price ASC
        "#;

        let rows = self.db.query_all(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            sql,
            [sea_orm::Value::BigInt(Some(ingredient_id))],
        )).await?;

        let mut result = vec![];
        for row in rows {
            let id: Uuid = row.try_get("", "id").map_err(|e| AppError::Internal(e.to_string()))?;
            let store_id: Uuid = row.try_get("", "store_id").map_err(|e| AppError::Internal(e.to_string()))?;
            let product_name: String = row.try_get("", "product_name").map_err(|e| AppError::Internal(e.to_string()))?;
            let brand: Option<String> = row.try_get("", "brand").unwrap_or(None);
            let original_price: Option<Decimal> = row.try_get("", "original_price").unwrap_or(None);
            let discounted_price: Decimal = row.try_get("", "discounted_price").map_err(|e| AppError::Internal(e.to_string()))?;
            let discount_pct: Option<Decimal> = row.try_get("", "discount_pct").unwrap_or(None);
            let unit: Option<String> = row.try_get("", "unit").unwrap_or(None);
            let valid_from: Option<chrono::DateTime<chrono::FixedOffset>> = row.try_get("", "valid_from").unwrap_or(None);
            let valid_until: Option<chrono::DateTime<chrono::FixedOffset>> = row.try_get("", "valid_until").unwrap_or(None);
            let confidence: Option<Decimal> = row.try_get("", "confidence").unwrap_or(None);

            result.push(PromotionResponse {
                id, store_id, product_name, brand, original_price,
                discounted_price, discount_pct, unit, valid_from, valid_until, confidence,
            });
        }
        Ok(result)
    }
}

/// Background task: process a PDF job using pdftoppm + Ollama llava
async fn process_pdf_job(
    db: &DatabaseConnection,
    job_id: Uuid,
    ollama_url: &str,
    ollama_model: &str,
) -> Result<(), AppError> {
    let job = PdfJob::find_by_id(job_id)
        .one(db)
        .await?
        .ok_or_else(|| AppError::NotFound("PDF job".to_string()))?;

    // Mark as processing + set started_at
    let now = Utc::now().fixed_offset();
    let mut active: JobActiveModel = job.clone().into();
    active.status = Set("processing".to_string());
    active.started_at = Set(Some(now));
    active.heartbeat_at = Set(Some(now));
    active.update(db).await?;

    // Convert PDF pages to PNGs via pdftoppm (non-blocking via tokio process)
    let output_dir = std::path::Path::new(&job.file_path)
        .parent()
        .unwrap_or(std::path::Path::new("/tmp"))
        .join(format!("pages_{}", job_id));
    tokio::fs::create_dir_all(&output_dir).await
        .map_err(|e| AppError::Internal(format!("mkdir failed: {}", e)))?;

    let prefix = output_dir.join("page").to_string_lossy().to_string();

    let output = tokio::process::Command::new("pdftoppm")
        .args(["-png", "-r", "150", &job.file_path, &prefix])
        .output()
        .await
        .map_err(|e| AppError::Internal(format!("pdftoppm spawn failed: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Internal(format!("pdftoppm failed: {}", stderr)));
    }

    // Collect generated PNG files
    let mut png_files: Vec<PathBuf> = vec![];
    let mut dir = tokio::fs::read_dir(&output_dir).await
        .map_err(|e| AppError::Internal(format!("read_dir failed: {}", e)))?;
    while let Some(entry) = dir.next_entry().await
        .map_err(|e| AppError::Internal(format!("dir entry failed: {}", e)))?
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("png") {
            png_files.push(path);
        }
    }
    png_files.sort();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    for png_path in &png_files {
        let png_bytes = tokio::fs::read(png_path).await
            .map_err(|e| AppError::Internal(format!("read PNG failed: {}", e)))?;
        let b64 = BASE64.encode(&png_bytes);

        // Update heartbeat so recovery logic knows this job is still alive
        if let Ok(Some(j)) = PdfJob::find_by_id(job_id).one(db).await {
            let mut a: JobActiveModel = j.into();
            a.heartbeat_at = Set(Some(Utc::now().fixed_offset()));
            let _ = a.update(db).await;
        }

        let prompt = r#"
You are a grocery flyer price extraction assistant.
Analyze this supermarket flyer page and extract ALL products with their prices.
Return a JSON array. Each item must have:
  "product_name": string,
  "brand": string or null,
  "original_price": number or null (price before discount),
  "discounted_price": number (current/sale price),
  "discount_pct": number or null (e.g. 20 for 20%),
  "unit": string or null (e.g. "kg", "500g", "piece"),
  "valid_from": "YYYY-MM-DD" or null,
  "valid_until": "YYYY-MM-DD" or null,
  "confidence": number 0.0-1.0 (your confidence in this extraction)
Return ONLY the JSON array, no other text.
"#;

        let req_body = serde_json::json!({
            "model": ollama_model,
            "prompt": prompt,
            "images": [b64],
            "stream": false
        });

        let mut response_json: Option<serde_json::Value> = None;
        for base in ollama_base_candidates(ollama_url) {
            match client
                .post(format!("{}/api/generate", base))
                .json(&req_body)
                .send()
                .await
            {
                Ok(resp) => {
                    if !resp.status().is_success() {
                        tracing::warn!("Ollama returned {} on {}", resp.status(), base);
                        continue;
                    }

                    match resp.json::<serde_json::Value>().await {
                        Ok(parsed) => {
                            response_json = Some(parsed);
                            break;
                        }
                        Err(e) => {
                            tracing::warn!("Ollama JSON parse failed on {}: {}", base, e);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Ollama request failed on {}: {}", base, e);
                }
            }
        }

        let Some(resp_json) = response_json else {
            continue;
        };

        let raw_text = resp_json["response"].as_str().unwrap_or("");
        // Extract JSON array from response (model may include surrounding text)
        if let Some(start) = raw_text.find('[') {
            if let Some(end) = raw_text.rfind(']') {
                let json_str = &raw_text[start..=end];
                if let Ok(items) = serde_json::from_str::<Vec<serde_json::Value>>(json_str) {
                    for item in items {
                        insert_candidate(db, &job, &item).await;
                    }
                }
            }
        }
    }

    // Cleanup temporary PNG files
    let _ = tokio::fs::remove_dir_all(&output_dir).await;

    // Mark job as done
    if let Ok(Some(j)) = PdfJob::find_by_id(job_id).one(db).await {
        let mut a: JobActiveModel = j.into();
        a.status = Set("done".to_string());
        a.processed_at = Set(Some(Utc::now().fixed_offset()));
        let _ = a.update(db).await;
    }

    Ok(())
}

fn ollama_base_candidates(base: &str) -> Vec<String> {
    let normalized = base.trim_end_matches('/').to_string();
    let mut candidates = vec![normalized.clone()];

    if normalized.contains("localhost") || normalized.contains("127.0.0.1") {
        candidates.push(
            normalized
                .replace("localhost", "host.docker.internal")
                .replace("127.0.0.1", "host.docker.internal"),
        );
    } else if normalized.contains("host.docker.internal") {
        candidates.push(normalized.replace("host.docker.internal", "localhost"));
    }

    candidates.dedup();
    candidates
}

/// Insert a single AI-extracted item into the staging table
async fn insert_candidate(
    db: &DatabaseConnection,
    job: &pdf_processing_job::Model,
    item: &serde_json::Value,
) {
    let product_name = match item["product_name"].as_str() {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => return,
    };
    let discounted_price = match item["discounted_price"].as_f64() {
        Some(p) if p > 0.0 => Decimal::try_from(p).unwrap_or_default(),
        _ => return,
    };

    let original_price = item["original_price"].as_f64()
        .and_then(|p| Decimal::try_from(p).ok());
    let discount_pct = item["discount_pct"].as_f64()
        .and_then(|p| Decimal::try_from(p).ok());
    let confidence = item["confidence"].as_f64()
        .and_then(|c| Decimal::try_from(c).ok());

    let valid_from = item["valid_from"].as_str()
        .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc().fixed_offset());

    let valid_until = item["valid_until"].as_str()
        .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        .map(|d| d.and_hms_opt(23, 59, 59).unwrap().and_utc().fixed_offset());

    let candidate = CandidateActiveModel {
        id: Set(Uuid::new_v4()),
        store_id: Set(job.store_id),
        job_id: Set(job.id),
        product_name: Set(product_name),
        brand: Set(item["brand"].as_str().map(|s| s.to_string())),
        original_price: Set(original_price),
        discounted_price: Set(discounted_price),
        discount_pct: Set(discount_pct),
        unit: Set(item["unit"].as_str().map(|s| s.to_string())),
        valid_from: Set(valid_from),
        valid_until: Set(valid_until),
        confidence: Set(confidence),
        review_status: Set("pending".to_string()),
        reviewed_by: Set(None),
        reviewed_at: Set(None),
        created_at: Set(Utc::now().fixed_offset()),
    };

    if let Err(e) = candidate.insert(db).await {
        tracing::warn!("Failed to insert candidate: {:?}", e);
    }
}
