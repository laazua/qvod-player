use qvs_core::{FileMeta, QvodError};

const TARGET_DURATION: u64 = 10;

#[derive(Debug, Clone)]
pub struct HlsAdapter {
    metadata: FileMeta,
    segment_duration: u64,
}

impl HlsAdapter {
    #[must_use]
    pub fn new(metadata: FileMeta) -> Self {
        Self {
            metadata,
            segment_duration: TARGET_DURATION,
        }
    }

    #[must_use]
    pub fn generate_m3u8(&self) -> Result<String, QvodError> {
        let segments = self.segment_count();
        let mut playlist = String::new();
        playlist.push_str("#EXTM3U\n");
        playlist.push_str("#EXT-X-VERSION:3\n");
        playlist.push_str(&format!(
            "#EXT-X-TARGETDURATION:{}\n",
            self.segment_duration
        ));
        playlist.push_str("#EXT-X-MEDIA-SEQUENCE:0\n\n");

        for i in 0..segments {
            let (offset, length) = self.segment_info(i)?;
            let dur = self.segment_duration;
            playlist.push_str(&format!("#EXTINF:{dur:.1},\n"));
            playlist.push_str(&format!("/segment?offset={offset}&length={length}\n"));
        }

        playlist.push_str("#EXT-X-ENDLIST\n");
        Ok(playlist)
    }

    #[must_use]
    pub fn wrap_as_ts(&self, data: &[u8]) -> Vec<u8> {
        data.to_vec()
    }

    #[must_use]
    pub fn segment_info(&self, index: usize) -> Result<(u64, u64), QvodError> {
        let kfi = self
            .metadata
            .keyframe_index
            .as_ref()
            .ok_or(QvodError::Protocol("no keyframe index".into()))?;
        kfi.segment_at(index)
            .ok_or(QvodError::Protocol("segment index out of range".into()))
    }

    #[must_use]
    pub fn segment_count(&self) -> usize {
        self.metadata
            .keyframe_index
            .as_ref()
            .map_or(1, |kfi| kfi.find_all_i_frames().len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qvs_core::{FrameType, InfoHash, KeyFrameEntry, KeyFrameIndex};

    fn sample_meta() -> FileMeta {
        let kfi = KeyFrameIndex {
            entries: vec![
                KeyFrameEntry {
                    timestamp_ms: 0,
                    file_offset: 0,
                    frame_size: 5000,
                    frame_type: FrameType::I,
                },
                KeyFrameEntry {
                    timestamp_ms: 10000,
                    file_offset: 5000,
                    frame_size: 6000,
                    frame_type: FrameType::I,
                },
            ],
        };
        FileMeta {
            info_hash: InfoHash([0u8; 20]),
            filename: "test.mp4".into(),
            file_size: 100000,
            piece_length: 16384,
            pieces: vec![],
            keyframe_index: Some(kfi),
            duration_ms: 20000,
            video_codec: None,
            audio_codec: None,
            width: 1920,
            height: 1080,
            bitrate: 1000000,
            from_cache: false,
        }
    }

    #[test]
    fn test_m3u8_generation() {
        let adapter = HlsAdapter::new(sample_meta());
        let playlist = adapter.generate_m3u8().unwrap();
        assert!(playlist.starts_with("#EXTM3U"));
        assert!(playlist.contains("#EXT-X-VERSION:3"));
        assert!(playlist.contains("/segment?offset=0"));
        assert!(playlist.contains("/segment?offset=5000"));
    }

    #[test]
    fn test_segment_info() {
        let adapter = HlsAdapter::new(sample_meta());
        let (offset, length) = adapter.segment_info(0).unwrap();
        assert_eq!(offset, 0);
        assert_eq!(length, 5000);
        let (offset, _length) = adapter.segment_info(1).unwrap();
        assert_eq!(offset, 5000);
    }

    #[test]
    fn test_segment_count() {
        let adapter = HlsAdapter::new(sample_meta());
        assert_eq!(adapter.segment_count(), 2);
    }

    #[test]
    fn test_wrap_as_ts() {
        let adapter = HlsAdapter::new(sample_meta());
        let data = vec![0x47u8; 188];
        let wrapped = adapter.wrap_as_ts(&data);
        assert_eq!(wrapped, data);
    }
}
