//! Dataset import service — scans folders, parses CSV/JSON, inserts into local DB

use crate::entity::recipe;
use crate::errors::AppError;
use crate::services::time_region::{classify_region, estimate_time};
use sea_orm::{ActiveModelTrait, ConnectionTrait, DatabaseConnection, Set, TransactionTrait};
use serde::Deserialize;
use std::path::{Path, PathBuf};

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

    /// List .csv, .json, and .csv.gz files in the given folder path (container-local).
    pub fn scan_folder(&self, folder: &str) -> Result<Vec<String>, AppError> {
        // Basic sandbox check: disallow path traversal
        let path = Path::new(folder);
        if folder.contains("..") {
            return Err(AppError::Forbidden);
        }
        if !path.exists() {
            return Err(AppError::NotFound(format!(
                "Folder '{}' does not exist",
                folder
            )));
        }
        if !path.is_dir() {
            return Err(AppError::NotFound(format!(
                "'{}' is not a directory",
                folder
            )));
        }

        let mut files = Vec::new();
        for entry in std::fs::read_dir(path)
            .map_err(|e| AppError::Internal(format!("Cannot read directory: {}", e)))?
        {
            let entry = entry.map_err(|e| AppError::Internal(e.to_string()))?;
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.ends_with(".csv") || fname.ends_with(".json") || fname.ends_with(".csv.gz") {
                files.push(fname);
            }
        }
        files.sort();
        Ok(files)
    }

    /// Import a single file into the recipes table or ingredients table using a SeaORM transaction.
    pub async fn import_file(
        &self,
        folder: &str,
        filename: &str,
        format: &str,
    ) -> Result<ImportResult, AppError> {
        if folder.contains("..") || filename.contains("..") {
            return Err(AppError::Forbidden);
        }

        // If it's an Open Food Facts file, delegate to off importer
        if filename.contains("openfoodfacts") || filename.contains("products") {
            return self.import_openfoodfacts(folder, filename).await;
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
            let ingredient_list: Vec<String> = row
                .ingredients_csv
                .as_deref()
                .unwrap_or("")
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let step_instructions: Vec<String> = row
                .steps_text
                .as_deref()
                .unwrap_or("")
                .split(" | ")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let step_refs: Vec<&str> = step_instructions.iter().map(|s| s.as_str()).collect();
            let ing_refs: Vec<&str> = ingredient_list.iter().map(|s| s.as_str()).collect();

            let (prep_time, cook_time, total_time) =
                if row.prep_time_min.is_none() && row.cook_time_min.is_none() {
                    let est = estimate_time(
                        &step_refs,
                        ingredient_list.len(),
                        step_instructions.len(),
                        row.category.as_deref(),
                    );
                    (
                        Some(est.prep_time_min),
                        Some(est.cook_time_min),
                        Some(est.total_time_min),
                    )
                } else {
                    (row.prep_time_min, row.cook_time_min, row.total_time_min)
                };

            let cuisine = if row.cuisine.is_none() {
                let region = classify_region(&ing_refs, &[]);
                if region == "International" {
                    None
                } else {
                    Some(region)
                }
            } else {
                row.cuisine.clone()
            };

            let slug = {
                let base = slug::slugify(&row.name);
                format!(
                    "{}-{}",
                    base,
                    uuid::Uuid::new_v4()
                        .to_string()
                        .split('-')
                        .next()
                        .unwrap_or("x")
                )
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

    /// Import a simple ingredient-list file (seed CSV format:
    /// `name,category,calories,protein_g,carbs_g,fat_g`) into the catalog.
    pub async fn import_ingredients_file(
        &self,
        folder: &str,
        filename: &str,
    ) -> Result<ImportResult, AppError> {
        if folder.contains("..") || filename.contains("..") {
            return Err(AppError::Forbidden);
        }
        let path = PathBuf::from(folder).join(filename);
        if !path.exists() {
            return Err(AppError::NotFound(format!("File '{}' not found", filename)));
        }
        let file = std::fs::File::open(&path)
            .map_err(|e| AppError::Internal(format!("Failed to open file: {}", e)))?;
        let imported = self.import_ingredients_reader(file).await?;
        Ok(ImportResult {
            rows_imported: imported,
            rows_skipped: 0,
            message: format!("Imported {} ingredients", imported),
        })
    }

    /// Core ingredient upsert from a CSV reader, shared by the startup seed and
    /// the directory import. Columns: `name,category,calories,protein_g,carbs_g,fat_g`
    /// (header required; extra columns ignored; numeric cells optional). Upserts by
    /// name so re-running is idempotent.
    pub async fn import_ingredients_reader<R: std::io::Read>(
        &self,
        reader: R,
    ) -> Result<usize, AppError> {
        use std::str::FromStr;
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(true)
            .flexible(true)
            .from_reader(reader);

        let parse_dec = |s: Option<&str>| -> Option<rust_decimal::Decimal> {
            let t = s.unwrap_or("").trim();
            if t.is_empty() {
                None
            } else {
                rust_decimal::Decimal::from_str(t).ok()
            }
        };

        let now = chrono::Utc::now().fixed_offset();
        let txn = self.db.begin().await?;
        let mut imported = 0usize;

        for record in rdr.records() {
            let record =
                record.map_err(|e| AppError::Internal(format!("csv parse error: {}", e)))?;
            let name = record.get(0).unwrap_or("").trim();
            if name.is_empty() {
                continue;
            }
            let category = record.get(1).map(str::trim).filter(|s| !s.is_empty());
            let calories = parse_dec(record.get(2));
            let protein = parse_dec(record.get(3));
            let carbs = parse_dec(record.get(4));
            let fat = parse_dec(record.get(5));

            let ing_row = txn
                .query_one(sea_orm::Statement::from_sql_and_values(
                    sea_orm::DatabaseBackend::Postgres,
                    "INSERT INTO ingredients (name, category, created_at) VALUES ($1, $2, $3) \
                     ON CONFLICT (name) DO UPDATE SET \
                       category = COALESCE(EXCLUDED.category, ingredients.category) \
                     RETURNING id",
                    [name.into(), category.into(), now.into()],
                ))
                .await?;

            if let Some(row) = ing_row {
                let ing_id: i64 = row.try_get("", "id")?;
                imported += 1;
                if calories.is_some() || protein.is_some() || carbs.is_some() || fat.is_some() {
                    txn.execute(sea_orm::Statement::from_sql_and_values(
                        sea_orm::DatabaseBackend::Postgres,
                        "INSERT INTO ingredient_nutrients \
                           (ingredient_id, calories, protein_g, carbs_g, fat_g) \
                         VALUES ($1, $2, $3, $4, $5) \
                         ON CONFLICT (ingredient_id) DO UPDATE SET \
                           calories=EXCLUDED.calories, protein_g=EXCLUDED.protein_g, \
                           carbs_g=EXCLUDED.carbs_g, fat_g=EXCLUDED.fat_g",
                        [
                            ing_id.into(),
                            calories.into(),
                            protein.into(),
                            carbs.into(),
                            fat.into(),
                        ],
                    ))
                    .await?;
                }
            }
        }

        txn.commit().await?;
        Ok(imported)
    }

    async fn import_openfoodfacts(
        &self,
        folder: &str,
        filename: &str,
    ) -> Result<ImportResult, AppError> {
        use flate2::read::GzDecoder;
        use std::fs::File;
        use std::io::BufReader;
        use std::str::FromStr;

        let path = PathBuf::from(folder).join(filename);
        if !path.exists() {
            return Err(AppError::NotFound(format!("File '{}' not found", filename)));
        }

        let file = File::open(&path)
            .map_err(|e| AppError::Internal(format!("Failed to open file: {}", e)))?;
        let reader = BufReader::new(file);

        let input: Box<dyn std::io::Read> = if filename.ends_with(".gz") {
            let decoder = GzDecoder::new(reader);
            Box::new(decoder)
        } else {
            Box::new(reader)
        };

        let mut rdr = csv::ReaderBuilder::new()
            .delimiter(b'\t')
            .flexible(true)
            .from_reader(input);

        let headers = rdr
            .headers()
            .map_err(|e| AppError::Internal(format!("Failed to read CSV headers: {}", e)))?;

        let get_idx = |name: &str| headers.iter().position(|h| h == name);

        let code_idx = get_idx("code");
        let name_idx = get_idx("product_name");
        let cat_idx = get_idx("categories");
        let img_idx = get_idx("image_url");

        let kcal_idx = get_idx("energy-kcal_100g");
        let protein_idx = get_idx("proteins_100g");
        let carbs_idx = get_idx("carbohydrates_100g");
        let fat_idx = get_idx("fat_100g");
        let fiber_idx = get_idx("fiber_100g");
        let sugar_idx = get_idx("sugars_100g");
        let sodium_idx = get_idx("sodium_100g");
        let sat_fat_idx = get_idx("saturated-fat_100g");

        let mut imported = 0usize;
        let mut skipped = 0usize;

        // Limit the total imports to 100,000 to keep execution time under 1 minute.
        let max_rows = 100_000;

        let mut record = csv::StringRecord::new();
        let mut batch = Vec::new();

        while rdr
            .read_record(&mut record)
            .map_err(|e| AppError::Internal(format!("CSV read record error: {}", e)))?
        {
            if imported >= max_rows {
                break;
            }

            let code = code_idx
                .and_then(|idx| record.get(idx))
                .unwrap_or("")
                .trim()
                .to_string();
            let name = name_idx
                .and_then(|idx| record.get(idx))
                .unwrap_or("")
                .trim()
                .to_string();
            if code.is_empty() || name.is_empty() {
                skipped += 1;
                continue;
            }

            let categories = cat_idx
                .and_then(|idx| record.get(idx))
                .map(|c| c.split(',').next().unwrap_or("").trim().to_string());
            let image_url = img_idx
                .and_then(|idx| record.get(idx))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());

            let parse_dec =
                |idx: Option<usize>, record: &csv::StringRecord| -> Option<rust_decimal::Decimal> {
                    idx.and_then(|i| record.get(i))
                        .and_then(|val| rust_decimal::Decimal::from_str(val).ok())
                };

            let calories = parse_dec(kcal_idx, &record);
            let protein = parse_dec(protein_idx, &record);
            let carbs = parse_dec(carbs_idx, &record);
            let fat = parse_dec(fat_idx, &record);
            let fiber = parse_dec(fiber_idx, &record);
            let sugar = parse_dec(sugar_idx, &record);
            let sodium =
                parse_dec(sodium_idx, &record).map(|s| s * rust_decimal::Decimal::from(1000));
            let saturated_fat = parse_dec(sat_fat_idx, &record);

            batch.push((
                code,
                name,
                categories,
                image_url,
                calories,
                protein,
                carbs,
                fat,
                fiber,
                sugar,
                sodium,
                saturated_fat,
            ));

            if batch.len() >= 500 {
                self.flush_batch_off(&batch).await?;
                imported += batch.len();
                batch.clear();
            }
        }

        if !batch.is_empty() {
            self.flush_batch_off(&batch).await?;
            imported += batch.len();
        }

        Ok(ImportResult {
            rows_imported: imported,
            rows_skipped: skipped,
            message: format!(
                "Successfully imported {} OpenFoodFacts products (skipped {})",
                imported, skipped
            ),
        })
    }

    async fn flush_batch_off(
        &self,
        batch: &[(
            String,
            String,
            Option<String>,
            Option<String>,
            Option<rust_decimal::Decimal>,
            Option<rust_decimal::Decimal>,
            Option<rust_decimal::Decimal>,
            Option<rust_decimal::Decimal>,
            Option<rust_decimal::Decimal>,
            Option<rust_decimal::Decimal>,
            Option<rust_decimal::Decimal>,
            Option<rust_decimal::Decimal>,
        )],
    ) -> Result<(), AppError> {
        use sea_orm::TransactionTrait;
        let txn = self.db.begin().await?;

        for (code, name, cat, img, calories, protein, carbs, fat, fiber, sugar, sodium, sat_fat) in
            batch
        {
            let now = chrono::Utc::now().fixed_offset();
            let ing_row = txn.query_one(sea_orm::Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Postgres,
                "INSERT INTO ingredients (name, category, off_id, image_url, created_at)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (name) DO UPDATE SET off_id = EXCLUDED.off_id, image_url = COALESCE(ingredients.image_url, EXCLUDED.image_url)
                 RETURNING id",
                [name.clone().into(), cat.clone().into(), Some(code.clone()).into(), img.clone().into(), now.into()],
            )).await?;

            if let Some(row) = ing_row {
                if let Ok(ing_id) = row.try_get::<i64>("", "id") {
                    let _ = txn.execute(sea_orm::Statement::from_sql_and_values(
                        sea_orm::DatabaseBackend::Postgres,
                        "INSERT INTO ingredient_nutrients (ingredient_id, calories, protein_g, carbs_g, fat_g, fiber_g, sugar_g, sodium_mg, saturated_fat_g)
                         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                         ON CONFLICT (ingredient_id) DO UPDATE SET
                            calories = EXCLUDED.calories,
                            protein_g = EXCLUDED.protein_g,
                            carbs_g = EXCLUDED.carbs_g,
                            fat_g = EXCLUDED.fat_g,
                            fiber_g = EXCLUDED.fiber_g,
                            sugar_g = EXCLUDED.sugar_g,
                            sodium_mg = EXCLUDED.sodium_mg,
                            saturated_fat_g = EXCLUDED.saturated_fat_g",
                        [ing_id.into(), calories.clone().into(), protein.clone().into(), carbs.clone().into(), fat.clone().into(), fiber.clone().into(), sugar.clone().into(), sodium.clone().into(), sat_fat.clone().into()],
                    )).await;

                    let _ = txn.execute(sea_orm::Statement::from_sql_and_values(
                        sea_orm::DatabaseBackend::Postgres,
                        "INSERT INTO portion_sizes (ingredient_id, description, weight_grams, unit)
                         VALUES ($1, '100g', 100.0, 'g')
                         ON CONFLICT DO NOTHING",
                        [ing_id.into()],
                    )).await;
                }
            }
        }

        txn.commit().await?;
        Ok(())
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
