use qvs_core::QvodError;

#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub pts_ms: u64,
    pub keyframe: bool,
}

pub struct VideoDecoder;

impl VideoDecoder {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    pub fn decode_video(&mut self, _packet: &[u8]) -> Result<VideoFrame, QvodError> {
        Err(QvodError::Decode("video decoder not available".into()))
    }
}

impl Default for VideoDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct AudioFrame {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
    pub pts_ms: u64,
}

pub struct AudioDecoder;

impl AudioDecoder {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    pub fn decode_audio(&mut self, _packet: &[u8]) -> Result<AudioFrame, QvodError> {
        Err(QvodError::Decode("audio decoder not available".into()))
    }
}

impl Default for AudioDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_video_decoder_creation() {
        let _decoder = VideoDecoder::new();
    }

    #[test]
    fn test_video_decode_not_available() {
        let mut decoder = VideoDecoder::new();
        let result = decoder.decode_video(b"test data");
        assert!(result.is_err());
    }

    #[test]
    fn test_audio_decoder_creation() {
        let _decoder = AudioDecoder::new();
    }

    #[test]
    fn test_audio_decode_not_available() {
        let mut decoder = AudioDecoder::new();
        let result = decoder.decode_audio(b"test data");
        assert!(result.is_err());
    }
}
