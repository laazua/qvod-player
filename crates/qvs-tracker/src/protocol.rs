use std::collections::BTreeMap;

use qvs_core::util::{simple_bencode_decode, SimpleBencode};
use qvs_core::{AnnounceEvent, InfoHash, QvodError};
use std::fmt::Write;

pub struct AnnounceParams {
    pub info_hash: InfoHash,
    pub peer_id: [u8; 20],
    pub port: u16,
    pub uploaded: u64,
    pub downloaded: u64,
    pub left: u64,
    pub event: AnnounceEvent,
    pub compact: bool,
}

impl AnnounceParams {
    #[must_use]
    pub fn to_query(&self) -> String {
        let mut params = Vec::new();
        params.push(format!("info_hash={}", url_encode(&self.info_hash.0)));
        params.push(format!("peer_id={}", url_encode(&self.peer_id)));
        params.push(format!("port={}", self.port));
        params.push(format!("uploaded={}", self.uploaded));
        params.push(format!("downloaded={}", self.downloaded));
        params.push(format!("left={}", self.left));
        params.push(format!("event={}", self.event.as_str()));
        if self.compact {
            params.push("compact=1".to_string());
        }
        params.join("&")
    }
}

fn url_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len() * 3);
    for &b in data {
        match b {
            b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

#[derive(Debug, Clone)]
pub struct AnnounceResponse {
    pub interval: u32,
    pub min_interval: Option<u32>,
    pub complete: u32,
    pub incomplete: u32,
    pub downloaded: u32,
    pub peers: Vec<(Vec<u8>, u16)>,
}

impl AnnounceResponse {
    #[allow(clippy::missing_errors_doc)]
    pub fn from_bencode(data: &[u8]) -> Result<Self, QvodError> {
        let (val, _) = simple_bencode_decode(data).map_err(QvodError::TrackerProtocol)?;

        let dict = val
            .as_dict()
            .ok_or_else(|| QvodError::TrackerProtocol("response not a dict".into()))?;

        let interval = u32::try_from(dict_get_int(dict, b"interval")?)
            .map_err(|_| QvodError::TrackerProtocol("interval out of range".into()))?;
        let min_interval = dict_get_int(dict, b"min interval")
            .ok()
            .and_then(|v| u32::try_from(v).ok());
        let complete = u32::try_from(dict_get_int(dict, b"complete").unwrap_or(0)).unwrap_or(0);
        let incomplete = u32::try_from(dict_get_int(dict, b"incomplete").unwrap_or(0)).unwrap_or(0);
        let downloaded = u32::try_from(dict_get_int(dict, b"downloaded").unwrap_or(0)).unwrap_or(0);

        let peers = if let Some(peers_val) = dict.get(&b"peers"[..]) {
            if let Some(peers_bytes) = peers_val.as_str_bytes() {
                qvs_core::util::parse_compact_peers(peers_bytes)
            } else if let Some(peers_list) = peers_val.as_list() {
                qvs_core::util::parse_dict_peers(peers_list)
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        Ok(Self {
            interval,
            min_interval,
            complete,
            incomplete,
            downloaded,
            peers,
        })
    }
}

fn dict_get_int(dict: &BTreeMap<Vec<u8>, SimpleBencode>, key: &[u8]) -> Result<i64, QvodError> {
    dict.get(key)
        .and_then(SimpleBencode::as_int)
        .ok_or_else(|| {
            QvodError::TrackerProtocol(format!("missing field: {:?}", String::from_utf8_lossy(key)))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_announce_params_query() {
        let params = AnnounceParams {
            info_hash: InfoHash([0u8; 20]),
            peer_id: [1u8; 20],
            port: 8621,
            uploaded: 0,
            downloaded: 0,
            left: 1000,
            event: AnnounceEvent::Started,
            compact: true,
        };
        let query = params.to_query();
        assert!(query.contains("info_hash="));
        assert!(query.contains("port=8621"));
        assert!(query.contains("event=started"));
        assert!(query.contains("compact=1"));
    }

    #[test]
    fn test_announce_response_empty_peers() {
        let data = b"d8:intervali1800e12:min intervali900e8:completei10e10:incompletei5e5:peers0:e";
        let resp = AnnounceResponse::from_bencode(data).unwrap();
        assert_eq!(resp.interval, 1800);
        assert_eq!(resp.min_interval, Some(900));
        assert_eq!(resp.complete, 10);
        assert_eq!(resp.incomplete, 5);
        assert!(resp.peers.is_empty());
    }
}
