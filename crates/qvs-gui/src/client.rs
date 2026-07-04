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
        let encoded = urlencoding(hash);
        format!("{}/play?hash={encoded}", self.base_url)
    }

    pub fn segment_url(&self, hash: &str, index: u32) -> String {
        let encoded = urlencoding(hash);
        format!("{}/segment?hash={encoded}&index={index}", self.base_url)
    }

    /// Play a file:// or http(s):// URL via the remote server (uses `url` field).
    pub async fn play_uri(&self, uri: &str) -> Result<(), QvodError> {
        let ctrl_url = format!("{}/control", self.base_url);
        let body = serde_json::json!({
            "action": "play",
            "url": uri,
        });
        let resp = self
            .client
            .post(&ctrl_url)
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
        let url = format!("{}/status", self.base_url);
        let resp = self
            .client
            .get(&url)
            .query(&[("hash", hash)])
            .send()
            .await
            .map_err(|e| {
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

/// Minimal URL-encoding for query parameter values.
/// Only encodes characters that are not valid in query strings
/// (`://` etc are valid in query values, but we percent-encode
/// everything non-alphanumeric for maximum compatibility).
fn urlencoding(s: &str) -> String {
    use std::fmt::Write as _;
    let mut encoded = String::with_capacity(s.len() + 8);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            b' ' => encoded.push_str("%20"),
            _ => {
                let _ = write!(encoded, "%{byte:02X}");
            }
        }
    }
    encoded
}
