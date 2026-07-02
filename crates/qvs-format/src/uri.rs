use std::str::FromStr;

use qvs_core::{InfoHash, QvodError};

#[derive(Debug, Clone)]
pub struct QvodUri {
    pub info_hash: InfoHash,
    pub filename: String,
    pub filesize: u64,
    pub format: String,
}

impl QvodUri {
    #[must_use]
    pub fn new(info_hash: InfoHash, filename: String, filesize: u64, format: String) -> Self {
        Self {
            info_hash,
            filename,
            filesize,
            format,
        }
    }
}

impl FromStr for QvodUri {
    type Err = QvodError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s
            .strip_prefix("qvod://")
            .or_else(|| s.strip_prefix("qvod:"))
            .ok_or_else(|| QvodError::InvalidUri("missing qvod:// prefix".into()))?;

        let parts: Vec<&str> = s.split('|').collect();
        if parts.len() < 4 {
            return Err(QvodError::InvalidUri(format!(
                "expected at least 4 pipe-delimited parts, got {}",
                parts.len()
            )));
        }

        let hash_str = parts[0];
        if hash_str.len() != 40 {
            return Err(QvodError::InvalidUri(format!(
                "info_hash must be 40 hex chars, got {}",
                hash_str.len()
            )));
        }
        let info_hash: InfoHash = hash_str.parse()?;

        let filename = parts[1].to_string();
        if filename.is_empty() {
            return Err(QvodError::InvalidUri("filename cannot be empty".into()));
        }

        let filesize: u64 = parts[2]
            .parse()
            .map_err(|e| QvodError::InvalidUri(format!("invalid filesize: {e}")))?;

        let format = parts[3].to_string();
        if format.is_empty() {
            return Err(QvodError::InvalidUri("format cannot be empty".into()));
        }

        Ok(Self {
            info_hash,
            filename,
            filesize,
            format,
        })
    }
}

impl std::fmt::Display for QvodUri {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "qvod://{}|{}|{}|{}|",
            self.info_hash, self.filename, self.filesize, self.format
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_uri_roundtrip() {
        let uri_str = "qvod://a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0|movie.mp4|734003200|rmvb|";
        let uri: QvodUri = uri_str.parse().unwrap();
        assert_eq!(uri.filename, "movie.mp4");
        assert_eq!(uri.filesize, 734003200);
        assert_eq!(uri.format, "rmvb");
        assert_eq!(uri.to_string(), uri_str);
    }

    #[test]
    fn test_missing_prefix() {
        let err = "invalid://hash".parse::<QvodUri>().unwrap_err();
        assert!(err.to_string().contains("missing qvod:// prefix"));
    }

    #[test]
    fn test_too_few_parts() {
        let err = "qvod://hash".parse::<QvodUri>().unwrap_err();
        assert!(err.to_string().contains("expected at least 4"));
    }

    #[test]
    fn test_invalid_hash_length() {
        let err = "qvod://short|file.mp4|1000|mp4|"
            .parse::<QvodUri>()
            .unwrap_err();
        assert!(err.to_string().contains("must be 40 hex chars"));
    }

    #[test]
    fn test_empty_filename() {
        let err = "qvod://a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0||1000|mp4|"
            .parse::<QvodUri>()
            .unwrap_err();
        assert!(err.to_string().contains("filename cannot be empty"));
    }
}
