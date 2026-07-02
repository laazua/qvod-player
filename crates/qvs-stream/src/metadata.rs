use std::sync::Arc;

use qvs_core::{FileMeta, InfoHash, PeerInfo, QvodError};

use crate::buffer::RingBuffer;
use crate::config::EngineConfig;

pub struct MetadataResolver {
    config: Arc<EngineConfig>,
}

impl MetadataResolver {
    #[must_use]
    pub fn new(config: Arc<EngineConfig>) -> Self {
        Self { config }
    }

    pub async fn resolve_metadata(&self, _info_hash: &InfoHash) -> Result<FileMeta, QvodError> {
        Err(QvodError::MetadataParse)
    }

    pub async fn resolve_from_peers(
        &self,
        _info_hash: &InfoHash,
        _peers: &[PeerInfo],
    ) -> Result<FileMeta, QvodError> {
        Err(QvodError::MetadataParse)
    }

    #[must_use]
    pub fn empty_meta(info_hash: InfoHash, file_size: u64) -> FileMeta {
        FileMeta {
            info_hash,
            filename: "unknown".into(),
            file_size,
            piece_length: qvs_core::PIECE_LENGTH,
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
}

pub struct ProgressTracker {
    buffer: RingBuffer,
    metadata: FileMeta,
    bytes_downloaded: u64,
    total_pieces: u32,
    completed_pieces: u32,
}

impl ProgressTracker {
    #[must_use]
    pub fn new(metadata: FileMeta, buffer: RingBuffer) -> Self {
        let total_pieces = if metadata.piece_length > 0 {
            ((metadata.file_size + metadata.piece_length - 1) / metadata.piece_length) as u32
        } else {
            0
        };
        Self {
            buffer,
            metadata,
            bytes_downloaded: 0,
            total_pieces,
            completed_pieces: 0,
        }
    }

    pub fn record_download(&mut self, bytes: u64) {
        self.bytes_downloaded += bytes;
    }

    #[must_use]
    pub fn completion(&self) -> f64 {
        self.buffer.filled_percentage()
    }

    #[must_use]
    pub fn bytes_downloaded(&self) -> u64 {
        self.bytes_downloaded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_meta() {
        let ih = InfoHash([0u8; 20]);
        let meta = MetadataResolver::empty_meta(ih, 1024);
        assert_eq!(meta.info_hash, ih);
        assert_eq!(meta.file_size, 1024);
    }

    #[test]
    fn test_progress_tracker() {
        let ih = InfoHash([0u8; 20]);
        let meta = MetadataResolver::empty_meta(ih, 1024 * 1024);
        let buffer = RingBuffer::new(65536, 1024 * 1024);
        let mut tracker = ProgressTracker::new(meta, buffer);
        assert_eq!(tracker.completion(), 0.0);
        tracker.record_download(4096);
        assert_eq!(tracker.bytes_downloaded(), 4096);
    }
}
