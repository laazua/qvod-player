use std::str::FromStr;

use sha1::Digest;

use qvs_core::{InfoHash, QvodError};

#[derive(Debug, Clone)]
pub struct HttpUrl {
    pub url: String,
    pub filename: String,
}

#[derive(Debug, Clone)]
pub enum MediaUri {
    Qvod(QvodUri),
    Http(HttpUrl),
    /// Local file path (file:// scheme or bare absolute path)
    File(String),
}

impl MediaUri {
    #[must_use]
    pub fn info_hash(&self) -> InfoHash {
        match self {
            MediaUri::Qvod(u) => u.info_hash,
            MediaUri::Http(h) => {
                let mut hasher = sha1::Sha1::new();
                hasher.update(h.url.as_bytes());
                let result: [u8; 20] = hasher.finalize().into();
                InfoHash(result)
            }
            MediaUri::File(path) => {
                let mut hasher = sha1::Sha1::new();
                hasher.update(path.as_bytes());
                let result: [u8; 20] = hasher.finalize().into();
                InfoHash(result)
            }
        }
    }

    #[must_use]
    pub fn filename(&self) -> &str {
        match self {
            MediaUri::Qvod(u) => &u.filename,
            MediaUri::Http(h) => &h.filename,
            MediaUri::File(path) => path
                .rsplit('/')
                .next()
                .and_then(|s| s.rsplit('\\').next())
                .unwrap_or(path),
        }
    }

    #[must_use]
    pub fn filesize(&self) -> u64 {
        match self {
            MediaUri::Qvod(u) => u.filesize,
            MediaUri::Http(_) | MediaUri::File(_) => 0,
        }
    }
}

impl FromStr for MediaUri {
    type Err = QvodError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with("qvod://") || s.starts_with("qvod:") {
            s.parse::<QvodUri>().map(MediaUri::Qvod)
        } else if s.starts_with("http://") || s.starts_with("https://") {
            let filename = s
                .rsplit('/')
                .next()
                .filter(|n| !n.is_empty())
                .unwrap_or("stream")
                .to_string();
            Ok(MediaUri::Http(HttpUrl {
                url: s.to_string(),
                filename,
            }))
        } else if let Some(path) = s.strip_prefix("file://") {
            // file:///path/to/file → /path/to/file
            let path = path.to_string();
            Ok(MediaUri::File(path))
        } else {
            // Absolute path on disk (must start with / or a drive letter)
            let is_abs_path = s.starts_with('/')
                || s.starts_with("\\\\")
                || s.as_bytes().first().is_some_and(|b| {
                    // Windows drive letter: C:\...
                    b.is_ascii_alphabetic()
                        && s.len() > 2
                        && s.as_bytes()[1] == b':'
                        && (s.as_bytes().get(2) == Some(&b'\\')
                            || s.as_bytes().get(2) == Some(&b'/'))
                });
            if is_abs_path {
                Ok(MediaUri::File(s.to_string()))
            } else {
                Err(QvodError::InvalidUri(
                    "unsupported URI scheme (expected qvod://, http(s)://, file://, or a local file path)"
                        .into(),
                ))
            }
        }
    }
}

impl std::fmt::Display for MediaUri {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MediaUri::Qvod(u) => write!(f, "{u}"),
            MediaUri::Http(h) => write!(f, "{}", h.url),
            MediaUri::File(path) => write!(f, "file://{path}"),
        }
    }
}

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

        if !s.ends_with('|') {
            return Err(QvodError::InvalidUri(
                "URI must end with trailing pipe '|'".into(),
            ));
        }

        let trimmed = s.strip_suffix('|').unwrap_or(s);
        let parts: Vec<&str> = trimmed.split('|').collect();
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
        assert!(err.to_string().contains("trailing pipe"));
    }

    #[test]
    fn test_missing_trailing_pipe() {
        let err = "qvod://a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0|file.mp4|1000|mp4"
            .parse::<QvodUri>()
            .unwrap_err();
        assert!(err.to_string().contains("trailing pipe"));
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

    #[test]
    fn test_media_uri_qvod() {
        let uri: MediaUri =
            "qvod://a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0|movie.mp4|734003200|rmvb|"
                .parse()
                .unwrap();
        assert!(matches!(uri, MediaUri::Qvod(_)));
        assert_eq!(uri.filename(), "movie.mp4");
        assert_eq!(uri.filesize(), 734003200);
    }

    #[test]
    fn test_media_uri_https() {
        let uri: MediaUri = "https://example.com/videos/movie.mp4".parse().unwrap();
        match &uri {
            MediaUri::Http(h) => {
                assert_eq!(h.url, "https://example.com/videos/movie.mp4");
                assert_eq!(h.filename, "movie.mp4");
            }
            _ => panic!("expected Http variant"),
        }
        assert_eq!(uri.filename(), "movie.mp4");
        assert_eq!(uri.filesize(), 0);
    }

    #[test]
    fn test_media_uri_http() {
        let uri: MediaUri = "http://example.com/video.avi".parse().unwrap();
        assert!(matches!(uri, MediaUri::Http(_)));
        assert_eq!(uri.filename(), "video.avi");
    }

    #[test]
    fn test_media_uri_unsupported_scheme() {
        let err = "ftp://example.com/file".parse::<MediaUri>().unwrap_err();
        assert!(
            err.to_string().contains("unsupported URI scheme")
                || err.to_string().contains("expected")
        );
    }

    #[test]
    fn test_media_uri_file_scheme() {
        let uri: MediaUri = "file:///home/user/video.mp4".parse().unwrap();
        assert!(matches!(uri, MediaUri::File(_)));
        assert_eq!(uri.filename(), "video.mp4");

        let uri: MediaUri = "file://C:/Users/user/video.mp4".parse().unwrap();
        assert!(matches!(uri, MediaUri::File(_)));
    }

    #[test]
    fn test_media_uri_abs_path_unix() {
        let uri: MediaUri = "/home/user/video.mp4".parse().unwrap();
        assert!(matches!(uri, MediaUri::File(_)));
        assert_eq!(uri.filename(), "video.mp4");
    }

    #[test]
    fn test_media_uri_info_hash_from_url() {
        let uri: MediaUri = "https://example.com/video.mp4".parse().unwrap();
        let hash = uri.info_hash();
        // SHA1 of "https://example.com/video.mp4" should be deterministic
        let expected = {
            let mut hasher = sha1::Sha1::new();
            hasher.update(b"https://example.com/video.mp4");
            let result: [u8; 20] = hasher.finalize().into();
            InfoHash(result)
        };
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_media_uri_roundtrip() {
        let qvod_uri = "qvod://a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0|file.mp4|1000|mp4|";
        let parsed: MediaUri = qvod_uri.parse().unwrap();
        assert_eq!(parsed.to_string(), qvod_uri);

        let http_uri = "https://example.com/video.mp4";
        let parsed: MediaUri = http_uri.parse().unwrap();
        assert_eq!(parsed.to_string(), http_uri);
    }

    #[test]
    fn test_media_uri_url_no_path() {
        let uri: MediaUri = "https://example.com".parse().unwrap();
        assert_eq!(uri.filename(), "example.com");
    }
}
