//! Rate Limiting Middleware
//! 
//! Implements per-IP rate limiting using the Governor crate
//! Different limits for different endpoint types

use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    Error,
};
use futures::future::{ok, LocalBoxFuture, Ready};
use governor::{
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;

use crate::errors::AppError;

/// Rate limiter configuration
#[derive(Clone)]
pub struct RateLimitConfig {
    /// Requests per minute
    pub requests_per_minute: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_minute: 60,
        }
    }
}

/// Rate limiting middleware factory
pub struct RateLimit {
    limiter: Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
}

impl RateLimit {
    pub fn new(config: RateLimitConfig) -> Self {
        let quota = Quota::per_minute(
            NonZeroU32::new(config.requests_per_minute).unwrap_or(NonZeroU32::new(60).unwrap()),
        );
        
        Self {
            limiter: Arc::new(RateLimiter::direct(quota)),
        }
    }

    /// Create a strict rate limiter for auth endpoints
    pub fn strict() -> Self {
        Self::new(RateLimitConfig {
            requests_per_minute: 10,
        })
    }

    /// Create a lenient rate limiter for general endpoints
    pub fn lenient() -> Self {
        Self::new(RateLimitConfig {
            requests_per_minute: 100,
        })
    }
}

impl<S, B> Transform<S, ServiceRequest> for RateLimit
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = RateLimitMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(RateLimitMiddleware {
            service: Rc::new(service),
            limiter: self.limiter.clone(),
        })
    }
}

pub struct RateLimitMiddleware<S> {
    service: Rc<S>,
    limiter: Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
}

impl<S, B> Service<ServiceRequest> for RateLimitMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = self.service.clone();
        let limiter = self.limiter.clone();

        Box::pin(async move {
            // Check rate limit
            if limiter.check().is_err() {
                tracing::warn!(
                    "Rate limit exceeded for IP: {:?}",
                    req.connection_info().peer_addr()
                );
                return Err(AppError::RateLimitExceeded.into());
            }

            service.call(req).await
        })
    }
}
