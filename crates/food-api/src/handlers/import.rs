//! Import handlers — folder scan and dataset execution endpoints
//!
//! Routes (admin-only, no JWT here; JWT verified by app-api proxy):
//!   GET  /api/v1/admin/import/scan?folder=...
//!   POST /api/v1/admin/import/execute

use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::errors::AppError;
use crate::services::ImportService;

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
        if body.filename.ends_with(".json") { "json" } else { "csv" }
    });
    let result = import_svc.import_file(folder, &body.filename, format).await?;
    Ok(HttpResponse::Ok().json(ExecuteResponse {
        success: true,
        rows_imported: result.rows_imported,
        message: result.message,
    }))
}

pub fn configure_import(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/v1/admin/import")
            .route("/scan", web::get().to(scan_folder))
            .route("/execute", web::post().to(execute_import)),
    );
}
