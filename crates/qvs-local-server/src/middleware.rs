use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use tokio::sync::Mutex;

#[derive(Clone, Default)]
struct RateLimiterInner {
    requests: HashMap<IpAddr, Vec<Instant>>,
}

#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<Mutex<RateLimiterInner>>,
    max_requests: usize,
    window: Duration,
}

impl RateLimiter {
    #[must_use]
    pub fn new(max_requests: usize, window_secs: u64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(RateLimiterInner::default())),
            max_requests,
            window: Duration::from_secs(window_secs),
        }
    }

    pub async fn check(&self, ip: IpAddr) -> bool {
        let now = Instant::now();
        let mut inner = self.inner.lock().await;
        let timestamps = inner.requests.entry(ip).or_default();
        timestamps.retain(|t| now.duration_since(*t) < self.window);
        if timestamps.len() >= self.max_requests {
            false
        } else {
            timestamps.push(now);
            true
        }
    }
}

pub async fn request_logger(req: Request, next: Next) -> Response {
    let start = Instant::now();
    let method = req.method().clone();
    let uri = req.uri().clone();
    let ua = req
        .headers()
        .get("User-Agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_owned();
    let response = next.run(req).await;
    let status = response.status();
    let elapsed = start.elapsed();
    tracing::info!("{method} {uri} -> {status} ({elapsed:?}) [{ua}]");
    response
}

pub async fn cors_middleware(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    response
        .headers_mut()
        .insert("Access-Control-Allow-Origin", "*".parse().unwrap());
    response.headers_mut().insert(
        "Access-Control-Allow-Methods",
        "GET, POST, OPTIONS".parse().unwrap(),
    );
    response.headers_mut().insert(
        "Access-Control-Allow-Headers",
        "Content-Type".parse().unwrap(),
    );
    response
}

pub async fn handle_options() -> impl IntoResponse {
    (
        [(axum::http::header::ALLOW, "GET, POST, OPTIONS")],
        StatusCode::NO_CONTENT,
    )
}

pub async fn rate_limit_middleware(
    State(limiter): State<RateLimiter>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let ip = req
        .headers()
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .and_then(|v| v.parse::<IpAddr>().ok())
        .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));

    if !limiter.check(ip).await {
        tracing::warn!("rate_limit: too many requests from {ip}");
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_allows_first_request() {
        let limiter = RateLimiter::new(10, 60);
        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        assert!(limiter.check(ip).await);
    }

    #[tokio::test]
    async fn test_rate_limiter_blocks_excessive_requests() {
        let limiter = RateLimiter::new(3, 60);
        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        assert!(limiter.check(ip).await);
        assert!(limiter.check(ip).await);
        assert!(limiter.check(ip).await);
        assert!(!limiter.check(ip).await);
    }

    #[tokio::test]
    async fn test_handle_options() {
        let response = handle_options().await.into_response();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
}
