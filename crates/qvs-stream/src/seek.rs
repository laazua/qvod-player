use qvs_core::{FileMeta, QvodError};

pub struct SeekEngine {
    metadata: FileMeta,
}

impl SeekEngine {
    #[must_use]
    pub fn new(metadata: FileMeta) -> Self {
        tracing::debug!(
            "SeekEngine::new: duration={}ms, has_keyframe_index={}",
            metadata.duration_ms,
            metadata.keyframe_index.is_some()
        );
        Self { metadata }
    }

    #[must_use]
    pub fn find_nearest_keyframe(&self, timestamp_ms: u64) -> Result<u64, QvodError> {
        let kfi = self.metadata.keyframe_index.as_ref().ok_or_else(|| {
            tracing::warn!("SeekEngine::find_nearest_keyframe: no keyframe index");
            QvodError::Protocol("no keyframe index".into())
        })?;
        let entry = kfi.find_nearest_i_frame(timestamp_ms).ok_or_else(|| {
            tracing::warn!(
                "SeekEngine::find_nearest_keyframe: no keyframe at {}ms",
                timestamp_ms
            );
            QvodError::Protocol("no keyframe found".into())
        })?;
        tracing::debug!(
            "SeekEngine::find_nearest_keyframe: {}ms -> offset={}",
            timestamp_ms,
            entry.file_offset
        );
        Ok(entry.file_offset)
    }

    #[must_use]
    pub fn piece_for_offset(&self, offset: u64) -> u32 {
        if self.metadata.piece_length == 0 {
            tracing::warn!("SeekEngine::piece_for_offset: piece_length=0");
            return 0;
        }
        let piece = (offset / self.metadata.piece_length) as u32;
        tracing::debug!(
            "SeekEngine::piece_for_offset: offset={} -> piece={}",
            offset,
            piece
        );
        piece
    }

    #[must_use]
    pub fn metadata(&self) -> &FileMeta {
        &self.metadata
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qvs_core::{FrameType, KeyFrameEntry, KeyFrameIndex};

    #[test]
    fn test_piece_for_offset() {
        let meta = FileMeta {
            info_hash: qvs_core::InfoHash([0u8; 20]),
            filename: "test.mp4".into(),
            file_size: 1_000_000,
            piece_length: 262_144,
            pieces: vec![],
            keyframe_index: None,
            duration_ms: 10_000,
            video_codec: None,
            audio_codec: None,
            width: 0,
            height: 0,
            bitrate: 0,
            from_cache: false,
        };
        let engine = SeekEngine::new(meta);
        assert_eq!(engine.piece_for_offset(0), 0);
        assert_eq!(engine.piece_for_offset(262_144), 1);
        assert_eq!(engine.piece_for_offset(1_000_000), 3);
    }

    #[test]
    fn test_find_nearest_keyframe_no_index() {
        let meta = FileMeta {
            info_hash: qvs_core::InfoHash([0u8; 20]),
            filename: "test.mp4".into(),
            file_size: 1_000_000,
            piece_length: 262_144,
            pieces: vec![],
            keyframe_index: None,
            duration_ms: 10_000,
            video_codec: None,
            audio_codec: None,
            width: 0,
            height: 0,
            bitrate: 0,
            from_cache: false,
        };
        let engine = SeekEngine::new(meta);
        assert!(engine.find_nearest_keyframe(5000).is_err());
    }

    #[test]
    fn test_find_nearest_keyframe_with_index() {
        let kfi = KeyFrameIndex {
            entries: vec![
                KeyFrameEntry {
                    timestamp_ms: 0,
                    file_offset: 0,
                    frame_size: 1000,
                    frame_type: FrameType::I,
                },
                KeyFrameEntry {
                    timestamp_ms: 5000,
                    file_offset: 50000,
                    frame_size: 1000,
                    frame_type: FrameType::I,
                },
            ],
        };
        let meta = FileMeta {
            info_hash: qvs_core::InfoHash([0u8; 20]),
            filename: "test.mp4".into(),
            file_size: 1_000_000,
            piece_length: 262_144,
            pieces: vec![],
            keyframe_index: Some(kfi),
            duration_ms: 10_000,
            video_codec: None,
            audio_codec: None,
            width: 0,
            height: 0,
            bitrate: 0,
            from_cache: false,
        };
        let engine = SeekEngine::new(meta);
        let offset = engine.find_nearest_keyframe(3000).unwrap();
        assert_eq!(offset, 0);
        let offset = engine.find_nearest_keyframe(7000).unwrap();
        assert_eq!(offset, 50000);
    }
}
