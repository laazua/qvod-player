use std::sync::Arc;

use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use tokio::net::TcpListener;

use qvs_stream::QvodEngine;

use crate::config::LocalServerConfig;
use crate::handler::{handle_control, handle_play, handle_segment, handle_status, AppState};
use crate::middleware::{cors_middleware, rate_limit_middleware, request_logger, RateLimiter};

pub struct LocalServer {
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    port: u16,
}

impl LocalServer {
    pub async fn new(
        config: &LocalServerConfig,
        engine: QvodEngine,
    ) -> Result<Self, qvs_core::QvodError> {
        let port = config
            .find_available_port_async()
            .await
            .ok_or_else(|| qvs_core::QvodError::Server("no available port".into()))?;

        let rate_limiter = RateLimiter::new(100, 1);
        let state = Arc::new(AppState {
            engine: Arc::new(tokio::sync::Mutex::new(engine)),
            rate_limiter: rate_limiter.clone(),
            start_time: tokio::time::Instant::now(),
        });

        let app = Router::new()
            .route("/play", get(handle_play))
            .route("/status", get(handle_status))
            .route("/segment", get(handle_segment))
            .route("/control", post(handle_control))
            .layer(middleware::from_fn(cors_middleware))
            .layer(middleware::from_fn_with_state(
                rate_limiter,
                rate_limit_middleware,
            ))
            .layer(middleware::from_fn(request_logger))
            .with_state(state);

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        tokio::spawn(async move {
            let addr = format!("127.0.0.1:{port}");
            let listener = match TcpListener::bind(&addr).await {
                Ok(l) => l,
                Err(e) => {
                    tracing::error!("Failed to bind {addr}: {e}");
                    return;
                }
            };
            tracing::info!("Local server listening on {addr}");

            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    tokio::select! {
                        _ = shutdown_rx => {
                            tracing::info!("Shutdown signal received");
                        }
                        () = shutdown_signal() => {
                            tracing::info!("OS shutdown signal received");
                        }
                    }
                })
                .await
                .unwrap_or_else(|e| {
                    tracing::error!("Server error: {e}");
                });
        });

        Ok(Self {
            shutdown_tx: Some(shutdown_tx),
            port,
        })
    }

    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }

    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();
        tokio::select! {
            _ = ctrl_c => {}
            () = async { if let Some(sig) = &mut term { sig.recv().await; } } => {}
        }
    }
    #[cfg(not(unix))]
    {
        ctrl_c.await.ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qvs_stream::EngineConfig;

    #[tokio::test]
    async fn test_server_creation() {
        let config = LocalServerConfig::new(9998);
        let engine = QvodEngine::new(EngineConfig::default()).await;
        let server = LocalServer::new(&config, engine).await;
        assert!(server.is_ok());
        if let Ok(srv) = server {
            assert!(srv.port() > 0);
        }
    }

    #[tokio::test]
    async fn test_server_stop() {
        let config = LocalServerConfig::new(9997);
        let engine = QvodEngine::new(EngineConfig::default()).await;
        let mut server = LocalServer::new(&config, engine).await.unwrap();
        server.stop();
    }
}
