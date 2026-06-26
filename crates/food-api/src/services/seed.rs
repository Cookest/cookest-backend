//! One-time base catalog seed.
//!
//! When the `ingredients` table is empty, load a small curated set of common
//! ingredients (bundled at compile time) so the app ships with a usable preset
//! catalog out of the box. Larger imports (Open Food Facts, MM-Food-100K, or a
//! directory of CSVs) stay admin-driven via the Database / ingredient import.

use sea_orm::DatabaseConnection;

use crate::errors::AppError;
use crate::services::ImportService;

/// Curated common ingredients, embedded so no data volume is required.
const SEED_CSV: &str = include_str!("../../seed/ingredients.csv");

/// Seed the catalog only when it is currently empty. Returns rows inserted.
pub async fn seed_if_empty(db: &DatabaseConnection) -> Result<usize, AppError> {
    use sea_orm::{ConnectionTrait, Statement};
    let count_row = db
        .query_one(Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT COUNT(*) AS n FROM ingredients".to_string(),
        ))
        .await?;
    let existing: i64 = count_row
        .and_then(|r| r.try_get::<i64>("", "n").ok())
        .unwrap_or(0);
    if existing > 0 {
        return Ok(0);
    }

    let inserted = ImportService::new(db.clone())
        .import_ingredients_reader(SEED_CSV.as_bytes())
        .await?;
    tracing::info!("Seeded {} base ingredients into empty catalog", inserted);
    Ok(inserted)
}
