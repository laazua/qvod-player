use async_trait::async_trait;

use crate::error::QvodError;
use crate::types::{ConnectionStats, FileMeta, InfoHash, NodeId, PeerInfo};

#[async_trait]
pub trait DhtEngine: Send + Sync {
    async fn bootstrap(&self, seed_nodes: &[String]) -> Result<(), QvodError>;
    async fn find_peers(&self, info_hash: &InfoHash) -> Result<Vec<PeerInfo>, QvodError>;
    async fn announce(&self, info_hash: &InfoHash, port: u16) -> Result<(), QvodError>;
    fn local_id(&self) -> NodeId;
    async fn stats(&self) -> Result<DhtStats, QvodError>;
}

#[derive(Debug, Clone, Default)]
pub struct DhtStats {
    pub total_peers_found: u64,
    pub messages_sent: u64,
    pub messages_received: u64,
    pub routing_table_size: usize,
}

#[async_trait]
pub trait Transport: Send + Sync {
    async fn connect(&self, peer: &PeerInfo) -> Result<(), QvodError>;
    async fn disconnect(&self, peer_id: &[u8; 20]) -> Result<(), QvodError>;
    async fn send_request(
        &self,
        peer_id: &[u8; 20],
        request: &crate::types::BlockRequest,
    ) -> Result<(), QvodError>;
    async fn send_piece(
        &self,
        peer_id: &[u8; 20],
        index: u32,
        begin: u32,
        data: Vec<u8>,
    ) -> Result<(), QvodError>;
    async fn stats(&self) -> Result<ConnectionStats, QvodError>;
}

#[async_trait]
pub trait CacheBackend: Send + Sync {
    async fn find(&self, info_hash: &InfoHash) -> Option<crate::types::FileMeta>;
    async fn read(
        &self,
        info_hash: &InfoHash,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, QvodError>;
    async fn write(
        &self,
        info_hash: &InfoHash,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<(), QvodError>;
    async fn completion(&self, info_hash: &InfoHash) -> f64;
    async fn cleanup(&self) -> Result<(), QvodError>;
}

#[async_trait]
pub trait MetadataResolver: Send + Sync {
    async fn resolve_metadata(&self, info_hash: &InfoHash) -> Result<FileMeta, QvodError>;
}
