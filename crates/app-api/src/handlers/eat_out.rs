//! Eat-out handlers — find nearby restaurants via OpenStreetMap's Overpass API.
//!
//! Key-free, matching the app's existing OpenFreeMap/MapLibre stack. The user
//! declares a meal as "eat out" on a slot; this endpoint lists nearby places so
//! the user (and the AI) can choose one. Overpass returns no ratings, so ranking
//! is left to the client/AI on cuisine, price and distance.

use actix_web::{web, HttpResponse};
use reqwest::Client;
use serde::Deserialize;

use crate::middleware::auth::AuthenticatedUser;
use cookest_shared::errors::AppError;

pub fn configure_eat_out(cfg: &mut web::ServiceConfig) {
    cfg.service(web::scope("/api/eat-out").route("/nearby", web::get().to(nearby)));
}

#[derive(Debug, Deserialize)]
struct NearbyQuery {
    lat: f64,
    lng: f64,
    /// Search radius in metres (default 1500, clamped to 5000).
    radius: Option<u32>,
}

/// `GET /api/eat-out/nearby?lat=..&lng=..&radius=..` — nearby restaurants/cafés.
async fn nearby(
    _user: AuthenticatedUser,
    query: web::Query<NearbyQuery>,
) -> Result<HttpResponse, AppError> {
    let q = query.into_inner();
    let radius = q.radius.unwrap_or(1500).min(5000);

    // Overpass QL: restaurants, cafés, fast food within the radius.
    let ql = format!(
        r#"[out:json][timeout:20];
        (
          node["amenity"~"restaurant|cafe|fast_food"](around:{r},{lat},{lng});
          way["amenity"~"restaurant|cafe|fast_food"](around:{r},{lat},{lng});
        );
        out center 40;"#,
        r = radius,
        lat = q.lat,
        lng = q.lng,
    );

    let client = Client::new();
    let resp = client
        .post("https://overpass-api.de/api/interpreter")
        .header("User-Agent", "Cookest/1.0 (eat-out)")
        .body(ql)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Overpass request failed: {e}")))?;

    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("Overpass parse failed: {e}")))?;

    let empty = vec![];
    let elements = data.get("elements").and_then(|e| e.as_array()).unwrap_or(&empty);

    let places: Vec<serde_json::Value> = elements
        .iter()
        .filter_map(|el| {
            let tags = el.get("tags")?;
            let name = tags.get("name")?.as_str()?.to_string();
            // Coordinates live on the node directly, or on `center` for ways.
            let (lat, lng) = match (el.get("lat"), el.get("lon")) {
                (Some(la), Some(lo)) => (la.as_f64()?, lo.as_f64()?),
                _ => {
                    let c = el.get("center")?;
                    (c.get("lat")?.as_f64()?, c.get("lon")?.as_f64()?)
                }
            };
            Some(serde_json::json!({
                "name": name,
                "lat": lat,
                "lng": lng,
                "amenity": tags.get("amenity").and_then(|v| v.as_str()),
                "cuisine": tags.get("cuisine").and_then(|v| v.as_str()),
                "address": tags.get("addr:street").and_then(|v| v.as_str()),
            }))
        })
        .collect();

    Ok(HttpResponse::Ok().json(serde_json::json!({ "places": places })))
}
