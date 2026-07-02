use std::sync::Arc;
use std::time::Duration;

use std::convert::Infallible;

use axum::{
    body::Body,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{Json, Response},
};
use std::fmt::Write;

use serde_json::{json, Value};
use tokio::sync::Mutex;
use tokio_stream::wrappers::ReceiverStream;

use qvs_core::{InfoHash, QvodError};
use qvs_stream::{QvodEngine, StreamState};

use crate::middleware::RateLimiter;
use crate::range::RangeHeader;

#[derive(serde::Deserialize)]
pub struct PlayParams {
    pub hash: String,
    pub name: Option<String>,
    pub size: Option<u64>,
}

#[derive(serde::Deserialize)]
pub struct SegmentParams {
    pub hash: String,
    pub offset: Option<u64>,
    pub length: Option<u64>,
    pub index: Option<u32>,
}

#[derive(serde::Deserialize)]
pub struct ControlParams {
    pub action: String,
    pub hash: Option<String>,
    pub value: Option<String>,
}

#[derive(serde::Serialize)]
pub struct ControlResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Clone)]
pub struct AppState {
    pub engine: Arc<Mutex<QvodEngine>>,
    pub rate_limiter: RateLimiter,
    pub start_time: tokio::time::Instant,
}

fn parse_info_hash(s: &str) -> Result<InfoHash, QvodError> {
    s.parse()
}

