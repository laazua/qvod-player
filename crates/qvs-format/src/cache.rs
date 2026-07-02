use std::path::PathBuf;
use std::sync::Arc;

use qvs_core::{Bitfield, FileMeta, InfoHash, QvodError};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub cache_dir: PathBuf,
    pub max_size: u64,
    pub max_files: u32,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            cache_dir: dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("qvs")
                .join("cache"),
            max_size: 10 * 1024 * 1024 * 1024,
            max_files: 1000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub info_hash: InfoHash,
    pub file_size: u64,
    pub downloaded: u64,
    pub bitfield: Bitfield,
    pub last_access: std::time::SystemTime,
    pub created_at: std::time::SystemTime,
}

pub struct CacheManager {
    config: CacheConfig,
    inner: Arc<Mutex<CacheInner>>,
}

struct CacheInner {
    entries: Vec<CacheEntry>,
}

impl CacheManager {
    pub async fn new(config: CacheConfig) -> Self {
        let _ = tokio::fs::create_dir_all(config.cache_dir.join("qdata")).await;
        let _ = tokio::fs::create_dir_all(config.cache_dir.join("qmv")).await;
        let inner = CacheInner {
            entries: Vec::new(),
        };
        Self {
            config,
            inner: Arc::new(Mutex::new(inner)),
        }
    }

    fn qdata_path(&self, info_hash: &InfoHash) -> PathBuf {
        let hash_hex = info_hash.to_string();
        self.config
            .cache_dir
            .join("qdata")
            .join(format!("{hash_hex}.qdata"))
    }

    fn qmv_path(&self, info_hash: &InfoHash) -> PathBuf {
        let hash_hex = info_hash.to_string();
        self.config
            .cache_dir
            .join("qmv")
            .join(format!("{hash_hex}.qmv"))
    }

    #[allow(clippy::cast_possible_truncation)]
    pub async fn find_entry(&self, info_hash: &InfoHash) -> Option<CacheEntry> {
        let path = self.qmv_path(info_hash);
        if !path.exists() {
            return None;
        }
        let data = tokio::fs::read(&path).await.ok()?;
        let qvs_file = crate::qvs_file::QvsFile::decode(&data).ok()?;

        let num_pieces = if qvs_file.piece_length > 0 {
            qvs_file.file_size.div_ceil(qvs_file.piece_length) as u32
        } else {
            0
        };

        Some(CacheEntry {
            info_hash: qvs_file.info_hash,
            file_size: qvs_file.file_size,
            downloaded: 0,
            bitfield: Bitfield::new(num_pieces),
            last_access: std::time::SystemTime::now(),
            created_at: std::time::SystemTime::now(),
        })
    }

    #[allow(clippy::cast_possible_truncation)]
    pub async fn find(&self, info_hash: &InfoHash) -> Option<FileMeta> {
        let path = self.qmv_path(info_hash);
        if !path.exists() {
            return None;
        }
        let data = tokio::fs::read(&path).await.ok()?;
        let qvs_file = crate::qvs_file::QvsFile::decode(&data).ok()?;
        Some(FileMeta {
            info_hash: qvs_file.info_hash,
            filename: qvs_file.filename,
            file_size: qvs_file.file_size,
            piece_length: qvs_file.piece_length,
            pieces: qvs_file.pieces,
            keyframe_index: None,
            duration_ms: 0,
            video_codec: None,
            audio_codec: None,
            width: 0,
            height: 0,
            bitrate: 0,
        })
    }

    pub async fn read_entry(&self, info_hash: &InfoHash) -> Option<CacheEntry> {
        self.find_entry(info_hash).await
    }

