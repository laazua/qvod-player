use std::time::Duration;

use qvs_core::QvodError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// HTTP client for connecting to a remote QVOD server.
/// When server_url is set, the GUI operates in client mode
/// and delegates all engine operations to the remote server.
#[derive(Clone)]
pub struct ServerClient {
    base_url: String,
    client: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamStatusResponse {
    pub state: String,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub buffered_seconds: f64,
    pub download_progress: f64,
    pub peer_count: usize,
}

#[derive(Deserialize)]
pub struct ControlResponse {
    pub success: bool,
    pub message: String,
}

impl ServerClient {
    pub fn new(base_url: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Self { base_url, client }
    }

    pub fn play_url(&self, hash: &str) -> String {
        format!("{}/play?hash={}", self.base_url, hash)
    }

    pub fn segment_url(&self, hash: &str, index: u32) -> String {
        format!("{}/segment?hash={}&index={}", self.base_url, hash, index)
    }

    pub fn status_url(&self, hash: &str) -> String {
        format!("{}/status?hash={}", self.base_url, hash)
    }

    pub async fn play(&self, hash: &str) -> Result<(), QvodError> {
        let url = format!("{}/control", self.base_url);
        let body = serde_json::json!({
            "action": "play",
            "hash": hash,
        });
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                QvodError::Network(std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    e.to_string(),
                ))
            })?;
        let ctrl: ControlResponse = resp
            .json()
            .await
            .map_err(|e| QvodError::Protocol(e.to_string()))?;
        if ctrl.success {
            Ok(())
        } else {
            Err(QvodError::Protocol(ctrl.message))
        }
    }

    pub async fn pause(&self) -> Result<(), QvodError> {
        let url = format!("{}/control", self.base_url);
        let body = serde_json::json!({ "action": "pause" });
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                QvodError::Network(std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    e.to_string(),
                ))
            })?;
        let ctrl: ControlResponse = resp
            .json()
            .await
            .map_err(|e| QvodError::Protocol(e.to_string()))?;
        if ctrl.success {
            Ok(())
        } else {
            Err(QvodError::Protocol(ctrl.message))
        }
    }

    pub async fn resume(&self) -> Result<(), QvodError> {
        let url = format!("{}/control", self.base_url);
        let body = serde_json::json!({ "action": "resume" });
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                QvodError::Network(std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    e.to_string(),
                ))
            })?;
        let ctrl: ControlResponse = resp
            .json()
            .await
            .map_err(|e| QvodError::Protocol(e.to_string()))?;
        if ctrl.success {
            Ok(())
        } else {
            Err(QvodError::Protocol(ctrl.message))
        }
    }

    pub async fn stop(&self, hash: &str) -> Result<(), QvodError> {
        let url = format!("{}/control", self.base_url);
        let body = serde_json::json!({
            "action": "stop",
            "hash": hash,
        });
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                QvodError::Network(std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    e.to_string(),
                ))
            })?;
        let ctrl: ControlResponse = resp
            .json()
            .await
            .map_err(|e| QvodError::Protocol(e.to_string()))?;
        if ctrl.success {
            Ok(())
        } else {
            Err(QvodError::Protocol(ctrl.message))
        }
    }

    pub async fn seek(&self, hash: &str, timestamp_ms: u64) -> Result<(), QvodError> {
        let url = format!("{}/control", self.base_url);
        let body = serde_json::json!({
            "action": "seek",
            "hash": hash,
            "value": timestamp_ms.to_string(),
        });
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                QvodError::Network(std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    e.to_string(),
                ))
            })?;
        let ctrl: ControlResponse = resp
            .json()
            .await
            .map_err(|e| QvodError::Protocol(e.to_string()))?;
        if ctrl.success {
            Ok(())
        } else {
            Err(QvodError::Protocol(ctrl.message))
        }
    }

    pub async fn get_status(&self, hash: &str) -> Result<StreamStatusResponse, QvodError> {
        let url = self.status_url(hash);
        let resp = self.client.get(&url).send().await.map_err(|e| {
            QvodError::Network(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                e.to_string(),
            ))
        })?;
        let status: StreamStatusResponse = resp
            .json()
            .await
            .map_err(|e| QvodError::Protocol(e.to_string()))?;
        Ok(status)
    }

    pub async fn get_server_status(&self) -> Result<Value, QvodError> {
        let url = format!("{}/status", self.base_url);
        let resp = self.client.get(&url).send().await.map_err(|e| {
            QvodError::Network(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                e.to_string(),
            ))
        })?;
        let v: Value = resp
            .json()
            .await
            .map_err(|e| QvodError::Protocol(e.to_string()))?;
        Ok(v)
    }

    pub async fn ping(&self) -> Result<(), QvodError> {
        let url = format!("{}/status", self.base_url);
        self.client.get(&url).send().await.map_err(|e| {
            QvodError::Network(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                e.to_string(),
            ))
        })?;
        Ok(())
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Update the base URL (e.g. when settings change).
    pub fn set_base_url(&mut self, url: String) {
        self.base_url = url;
    }
}
