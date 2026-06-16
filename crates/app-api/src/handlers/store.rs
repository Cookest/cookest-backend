//! Store & admin handlers — store management, PDF upload, candidate review, price comparison
//!
//! Admin endpoints verify is_admin from DB (not JWT) for extra security.
//! Price comparison endpoints require Pro tier (checked via claims.tier).

use actix_multipart::Multipart;
use actix_web::{web, HttpResponse};
use futures::StreamExt;
use sea_orm::{DatabaseConnection, EntityTrait};
use std::sync::Arc;
use uuid::Uuid;

use crate::entity::user::Entity as User;
use cookest_shared::errors::AppError;
use crate::middleware::auth::AuthenticatedUser;
use crate::services::store::{CreateStoreRequest, NearbyQuery, StoreService};
use crate::services::token::SubscriptionTier;

/// Register all store-related routes onto `cfg`.
///
/// Public routes:
/// - `GET  /api/stores` — list all stores (no auth)
///
/// Admin routes (all require `is_admin = true` in DB):
/// - `POST /api/admin/stores`
/// - `POST /api/admin/stores/{store_id}/promotions/upload`
/// - `GET  /api/admin/stores/{store_id}/jobs`
/// - `GET  /api/admin/stores/{store_id}/candidates`
/// - `POST /api/admin/candidates/{id}/approve`
/// - `POST /api/admin/candidates/{id}/reject`
pub fn configure_stores(cfg: &mut web::ServiceConfig) {
    // Public list + nearby discovery + per-store promotions
    cfg.route("/api/stores", web::get().to(list_stores));
    cfg.route("/api/stores/nearby", web::get().to(nearby_stores));
    cfg.route("/api/stores/{id}/promotions", web::get().to(store_promotions));

    // Admin routes — require is_admin from DB
    cfg.service(
        web::scope("/api/admin")
            .route("/stores", web::post().to(create_store))
            .route("/stores/{store_id}/promotions/upload", web::post().to(upload_pdf))
            .route("/stores/{store_id}/jobs", web::get().to(list_jobs))
            .route("/stores/{store_id}/candidates", web::get().to(list_candidates))
            .route("/candidates/{id}/approve", web::post().to(approve_candidate))
            .route("/candidates/{id}/reject", web::post().to(reject_candidate)),
    );

    // Pro-gated price routes
    cfg.route("/api/ingredients/{id}/prices", web::get().to(get_prices_for_ingredient));
}

/// `GET /api/stores` — list all stores.
///
/// Public endpoint; no authentication required.
async fn list_stores(
    service: web::Data<Arc<StoreService>>,
) -> Result<HttpResponse, AppError> {
    let stores = service.list_stores().await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "stores": stores })))
}

/// `GET /api/stores/nearby?lat=&lng=&radius_km=` — supermarkets near a point.
///
/// Merges curated stores (with flyer prices) and OpenStreetMap POIs.
async fn nearby_stores(
    service: web::Data<Arc<StoreService>>,
    query: web::Query<NearbyQuery>,
) -> Result<HttpResponse, AppError> {
    let q = query.into_inner();
    let stores = service.nearby(q.lat, q.lng, q.radius_km.unwrap_or(5.0)).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "stores": stores })))
}

/// `GET /api/stores/{id}/promotions` — active promotions for a store.
async fn store_promotions(
    service: web::Data<Arc<StoreService>>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let promotions = service.list_store_promotions(path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "promotions": promotions })))
}