    #[allow(clippy::missing_errors_doc, clippy::cast_possible_truncation)]
    pub async fn read(
        &self,
        info_hash: &InfoHash,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, QvodError> {
        let path = self.qdata_path(info_hash);
        let mut file = tokio::fs::OpenOptions::new().read(true).open(&path).await?;
        file.seek(tokio::io::SeekFrom::Start(offset)).await?;
        let mut buf = vec![0u8; length as usize];
        let n = file.read(&mut buf).await?;
        buf.truncate(n);
        Ok(buf)
    }

    #[allow(clippy::missing_errors_doc)]
    pub async fn write(
        &self,
        info_hash: &InfoHash,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<(), QvodError> {
        let path = self.qdata_path(info_hash);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .await?;
        file.seek(tokio::io::SeekFrom::Start(offset)).await?;
        file.write_all(&data).await?;

        let written_end = offset + data.len() as u64;
        let mut inner = self.inner.lock().await;
        if let Some(existing) = inner.entries.iter_mut().find(|e| e.info_hash == *info_hash) {
            existing.downloaded = existing.downloaded.max(written_end);
            existing.last_access = std::time::SystemTime::now();
        } else {
            inner.entries.push(CacheEntry {
                info_hash: *info_hash,
                file_size: written_end,
                downloaded: written_end,
                bitfield: Bitfield::new(0),
                last_access: std::time::SystemTime::now(),
                created_at: std::time::SystemTime::now(),
            });
        }

        Ok(())
    }

    #[allow(clippy::manual_let_else, clippy::cast_precision_loss)]
    pub async fn completion(&self, info_hash: &InfoHash) -> f64 {
        let path = self.qdata_path(info_hash);
        let metadata = match tokio::fs::metadata(&path).await {
            Ok(m) => m,
            Err(_) => return 0.0,
        };
        let file_size = metadata.len();
        let qmv_path = self.qmv_path(info_hash);
        let qmv_data = match tokio::fs::read(&qmv_path).await {
            Ok(d) => d,
            Err(_) => return 0.0,
        };
        let qvs_file = match crate::qvs_file::QvsFile::decode(&qmv_data) {
            Ok(f) => f,
            Err(_) => return 0.0,
        };
        if qvs_file.file_size == 0 {
            return 1.0;
        }
        (file_size as f64) / (qvs_file.file_size as f64)
    }

    #[allow(clippy::missing_errors_doc)]
    pub async fn cleanup(&self) -> Result<(), QvodError> {
        let mut inner = self.inner.lock().await;
        inner
            .entries
            .sort_by(|a, b| a.last_access.cmp(&b.last_access));
        let mut total_size: u64 = inner.entries.iter().map(|e| e.file_size).sum();
        let max_size = self.config.max_size;
        let threshold = max_size * 80 / 100;

        let mut to_delete: Vec<InfoHash> = Vec::new();
        inner.entries.retain(|entry| {
            if total_size <= threshold {
                return true;
            }
            total_size = total_size.saturating_sub(entry.file_size);
            to_delete.push(entry.info_hash);
            false
        });

        drop(inner);
        for hash in &to_delete {
            let _ = tokio::fs::remove_file(self.qdata_path(hash)).await;
            let _ = tokio::fs::remove_file(self.qmv_path(hash)).await;
        }
        Ok(())
    }

    #[allow(clippy::missing_errors_doc)]
    pub async fn delete(&self, info_hash: &InfoHash) -> Result<(), QvodError> {
        let qdata = self.qdata_path(info_hash);
        let qmv = self.qmv_path(info_hash);
        let _ = tokio::fs::remove_file(&qdata).await;
        let _ = tokio::fs::remove_file(&qmv).await;
        Ok(())
    }

    pub async fn list(&self) -> Vec<CacheEntry> {
        let inner = self.inner.lock().await;
        inner.entries.clone()
    }
}

#[async_trait::async_trait]
impl qvs_core::traits::CacheBackend for CacheManager {
    async fn find(&self, info_hash: &InfoHash) -> Option<FileMeta> {
        CacheManager::find(self, info_hash).await
    }

    async fn read(
        &self,
        info_hash: &InfoHash,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, QvodError> {
        CacheManager::read(self, info_hash, offset, length).await
    }

    async fn write(
        &self,
        info_hash: &InfoHash,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<(), QvodError> {
        CacheManager::write(self, info_hash, offset, data).await
    }

    async fn completion(&self, info_hash: &InfoHash) -> f64 {
        CacheManager::completion(self, info_hash).await
    }

    async fn cleanup(&self) -> Result<(), QvodError> {
        CacheManager::cleanup(self).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_config_default() {
        let config = CacheConfig::default();
        assert!(config.cache_dir.to_str().unwrap_or("").contains("qvs"));
        assert_eq!(config.max_size, 10 * 1024 * 1024 * 1024);
    }

    #[tokio::test]
    async fn test_write_then_read() {
        let dir = tempfile::tempdir().unwrap();
        let config = CacheConfig {
            cache_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let manager = CacheManager::new(config).await;
        let hash = InfoHash([0xAB; 20]);
        let data = b"hello cache".to_vec();
        manager.write(&hash, 0, data.clone()).await.unwrap();
        let read_back = manager.read(&hash, 0, data.len() as u64).await.unwrap();
        assert_eq!(read_back, data);
    }

    #[tokio::test]
    async fn test_completion_zero_for_missing() {
        let config = CacheConfig::default();
        let manager = CacheManager::new(config).await;
        let hash = InfoHash([0xFF; 20]);
        assert_eq!(manager.completion(&hash).await, 0.0);
    }

    #[tokio::test]
    async fn test_find_entry_returns_none_for_missing() {
        let config = CacheConfig::default();
        let manager = CacheManager::new(config).await;
        let hash = InfoHash([0xCC; 20]);
        assert!(manager.find_entry(&hash).await.is_none());
    }

    #[tokio::test]
    async fn test_write_at_offset_does_not_truncate() {
        let dir = tempfile::tempdir().unwrap();
        let config = CacheConfig {
            cache_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let manager = CacheManager::new(config).await;
        let hash = InfoHash([0xDD; 20]);

        manager.write(&hash, 6, b"world".to_vec()).await.unwrap();
        manager.write(&hash, 0, b"hello ".to_vec()).await.unwrap();

        let read_back = manager.read(&hash, 0, 11).await.unwrap();
        assert_eq!(read_back, b"hello world");
    }

    #[tokio::test]
    async fn test_cleanup_deletes_files() {
        let dir = tempfile::tempdir().unwrap();
        let config = CacheConfig {
            cache_dir: dir.path().to_path_buf(),
            max_size: 100,
            ..Default::default()
        };
        let manager = CacheManager::new(config).await;
        let hash = InfoHash([0xEE; 20]);

        let data = vec![0u8; 200];
        manager.write(&hash, 0, data).await.unwrap();

        let qdata_path = manager.qdata_path(&hash);
        assert!(qdata_path.exists());

        manager.cleanup().await.unwrap();

        assert!(!qdata_path.exists());
    }
}
