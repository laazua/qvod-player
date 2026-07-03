use qvs_core::QvodError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaFormat {
    Rmvb,
    Avi,
    Mkv,
    Mp4,
    Wmv,
    Flv,
    Mov,
    Ts,
    Webm,
    Unknown,
}

impl MediaFormat {
    #[must_use]
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "rmvb" | "rm" => Self::Rmvb,
            "avi" => Self::Avi,
            "mkv" => Self::Mkv,
            "mp4" => Self::Mp4,
            "wmv" => Self::Wmv,
            "flv" => Self::Flv,
            "mov" => Self::Mov,
            "ts" => Self::Ts,
            "webm" => Self::Webm,
            _ => Self::Unknown,
        }
    }

    #[must_use]
    pub fn from_magic(bytes: &[u8]) -> Self {
        if bytes.len() < 4 {
            return Self::Unknown;
        }
        if bytes.starts_with(b"\x00\x00\x00\x18ftypmp4") || bytes.starts_with(b"ftyp") {
            return Self::Mp4;
        }
        if bytes.starts_with(b"\x1a\x45\xdf\xa3") {
            return Self::Mkv;
        }
        if bytes.starts_with(b"FLV") {
            return Self::Flv;
        }
        if bytes.starts_with(b"RIFF") {
            return Self::Avi;
        }
        if bytes.starts_with(b"\x30\x26\xb2\x75") {
            return Self::Wmv;
        }
        if bytes.starts_with(b"\x2e\x52\x4d\x46") {
            return Self::Rmvb;
        }
        if bytes.starts_with(b"\x47\x40") {
            return Self::Ts;
        }
        Self::Unknown
    }
}

pub fn probe_format(path: &str) -> Result<MediaFormat, QvodError> {
    if let Some(ext) = std::path::Path::new(path).extension() {
        let fmt = MediaFormat::from_extension(&ext.to_string_lossy());
        if fmt != MediaFormat::Unknown {
            return Ok(fmt);
        }
    }
    Ok(MediaFormat::Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_extension() {
        assert_eq!(MediaFormat::from_extension("mp4"), MediaFormat::Mp4);
        assert_eq!(MediaFormat::from_extension("MKV"), MediaFormat::Mkv);
        assert_eq!(MediaFormat::from_extension("avi"), MediaFormat::Avi);
        assert_eq!(MediaFormat::from_extension("rmvb"), MediaFormat::Rmvb);
        assert_eq!(MediaFormat::from_extension("flv"), MediaFormat::Flv);
        assert_eq!(MediaFormat::from_extension("unknown"), MediaFormat::Unknown);
    }

    #[test]
    fn test_from_magic_mp4() {
        assert_eq!(
            MediaFormat::from_magic(b"\x00\x00\x00\x18ftypmp4"),
            MediaFormat::Mp4
        );
    }

    #[test]
    fn test_from_magic_mkv() {
        assert_eq!(
            MediaFormat::from_magic(b"\x1a\x45\xdf\xa3"),
            MediaFormat::Mkv
        );
    }

    #[test]
    fn test_from_magic_unknown() {
        assert_eq!(
            MediaFormat::from_magic(b"\x00\x00\x00\x00"),
            MediaFormat::Unknown
        );
    }

    #[test]
    fn test_from_magic_short() {
        assert_eq!(MediaFormat::from_magic(b"\x00"), MediaFormat::Unknown);
    }

    #[test]
    fn test_probe_format() {
        let result = probe_format("video.mp4").unwrap();
        assert_eq!(result, MediaFormat::Mp4);
        let result = probe_format("no_ext").unwrap();
        assert_eq!(result, MediaFormat::Unknown);
    }
}