/// Verify the authenticated user is an admin by checking the DB
async fn verify_admin(user_id: Uuid, db: &DatabaseConnection) -> Result<(), AppError> {
    let user = User::find_by_id(user_id)
        .one(db)
        .await?
        .ok_or(AppError::Forbidden)?;
    if !user.is_admin {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

/// `POST /api/admin/stores` — create a new store.
///
/// Requires admin privileges (verified against DB, not just JWT claims).
async fn create_store(
    user: AuthenticatedUser,
    db: web::Data<DatabaseConnection>,
    service: web::Data<Arc<StoreService>>,
    body: web::Json<CreateStoreRequest>,
) -> Result<HttpResponse, AppError> {
    verify_admin(user.id, db.get_ref()).await?;
    let store = service.create_store(body.into_inner()).await?;
    Ok(HttpResponse::Created().json(store))
}

/// `POST /api/admin/stores/{store_id}/promotions/upload` — upload a promotions PDF.
///
/// Requires admin privileges.  The PDF is saved and a background
/// `pdf_processing_job` is spawned immediately; the handler returns 202
/// Accepted with the job record so the caller can poll job status.
async fn upload_pdf(
    user: AuthenticatedUser,
    db: web::Data<DatabaseConnection>,
    service: web::Data<Arc<StoreService>>,
    path: web::Path<Uuid>,
    mut payload: Multipart,
) -> Result<HttpResponse, AppError> {
    verify_admin(user.id, db.get_ref()).await?;
    let store_id = path.into_inner();

    let mut pdf_bytes: Vec<u8> = Vec::new();
    let mut filename = "upload.pdf".to_string();

    // Read multipart field (first file field only)
    while let Some(field) = payload.next().await {
        let mut field = field
            .map_err(|e| AppError::Internal(format!("Multipart error: {}", e)))?;

        if let Some(cd) = field.content_disposition() {
            if cd.get_name() == Some("file") {
                if let Some(name) = cd.get_filename() {
                    filename = name.to_string();
                }
                while let Some(chunk) = field.next().await {
                    let chunk = chunk
                        .map_err(|e| AppError::Internal(format!("Chunk error: {}", e)))?;
                    pdf_bytes.extend_from_slice(&chunk);
                    // 50 MB hard cap
                    if pdf_bytes.len() > 50 * 1024 * 1024 {
                        return Err(AppError::Validation(validator::ValidationErrors::new()));
                    }
                }
                break;
            }
        }
    }

    if pdf_bytes.is_empty() {
        return Err(AppError::Validation(validator::ValidationErrors::new()));
    }

    // Create the job and immediately spawn processing in background
    let job = service.create_pdf_job(store_id, pdf_bytes, &filename).await?;
    service.spawn_pdf_processing(job.id);

    Ok(HttpResponse::Accepted().json(serde_json::json!({
        "job": job,
        "message": "PDF uploaded and queued for processing"
    })))
}

/// `GET /api/admin/stores/{store_id}/jobs` — list PDF-processing jobs for a store.
///
/// Requires admin privileges.  Returns jobs of type `pdf_import` for the
/// given store so admins can monitor extraction progress.
async fn list_jobs(
    user: AuthenticatedUser,
    db: web::Data<DatabaseConnection>,
    service: web::Data<Arc<StoreService>>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    verify_admin(user.id, db.get_ref()).await?;
    let jobs = service.list_jobs(path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "jobs": jobs })))
}

/// `GET /api/admin/stores/{store_id}/candidates` — list promotion candidates.
///
/// Requires admin privileges.  Candidates are price/promotion records
/// extracted from a PDF that are awaiting human review before being
/// published as live store promotions.
async fn list_candidates(
    user: AuthenticatedUser,
    db: web::Data<DatabaseConnection>,
    service: web::Data<Arc<StoreService>>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    verify_admin(user.id, db.get_ref()).await?;
    let candidates = service.list_candidates(path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "candidates": candidates })))
}

/// `POST /api/admin/candidates/{id}/approve` — approve a promotion candidate.
///
/// Requires admin privileges.  Promotes the candidate record into a live
/// `store_promotion` row and records the approving admin’s ID.
async fn approve_candidate(
    user: AuthenticatedUser,
    db: web::Data<DatabaseConnection>,
    service: web::Data<Arc<StoreService>>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    verify_admin(user.id, db.get_ref()).await?;
    let promotion = service.approve_candidate(path.into_inner(), user.id).await?;
    Ok(HttpResponse::Created().json(promotion))
}

/// `POST /api/admin/candidates/{id}/reject` — reject a promotion candidate.
///
/// Requires admin privileges.  Marks the candidate as rejected so it is
/// excluded from future review queues without deleting the audit trail.
async fn reject_candidate(
    user: AuthenticatedUser,
    db: web::Data<DatabaseConnection>,
    service: web::Data<Arc<StoreService>>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    verify_admin(user.id, db.get_ref()).await?;
    service.reject_candidate(path.into_inner(), user.id).await?;
    Ok(HttpResponse::NoContent().finish())
}

/// Get active promotions for an ingredient — requires Pro tier
pub async fn get_prices_for_ingredient(
    user: AuthenticatedUser,
    service: web::Data<Arc<StoreService>>,
    path: web::Path<i64>,
) -> Result<HttpResponse, AppError> {
    let tier = user.claims.tier.as_ref().unwrap_or(&SubscriptionTier::Free);
    if !tier.is_pro_or_above() {
        return Err(AppError::SubscriptionRequired {
            feature: "price_comparison".to_string(),
        });
    }

    let promotions = service.get_promotions_for_ingredient(path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "promotions": promotions })))
}
