//! Import handlers — folder scan and dataset execution endpoints
//!
//! Routes (admin-only, no JWT here; JWT verified by app-api proxy):
//!   GET  /api/v1/admin/import/scan?folder=...
//!   POST /api/v1/admin/import/execute

use crate::errors::AppError;
use crate::services::ImportService;
use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Deserialize)]
pub struct ScanQuery {
    pub folder: Option<String>,
}

#[derive(Serialize)]
pub struct ScanResponse {
    pub files: Vec<String>,
    pub folder: String,
}

#[derive(Deserialize)]
pub struct ExecuteRequest {
    pub folder: Option<String>,
    pub filename: String,
    pub format: Option<String>,
}

#[derive(Serialize)]
pub struct ExecuteResponse {
    pub success: bool,
    pub rows_imported: usize,
    pub message: String,
}

pub async fn scan_folder(
    import_svc: web::Data<Arc<ImportService>>,
    query: web::Query<ScanQuery>,
) -> Result<HttpResponse, AppError> {
    let folder = query.folder.as_deref().unwrap_or("/data/imports");
    let files = import_svc.scan_folder(folder)?;
    Ok(HttpResponse::Ok().json(ScanResponse {
        files,
        folder: folder.to_string(),
    }))
}

pub async fn execute_import(
    import_svc: web::Data<Arc<ImportService>>,
    body: web::Json<ExecuteRequest>,
) -> Result<HttpResponse, AppError> {
    let folder = body.folder.as_deref().unwrap_or("/data/imports");
    let format = body.format.as_deref().unwrap_or_else(|| {
        if body.filename.ends_with(".json") {
            "json"
        } else {
            "csv"
        }
    });
    let result = import_svc
        .import_file(folder, &body.filename, format)
        .await?;
    Ok(HttpResponse::Ok().json(ExecuteResponse {
        success: true,
        rows_imported: result.rows_imported,
        message: result.message,
    }))
}

#[derive(Deserialize)]
pub struct IngredientImportRequest {
    pub folder: Option<String>,
    pub filename: String,
}

/// POST /api/v1/admin/import/ingredients
/// Import a simple ingredient-list CSV (name,category,calories,protein_g,carbs_g,fat_g)
/// from a file in the import folder.
pub async fn import_ingredients(
    import_svc: web::Data<Arc<ImportService>>,
    body: web::Json<IngredientImportRequest>,
) -> Result<HttpResponse, AppError> {
    let folder = body.folder.as_deref().unwrap_or("/data/imports");
    let result = import_svc
        .import_ingredients_file(folder, &body.filename)
        .await?;
    Ok(HttpResponse::Ok().json(ExecuteResponse {
        success: true,
        rows_imported: result.rows_imported,
        message: result.message,
    }))
}

#[derive(Serialize)]
pub struct FoodSourceResponse {
    pub source: String,
}

#[derive(Deserialize)]
pub struct FoodSourceRequest {
    pub source: String,
}

pub async fn get_food_source(
    db: web::Data<sea_orm::DatabaseConnection>,
) -> Result<HttpResponse, AppError> {
    use sea_orm::{ConnectionTrait, Statement};
    let query = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "SELECT value FROM system_settings WHERE key = 'food_data_source'",
        vec![],
    );
    let row = db.query_one(query).await?;
    let source = if let Some(row) = row {
        row.try_get::<String>("", "value")
            .unwrap_or_else(|_| "local".to_string())
    } else {
        "local".to_string()
    };
    Ok(HttpResponse::Ok().json(FoodSourceResponse { source }))
}

pub async fn update_food_source(
    db: web::Data<sea_orm::DatabaseConnection>,
    body: web::Json<FoodSourceRequest>,
) -> Result<HttpResponse, AppError> {
    use sea_orm::{ConnectionTrait, Statement};
    let source = body.source.to_lowercase();
    if !["local", "fatsecret", "openfoodfacts", "hybrid"].contains(&source.as_str()) {
        return Err(AppError::BadRequest(
            "Invalid data source value".to_string(),
        ));
    }
    let query = Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "INSERT INTO system_settings (key, value) VALUES ('food_data_source', $1)
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
        [source.clone().into()],
    );
    db.execute(query).await?;
    Ok(HttpResponse::Ok().json(FoodSourceResponse { source }))
}

pub fn configure_import(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/v1/admin/import")
            .route("/scan", web::get().to(scan_folder))
            .route("/execute", web::post().to(execute_import))
            .route("/ingredients", web::post().to(import_ingredients)),
    );
    cfg.service(
        web::scope("/api/v1/admin/settings")
            .route("/food-source", web::get().to(get_food_source))
            .route("/food-source", web::post().to(update_food_source)),
    );
}
