//! Browse handlers — proxy food-api requests through app-api.
//!
//! All routes require JWT (enforced by the JwtAuth scope wrapper in main.rs).
//! The food-api key is added server-side so it never reaches the client.
//!
//! Routes:
//!   GET /api/browse/recipes             — list/search food-api recipes
//!   GET /api/browse/recipes/{id}        — full detail for a food-api recipe
//!   GET /api/browse/ingredients         — search food-api ingredients
//!   GET /api/browse/ingredients/{id}    — ingredient detail

use actix_web::{web, HttpRequest, HttpResponse};
use reqwest::Client;
use std::sync::Arc;

use cookest_shared::errors::AppError;

/// Thin wrapper around reqwest::Client carrying food-api config.
#[derive(Clone)]
pub struct FoodApiClient {
    pub client: Arc<Client>,
    pub base_url: String,
    pub api_key: Option<String>,
}

impl FoodApiClient {
    pub fn new(base_url: String, api_key: Option<String>) -> Self {
        Self {
            client: Arc::new(Client::new()),
            base_url,
            api_key,
        }
    }

    /// Build a GET request to the food-api, optionally attaching the API key.
    pub fn get(&self, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.get(&url);
        if let Some(key) = &self.api_key {
            req = req.header("X-API-Key", key);
        }
        req
    }
}

// ── Handlers ────────────────────────────────────────────────────────────────

/// GET /api/browse/recipes?q=&cuisine=&category=&difficulty=&page=&per_page=...
pub async fn list_browse_recipes(
    food: web::Data<FoodApiClient>,
    req: HttpRequest,
) -> Result<HttpResponse, AppError> {
    let query = req.query_string();
    let path = if query.is_empty() {
        "/api/v1/recipes".to_string()
    } else {
        format!("/api/v1/recipes?{}", query)
    };
    proxy_get(&food, &path).await
}

/// GET /api/browse/recipes/{id}
pub async fn get_browse_recipe(
    food: web::Data<FoodApiClient>,
    path: web::Path<i64>,
) -> Result<HttpResponse, AppError> {
    proxy_get(&food, &format!("/api/v1/recipes/{}", path.into_inner())).await
}

/// GET /api/browse/ingredients?q=&category=&page=&per_page=
pub async fn list_browse_ingredients(
    food: web::Data<FoodApiClient>,
    req: HttpRequest,
) -> Result<HttpResponse, AppError> {
    let query = req.query_string();
    let path = if query.is_empty() {
        "/api/v1/ingredients".to_string()
    } else {
        format!("/api/v1/ingredients?{}", query)
    };
    proxy_get(&food, &path).await
}

/// GET /api/browse/ingredients/{id}
pub async fn get_browse_ingredient(
    food: web::Data<FoodApiClient>,
    path: web::Path<i64>,
) -> Result<HttpResponse, AppError> {
    proxy_get(&food, &format!("/api/v1/ingredients/{}", path.into_inner())).await
}

// ── Route config ────────────────────────────────────────────────────────────

pub fn configure_browse(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/browse")
            .route("/recipes", web::get().to(list_browse_recipes))
            .route("/recipes/{id}", web::get().to(get_browse_recipe))
            .route("/ingredients", web::get().to(list_browse_ingredients))
            .route("/ingredients/{id}", web::get().to(get_browse_ingredient)),
    );
}

// ── Internal helper ─────────────────────────────────────────────────────────

async fn proxy_get(food: &FoodApiClient, path: &str) -> Result<HttpResponse, AppError> {
    let resp = food
        .get(path)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("food-api unreachable: {}", e)))?;

    let status = actix_web::http::StatusCode::from_u16(resp.status().as_u16())
        .unwrap_or(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR);

    let body = resp
        .bytes()
        .await
        .map_err(|e| AppError::Internal(format!("food-api read error: {}", e)))?;

    Ok(HttpResponse::build(status)
        .content_type("application/json")
        .body(body))
}
