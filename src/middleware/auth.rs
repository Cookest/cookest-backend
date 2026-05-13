//! JWT Authentication Middleware
//!
//! Security properties:
//! - Zero-copy token extraction from Authorization header
//! - Validates token type (access vs refresh) — prevents refresh token reuse as access token
//! - Validates algorithm (HS256 only) — prevents algorithm confusion attacks
//! - Validates exp, iat, sub claims — rejects malformed or expired tokens
//! - Injects Claims into request extensions — handlers get a typed, already-validated struct
//! - No DB lookup on every request — fully stateless JWT validation (fast)
//! - All errors return the same generic 401 to prevent information leakage

use actix_web::{
    body::EitherBody,
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    http::StatusCode,
    Error, FromRequest, HttpMessage, HttpRequest, HttpResponse,
};
use futures::future::{ok, LocalBoxFuture, Ready};
use serde_json::json;
use std::future::{ready, Ready as StdReady};
use std::rc::Rc;
use std::sync::Arc;

use crate::services::token::{Claims, TokenService};

/// Convenience extractor for handlers — provides strongly-typed `id: Uuid` + full `claims`
pub struct AuthenticatedUser {
    pub id: uuid::Uuid,
    pub claims: Claims,
}

impl FromRequest for AuthenticatedUser {
    type Error = Error;
    type Future = StdReady<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut actix_web::dev::Payload) -> Self::Future {
        let result = req.extensions().get::<Claims>().map(|claims| {
            let id = uuid::Uuid::parse_str(&claims.sub)
                .unwrap_or_else(|_| uuid::Uuid::nil());
            AuthenticatedUser { id, claims: claims.clone() }
        });

        match result {
            Some(user) => ready(Ok(user)),
            None => {
                let err = actix_web::error::ErrorUnauthorized(
                    json!({ "error": "Unauthorized" }).to_string(),
                );
                ready(Err(err))
            }
        }
    }
}

/// Middleware factory — wrap a scope with `.wrap(JwtAuth::new(token_service))`
pub struct JwtAuth {
    token_service: Arc<TokenService>,
}

impl JwtAuth {
    pub fn new(token_service: Arc<TokenService>) -> Self {
        Self { token_service }
    }
}

impl<S, B> Transform<S, ServiceRequest> for JwtAuth
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type InitError = ();
    type Transform = JwtAuthMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(JwtAuthMiddleware {
            service: Rc::new(service),
            token_service: self.token_service.clone(),
        })
    }
}

pub struct JwtAuthMiddleware<S> {
    service: Rc<S>,
    token_service: Arc<TokenService>,
}

impl<S, B> Service<ServiceRequest> for JwtAuthMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = self.service.clone();
        let token_service = self.token_service.clone();

        Box::pin(async move {
            // ── Extract Bearer token ─────────────────────────────────────────
            let auth_header = req
                .headers()
                .get("Authorization")
                .and_then(|h| h.to_str().ok());

            // Constant-time comparison approach: always parse same code path
            let raw_token = match auth_header {
                Some(h) if h.len() > 7 && h[..7].eq_ignore_ascii_case("bearer ") => &h[7..],
                _ => return Ok(unauthorized(req, "Missing or malformed Authorization header")),
            };

            // ── Validate token (signature + expiry + type) ───────────────────
            let claims = match token_service.validate_access_token(raw_token) {
                Ok(c) => c,
                Err(e) => {
                    // Log internally if it's a suspicious error, but return generic 401
                    tracing::debug!("JWT validation failed: {:?}", e);
                    return Ok(unauthorized(req, "Invalid or expired token"));
                }
            };

            // Additional hardening: reject if sub is not a valid UUID
            if uuid::Uuid::parse_str(&claims.sub).is_err() {
                tracing::warn!("JWT with invalid sub UUID received");
                return Ok(unauthorized(req, "Invalid token subject"));
            }

            // ── Inject claims into request extensions ────────────────────────
            // Handlers use web::ReqData<Claims> or req.extensions().get::<Claims>()
            req.extensions_mut().insert(claims);

            // ── Continue to handler ──────────────────────────────────────────
            let res = service.call(req).await?;
            Ok(res.map_into_left_body())
        })
    }
}

/// Build a 401 Unauthorized response with a generic error body
fn unauthorized<B>(req: ServiceRequest, _reason: &str) -> ServiceResponse<EitherBody<B>> {
    let (http_req, _payload) = req.into_parts();
    let response = HttpResponse::build(StatusCode::UNAUTHORIZED)
        .content_type("application/json")
        .json(json!({ "error": "Unauthorized" }));
    ServiceResponse::new(http_req, response).map_into_right_body()
}