pub async fn handle_play(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PlayParams>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    let hash = &params.hash;
    let info_hash = parse_info_hash(hash).map_err(|e| {
        tracing::warn!("invalid info_hash: {e}");
        StatusCode::BAD_REQUEST
    })?;

    let uri = format!(
        "qvod://{hash}|{}|{}|mp4|",
        params.name.as_deref().unwrap_or("stream"),
        params.size.unwrap_or(0)
    );

    {
        let mut engine = state.engine.lock().await;
        engine.play(&uri).await.map_err(|e| {
            tracing::error!("play failed: {e:?}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    let file_size = {
        let engine = state.engine.lock().await;
        engine.file_size(&info_hash).unwrap_or(0)
    };

    let range_info = headers
        .get("range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| RangeHeader::parse(v, file_size))
        .and_then(|rh| rh.ranges.first().copied());

    let start_offset = range_info.map_or(0, |r| r.start);
    let chunk_size: u64 = 65536;

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Vec<u8>, Infallible>>(32);
    let engine_clone = state.engine.clone();

    tokio::spawn(async move {
        let mut position = start_offset;

        loop {
            let engine = engine_clone.lock().await;
            let status = engine.status(&info_hash).await;

            match status {
                Some(st) if st.state == StreamState::Ended => {
                    if let Some(data) = engine.read_buffer(&info_hash, position, chunk_size).await {
                        if !data.is_empty() {
                            let _ = tx.send(Ok(data)).await;
                        }
                    }
                    break;
                }
                Some(_) => {
                    if let Some(data) = engine.read_buffer(&info_hash, position, chunk_size).await {
                        if data.is_empty() {
                            drop(engine);
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            continue;
                        }
                        position += data.len() as u64;
                        if tx.send(Ok(data)).await.is_err() {
                            break;
                        }
                    } else {
                        drop(engine);
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
                None => break,
            }
        }
    });

    let stream = ReceiverStream::new(rx);

    let mut builder = Response::builder()
        .header("Content-Type", "video/mp4")
        .header("Access-Control-Allow-Origin", "*");

    if let Some(range) = range_info {
        builder = builder
            .header(
                "Content-Range",
                format!("bytes {}-{}/{}", range.start, range.end, file_size),
            )
            .status(StatusCode::PARTIAL_CONTENT);
    }

    Ok(builder.body(Body::from_stream(stream)).unwrap())
}

pub async fn handle_segment(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SegmentParams>,
) -> Result<Response, StatusCode> {
    let info_hash = parse_info_hash(&params.hash).map_err(|e| {
        tracing::warn!("invalid info_hash: {e}");
        StatusCode::BAD_REQUEST
    })?;

    let offset = params.offset.unwrap_or(0);
    let length = params.length.unwrap_or(65536);

    let engine = state.engine.lock().await;
    if let Some(data) = engine.read_buffer(&info_hash, offset, length).await {
        if data.is_empty() {
            return Err(StatusCode::NOT_FOUND);
        }
        Ok(Response::builder()
            .header("Content-Type", "video/MP2T")
            .header("Content-Length", data.len().to_string())
            .header("Access-Control-Allow-Origin", "*")
            .body(Body::from(data))
            .unwrap())
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

pub async fn handle_status(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PlayParams>,
) -> Json<Value> {
    let engine = state.engine.lock().await;

    if params.hash.is_empty() {
        let streams: Vec<Value> = engine
            .active_streams()
            .iter()
            .map(|ih| {
                let st = engine.status(ih);
                async {
                    match st.await {
                        Some(s) => json!({
                            "info_hash": ih.to_string(),
                            "state": format!("{:?}", s.state),
                            "position_ms": s.position_ms,
                            "duration_ms": s.duration_ms,
                            "buffered_seconds": s.buffered_seconds,
                            "download_progress": s.download_progress,
                            "peer_count": s.peer_count,
                        }),
                        None => json!({
                            "info_hash": ih.to_string(),
                            "state": "unknown",
                        }),
                    }
                }
            })
            .collect::<futures::future::JoinAll<_>>()
            .await
            .into_iter()
            .collect();

        Json(json!({
            "active_streams": streams,
            "stream_count": streams.len(),
            "uptime_secs": state.start_time.elapsed().as_secs(),
        }))
    } else {
        let Ok(info_hash) = parse_info_hash(&params.hash) else {
            return Json(json!({
                "state": "invalid_hash",
                "error": "info_hash must be 40 hex characters"
            }));
        };
        match engine.status(&info_hash).await {
            Some(s) => Json(json!({
                "state": format!("{:?}", s.state),
                "position_ms": s.position_ms,
                "duration_ms": s.duration_ms,
                "buffered_seconds": s.buffered_seconds,
                "download_progress": s.download_progress,
                "peer_count": s.peer_count,
            })),
            None => Json(json!({
                "state": "not_found",
            })),
        }
    }
}

pub async fn handle_control(
    State(state): State<Arc<AppState>>,
    Json(params): Json<ControlParams>,
) -> Json<ControlResponse> {
    match params.action.as_str() {
        "pause" => {
            let mut engine = state.engine.lock().await;
            engine.pause().await;
            Json(ControlResponse {
                success: true,
                message: "paused".into(),
            })
        }
        "resume" => {
            let mut engine = state.engine.lock().await;
            engine.resume().await;
            Json(ControlResponse {
                success: true,
                message: "resumed".into(),
            })
        }
        "stop" => {
            if let Some(hash) = &params.hash {
                match parse_info_hash(hash) {
                    Ok(ih) => {
                        let mut engine = state.engine.lock().await;
                        engine.stop(&ih);
                        Json(ControlResponse {
                            success: true,
                            message: "stopped".into(),
                        })
                    }
                    Err(e) => Json(ControlResponse {
                        success: false,
                        message: format!("invalid hash: {e}"),
                    }),
                }
            } else {
                Json(ControlResponse {
                    success: false,
                    message: "hash required".into(),
                })
            }
        }
        "seek" => {
            if let Some(value) = &params.value {
                match value.parse::<u64>() {
                    Ok(ms) => {
                        let mut engine = state.engine.lock().await;
                        match engine.seek(ms).await {
                            Ok(()) => Json(ControlResponse {
                                success: true,
                                message: format!("seeked to {ms}ms"),
                            }),
                            Err(e) => Json(ControlResponse {
                                success: false,
                                message: e.to_string(),
                            }),
                        }
                    }
                    Err(_) => Json(ControlResponse {
                        success: false,
                        message: "invalid timestamp".into(),
                    }),
                }
            } else {
                Json(ControlResponse {
                    success: false,
                    message: "value required".into(),
                })
            }
        }
        "status" => {
            let engine = state.engine.lock().await;
            let mut msg = String::new();
            for ih in engine.active_streams() {
                if let Some(st) = engine.status(&ih).await {
                    let _ = writeln!(
                        msg,
                        "{ih}: {:.1}% buffered, {:?}",
                        st.download_progress * 100.0,
                        st.state
                    );
                }
            }
            if msg.is_empty() {
                msg = "no active streams".into();
            }
            Json(ControlResponse {
                success: true,
                message: msg,
            })
        }
        _ => Json(ControlResponse {
            success: false,
            message: format!("unknown action: {}", params.action),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qvs_stream::EngineConfig;

    #[tokio::test]
    async fn test_control_pause() {
        let engine = Arc::new(Mutex::new(QvodEngine::new(EngineConfig::default()).await));
        let state = Arc::new(AppState {
            engine,
            rate_limiter: RateLimiter::new(100, 1),
            start_time: tokio::time::Instant::now(),
        });
        let params = ControlParams {
            action: "pause".into(),
            hash: None,
            value: None,
        };
        let result = handle_control(State(state), Json(params)).await;
        assert!(result.success);
        assert_eq!(result.message, "paused");
    }

    #[tokio::test]
    async fn test_control_unknown_action() {
        let engine = Arc::new(Mutex::new(QvodEngine::new(EngineConfig::default()).await));
        let state = Arc::new(AppState {
            engine,
            rate_limiter: RateLimiter::new(100, 1),
            start_time: tokio::time::Instant::now(),
        });
        let params = ControlParams {
            action: "nonexistent".into(),
            hash: None,
            value: None,
        };
        let result = handle_control(State(state), Json(params)).await;
        assert!(!result.success);
        assert_eq!(result.message, "unknown action: nonexistent");
    }

    #[tokio::test]
    async fn test_control_status_empty() {
        let engine = Arc::new(Mutex::new(QvodEngine::new(EngineConfig::default()).await));
        let state = Arc::new(AppState {
            engine,
            rate_limiter: RateLimiter::new(100, 1),
            start_time: tokio::time::Instant::now(),
        });
        let params = ControlParams {
            action: "status".into(),
            hash: None,
            value: None,
        };
        let result = handle_control(State(state), Json(params)).await;
        assert!(result.success);
        assert_eq!(result.message, "no active streams");
    }

    #[tokio::test]
    async fn test_control_seek_no_value() {
        let engine = Arc::new(Mutex::new(QvodEngine::new(EngineConfig::default()).await));
        let state = Arc::new(AppState {
            engine,
            rate_limiter: RateLimiter::new(100, 1),
            start_time: tokio::time::Instant::now(),
        });
        let params = ControlParams {
            action: "seek".into(),
            hash: None,
            value: None,
        };
        let result = handle_control(State(state), Json(params)).await;
        assert!(!result.success);
        assert_eq!(result.message, "value required");
    }

    #[tokio::test]
    async fn test_control_stop_requires_hash() {
        let engine = Arc::new(Mutex::new(QvodEngine::new(EngineConfig::default()).await));
        let state = Arc::new(AppState {
            engine,
            rate_limiter: RateLimiter::new(100, 1),
            start_time: tokio::time::Instant::now(),
        });
        let params = ControlParams {
            action: "stop".into(),
            hash: None,
            value: None,
        };
        let result = handle_control(State(state), Json(params)).await;
        assert!(!result.success);
        assert_eq!(result.message, "hash required");
    }

    #[tokio::test]
    async fn test_status_endpoint_returns_json() {
        let engine = Arc::new(Mutex::new(QvodEngine::new(EngineConfig::default()).await));
        let state = Arc::new(AppState {
            engine,
            rate_limiter: RateLimiter::new(100, 1),
            start_time: tokio::time::Instant::now(),
        });
        let params = PlayParams {
            hash: String::new(),
            name: None,
            size: None,
        };
        let result = handle_status(State(state), Query(params)).await;
        assert!(result.0.get("active_streams").is_some());
        assert_eq!(result.0["stream_count"], 0);
    }
}
