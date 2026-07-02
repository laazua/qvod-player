use qvs_core::util::SimpleBencode;
use qvs_core::{InfoHash, QvodError, SwarmStatus};

#[must_use]
pub fn build_scrape_url(base_url: &str, info_hashes: &[InfoHash]) -> String {
    let mut url = format!("{}/scrape?", base_url.trim_end_matches('/'));
    for (i, ih) in info_hashes.iter().enumerate() {
        if i > 0 {
            url.push('&');
        }
        url.push_str("info_hash=");
        url.push_str(&ih.to_string());
    }
    url
}

#[allow(clippy::missing_errors_doc)]
pub fn parse_scrape_response(data: &[u8]) -> Result<Vec<(InfoHash, SwarmStatus)>, QvodError> {
    let (val, _) = qvs_core::util::simple_bencode_decode(data)
        .map_err(|e| QvodError::TrackerProtocol(e.clone()))?;

    let files_dict = val
        .as_dict()
        .and_then(|d| d.get(b"files".as_ref()))
        .and_then(|v| v.as_dict())
        .ok_or_else(|| QvodError::TrackerProtocol("scrape response missing files".into()))?;

    let mut results = Vec::new();
    for (key, val) in files_dict {
        let dict = val
            .as_dict()
            .ok_or_else(|| QvodError::TrackerProtocol("invalid file entry".into()))?;
        let complete = u32::try_from(
            dict.get(b"complete".as_ref())
                .and_then(SimpleBencode::as_int)
                .unwrap_or(0),
        )
        .unwrap_or(0);
        let incomplete = u32::try_from(
            dict.get(b"incomplete".as_ref())
                .and_then(SimpleBencode::as_int)
                .unwrap_or(0),
        )
        .unwrap_or(0);
        let downloaded = u32::try_from(
            dict.get(b"downloaded".as_ref())
                .and_then(SimpleBencode::as_int)
                .unwrap_or(0),
        )
        .unwrap_or(0);
        let mut info_hash = [0u8; 20];
        let len = key.len().min(20);
        info_hash[..len].copy_from_slice(&key[..len]);
        results.push((
            InfoHash(info_hash),
            SwarmStatus {
                complete,
                incomplete,
                downloaded,
            },
        ));
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_scrape_url() {
        let ih = InfoHash([0xAB; 20]);
        let url = build_scrape_url("http://tracker.example.com:6969", &[ih]);
        assert!(url.contains("/scrape?"));
        assert!(url.contains("info_hash="));
    }

    #[test]
    fn test_parse_scrape_response() {
        // Build a valid scrape response: d5:filesd20:<hash>d8:completei10e10:incompletei5e10:downloadedi100eee
        let mut dict_data = b"d5:filesd".to_vec();
        let hash = [0xABu8; 20];
        dict_data.extend_from_slice(b"20:");
        dict_data.extend_from_slice(&hash);
        dict_data.extend_from_slice(b"d8:completei10e10:incompletei5e10:downloadedi100eeee");
        let result = parse_scrape_response(&dict_data);
        assert!(result.is_ok(), "parse failed: {:?}", result.err());
        let results = result.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.complete, 10);
    }
}
