use qvs_core::{FileMeta, InfoHash, MediaStream, QvodError};

#[derive(Debug, Clone)]
pub struct MediaInfo {
    pub codec: String,
    pub resolution: (u32, u32),
    pub bitrate: u64,
    pub duration_ms: u64,
}

impl Default for MediaInfo {
    fn default() -> Self {
        Self {
            codec: String::new(),
            resolution: (0, 0),
            bitrate: 0,
            duration_ms: 0,
        }
    }
}

pub struct Demuxer;

impl Demuxer {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    pub fn open(_path: &str) -> Result<Self, QvodError> {
        Ok(Self)
    }

    pub fn read_frame(&mut self) -> Result<MediaFrame, QvodError> {
        Err(QvodError::UnsupportedFormat("ffmpeg not available".into()))
    }

    pub fn seek(&mut self, _timestamp_ms: u64) -> Result<(), QvodError> {
        Err(QvodError::UnsupportedFormat("ffmpeg not available".into()))
    }

    #[must_use]
    pub fn info(&self) -> MediaInfo {
        MediaInfo::default()
    }
}

impl Default for Demuxer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct MediaFrame {
    pub data: Vec<u8>,
    pub pts_ms: u64,
    pub keyframe: bool,
}

pub struct FfmpegDemuxer;

impl FfmpegDemuxer {
    pub fn open(_path: &str) -> Result<Self, QvodError> {
        Err(QvodError::UnsupportedFormat(
            "ffmpeg not available in this build".into(),
        ))
    }
}

#[must_use]
pub fn extract_metadata(_stream: &MediaStream) -> FileMeta {
    FileMeta {
        info_hash: InfoHash([0u8; 20]),
        filename: String::new(),
        file_size: 0,
        piece_length: 262_144,
        pieces: Vec::new(),
        keyframe_index: None,
        duration_ms: 0,
        video_codec: None,
        audio_codec: None,
        width: 0,
        height: 0,
        bitrate: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_demuxer_open() {
        let result = Demuxer::open("/path/to/video.mp4");
        assert!(result.is_ok());
    }

    #[test]
    fn test_demuxer_info() {
        let demuxer = Demuxer::new();
        let info = demuxer.info();
        assert_eq!(info.duration_ms, 0);
    }

    #[test]
    fn test_ffmpeg_demuxer() {
        let result = FfmpegDemuxer::open("/path/to/video.mp4");
        assert!(result.is_err());
    }

    #[test]
    fn test_read_frame_not_available() {
        let mut demuxer = Demuxer::new();
        let result = demuxer.read_frame();
        assert!(result.is_err());
    }
}
