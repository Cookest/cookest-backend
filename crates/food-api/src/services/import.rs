//! Dataset import service — scans folders, parses CSV/JSON, inserts into local DB

use std::path::{Path, PathBuf};
use sea_orm::{DatabaseConnection, ActiveModelTrait, Set, TransactionTrait};
use serde::Deserialize;
use crate::errors::AppError;
use crate::entity::recipe;
use crate::services::time_region::{estimate_time, classify_region};

/// A single importable recipe row (flexible — many fields optional)
#[derive(Debug, Deserialize, Default)]
pub struct ImportRow {
    pub name: String,
    pub description: Option<String>,
    pub cuisine: Option<String>,
    pub category: Option<String>,
    pub difficulty: Option<String>,
    pub servings: Option<i32>,
    pub prep_time_min: Option<i32>,
    pub cook_time_min: Option<i32>,
    pub total_time_min: Option<i32>,
    pub is_vegetarian: Option<bool>,
    pub is_vegan: Option<bool>,
    pub is_gluten_free: Option<bool>,
    pub is_dairy_free: Option<bool>,
    pub is_nut_free: Option<bool>,
    pub source_url: Option<String>,
    /// Comma-separated ingredient names (for inference only)
    pub ingredients_csv: Option<String>,
    /// Step instructions separated by " | " (for time inference)
    pub steps_text: Option<String>,
}

pub struct ImportResult {
    pub rows_imported: usize,
    pub rows_skipped: usize,
    pub message: String,
}

pub struct ImportService {
    db: DatabaseConnection,
}

impl ImportService {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// List .csv and .json files in the given folder path (container-local).
    pub fn scan_folder(&self, folder: &str) -> Result<Vec<String>, AppError> {
        // Basic sandbox check: disallow path traversal
        let path = Path::new(folder);
        if folder.contains("..") {
            return Err(AppError::Forbidden);
        }
        if !path.exists() {
            return Err(AppError::NotFound(format!("Folder '{}' does not exist", folder)));
        }
        if !path.is_dir() {
            return Err(AppError::NotFound(format!("'{}' is not a directory", folder)));
        }

        let mut files = Vec::new();
        for entry in std::fs::read_dir(path)
            .map_err(|e| AppError::Internal(format!("Cannot read directory: {}", e)))?
        {
            let entry = entry.map_err(|e| AppError::Internal(e.to_string()))?;
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.ends_with(".csv") || fname.ends_with(".json") {
                files.push(fname);
            }
        }
        files.sort();
        Ok(files)
    }

    /// Import a single file into the recipes table using a SeaORM transaction.
    pub async fn import_file(
        &self,
        folder: &str,
        filename: &str,
        format: &str,
    ) -> Result<ImportResult, AppError> {
        if folder.contains("..") || filename.contains("..") {
            return Err(AppError::Forbidden);
        }
        let path = PathBuf::from(folder).join(filename);
        if !path.exists() {
            return Err(AppError::NotFound(format!("File '{}' not found", filename)));
        }

        let rows = match format.to_lowercase().as_str() {
            "json" => self.parse_json(&path)?,
            _ => self.parse_csv(&path)?,
        };

        let total = rows.len();
        let mut imported = 0usize;

        let txn = self.db.begin().await?;

        for row in rows {
            if row.name.is_empty() {
                continue;
            }

            // Run time inference if times are missing
            let ingredient_list: Vec<String> = row.ingredients_csv
                .as_deref()
                .unwrap_or("")
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let step_instructions: Vec<String> = row.steps_text
                .as_deref()
                .unwrap_or("")
                .split(" | ")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let step_refs: Vec<&str> = step_instructions.iter().map(|s| s.as_str()).collect();
            let ing_refs: Vec<&str> = ingredient_list.iter().map(|s| s.as_str()).collect();

            let (prep_time, cook_time, total_time) = if row.prep_time_min.is_none() && row.cook_time_min.is_none() {
                let est = estimate_time(&step_refs, ingredient_list.len(), step_instructions.len(), row.category.as_deref());
                (Some(est.prep_time_min), Some(est.cook_time_min), Some(est.total_time_min))
            } else {
                (row.prep_time_min, row.cook_time_min, row.total_time_min)
            };

            let cuisine = if row.cuisine.is_none() {
                let region = classify_region(&ing_refs, &[]);
                if region == "International" { None } else { Some(region) }
            } else {
                row.cuisine.clone()
            };

            let slug = {
                let base = slug::slugify(&row.name);
                format!("{}-{}", base, uuid::Uuid::new_v4().to_string().split('-').next().unwrap_or("x"))
            };

            let now = chrono::Utc::now().fixed_offset();

            let active = recipe::ActiveModel {
                name: Set(row.name.clone()),
                slug: Set(slug),
                description: Set(row.description.clone()),
                cuisine: Set(cuisine),
                category: Set(row.category.clone()),
                difficulty: Set(row.difficulty.clone()),
                servings: Set(row.servings.unwrap_or(2)),
                prep_time_min: Set(prep_time),
                cook_time_min: Set(cook_time),
                total_time_min: Set(total_time),
                is_vegetarian: Set(row.is_vegetarian.unwrap_or(false)),
                is_vegan: Set(row.is_vegan.unwrap_or(false)),
                is_gluten_free: Set(row.is_gluten_free.unwrap_or(false)),
                is_dairy_free: Set(row.is_dairy_free.unwrap_or(false)),
                is_nut_free: Set(row.is_nut_free.unwrap_or(false)),
                source_url: Set(row.source_url.clone()),
                average_rating: Set(None),
                rating_count: Set(0),
                author_id: Set(None),
                is_public: Set(true),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            };

            match active.insert(&txn).await {
                Ok(_) => imported += 1,
                Err(e) => {
                    tracing::warn!("Skipping row '{}': {}", row.name, e);
                }
            }
        }

        txn.commit().await?;

        Ok(ImportResult {
            rows_imported: imported,
            rows_skipped: total - imported,
            message: format!("Imported {}/{} rows", imported, total),
        })
    }

    fn parse_csv(&self, path: &Path) -> Result<Vec<ImportRow>, AppError> {
        let mut rdr = csv::Reader::from_path(path)
            .map_err(|e| AppError::Internal(format!("CSV read error: {}", e)))?;
        let mut rows = Vec::new();
        for result in rdr.deserialize::<ImportRow>() {
            match result {
                Ok(row) => rows.push(row),
                Err(e) => tracing::warn!("Skipping CSV row: {}", e),
            }
        }
        Ok(rows)
    }

    fn parse_json(&self, path: &Path) -> Result<Vec<ImportRow>, AppError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| AppError::Internal(format!("JSON read error: {}", e)))?;
        let rows: Vec<ImportRow> = serde_json::from_str(&content)
            .map_err(|e| AppError::Internal(format!("JSON parse error: {}", e)))?;
        Ok(rows)
    }
}
