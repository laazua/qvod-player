use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use qvs_core::{DhtEngine, DhtStats, InfoHash, NodeId, PeerInfo, QvodError};
use tokio::sync::Mutex;

use crate::bootstrap;
use crate::krpc::KademliaRpc;
use crate::routing::RoutingTable;
use crate::token::TokenManager;

#[derive(Debug, Clone)]
pub struct DhtConfig {
    pub listen_port: u16,
    pub k: u8,
    pub alpha: u8,
    pub refresh_interval: u64,
    pub peer_timeout: u64,
    pub seed_nodes: Vec<String>,
}

impl Default for DhtConfig {
    fn default() -> Self {
        Self {
            listen_port: qvs_core::DEFAULT_PORT,
            k: qvs_core::DHT_K,
            alpha: qvs_core::DHT_ALPHA,
            refresh_interval: qvs_core::DHT_REFRESH_INTERVAL,
            peer_timeout: qvs_core::DHT_PEER_TIMEOUT,
            seed_nodes: Vec::new(),
        }
    }
}

pub struct DhtNode {
    config: DhtConfig,
    local_id: NodeId,
    inner: Arc<Mutex<DhtInner>>,
    krpc: KademliaRpc,
    socket: Arc<tokio::net::UdpSocket>,
    stopped: Arc<AtomicBool>,
}

struct DhtInner {
    krpc: KademliaRpc,
    stats: DhtStats,
}

impl DhtNode {
    pub async fn new(config: DhtConfig) -> Result<Self, QvodError> {
        let local_id = NodeId(qvs_core::generate_node_id());

        let socket = tokio::net::UdpSocket::bind(format!("0.0.0.0:{}", config.listen_port))
            .await
            .map_err(QvodError::Network)?;
        let socket = Arc::new(socket);

        let routing_table = Arc::new(Mutex::new(RoutingTable::new(local_id)));
        let token_manager = Arc::new(Mutex::new(TokenManager::new()));
        let krpc = KademliaRpc::new(routing_table, token_manager);

        let inner = DhtInner {
            krpc: krpc.clone(),
            stats: DhtStats::default(),
        };

        Ok(Self {
            config,
            local_id,
            inner: Arc::new(Mutex::new(inner)),
            krpc,
            socket,
            stopped: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    pub fn set_alpha(&mut self, alpha: u8) {
        self.config.alpha = alpha;
    }

    pub fn set_k(&mut self, k: u8) {
        self.config.k = k;
    }

    pub fn local_id(&self) -> NodeId {
        self.local_id
    }

    pub fn config(&self) -> &DhtConfig {
        &self.config
    }

    pub fn socket(&self) -> &tokio::net::UdpSocket {
        &self.socket
    }

    pub fn krpc(&self) -> &KademliaRpc {
        &self.krpc
    }

    pub async fn start(&self) -> tokio::task::JoinHandle<()> {
        let inner = self.inner.clone();
        let socket = self.socket.clone();
        let stopped = self.stopped.clone();
        let refresh_interval = self.config.refresh_interval;

        tokio::spawn(async move {
            let mut buf = [0u8; 1400];
            let mut refresh_timer = tokio::time::interval(Duration::from_secs(refresh_interval));
            refresh_timer.tick().await;

            loop {
                if stopped.load(Ordering::SeqCst) {
                    break;
                }

                tokio::select! {
                    recv_result = socket.recv_from(&mut buf) => {
                        match recv_result {
                            Ok((len, sender)) => {
                                let mut guard = inner.lock().await;
                                guard.stats.messages_received += 1;
                                if let Ok(msg) = crate::rpc::DhtMessage::decode(&buf[..len]) {
                                    if let Ok(Some(response)) = guard.krpc.handle_message(&msg, sender).await {
                                        let encoded = response.encode();
                                        guard.stats.messages_sent += 1;
                                        let _ = socket.send_to(&encoded, sender).await;
                                    }
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    _ = refresh_timer.tick() => {
                        let guard = inner.lock().await;
                        guard.krpc.refresh_buckets().await;
                    }
                }
            }
        })
    }
}

#[async_trait]
impl DhtEngine for DhtNode {
    async fn bootstrap(&self, seed_nodes: &[String]) -> Result<(), QvodError> {
        let rt = self.krpc.routing_table().clone();
        let mut rt_guard = rt.lock().await;
        bootstrap::bootstrap(&mut rt_guard, seed_nodes).await
    }

    async fn find_peers(&self, info_hash: &InfoHash) -> Result<Vec<PeerInfo>, QvodError> {
        let cached = self.krpc.get_peers(info_hash).await;
        if !cached.is_empty() {
            return Ok(cached);
        }

        match self
            .krpc
            .iterative_find_peers(
                info_hash,
                self.config.alpha as usize,
                &self.socket,
                &self.local_id,
            )
            .await
        {
            Ok(result) => {
                if !result.peers.is_empty() {
                    Ok(result.peers)
                } else {
                    Err(QvodError::NoPeers)
                }
            }
            Err(_) => Err(QvodError::NoPeers),
        }
    }

    async fn announce(&self, info_hash: &InfoHash, port: u16) -> Result<(), QvodError> {
        let socket = self.socket.clone();
        let local_id = self.local_id;

        let closest = {
            let rt = self.krpc.routing_table().lock().await;
            rt.find_closest(&NodeId(info_hash.0), 8)
        };

        for entry in &closest {
            let token = {
                let tm = self.krpc.token_manager().lock().await;
                tm.generate_token(&entry.addr)
            };
            let msg = crate::rpc::DhtMessage::Announce {
                header: crate::rpc::MessageHeader {
                    magic: crate::rpc::MAGIC,
                    msg_type: crate::rpc::MessageType::Announce,
                    txn_id: 0,
                    ver: crate::rpc::PROTOCOL_VERSION,
                },
                node_id: local_id,
                info_hash: info_hash.0,
                token,
                port,
            };
            let encoded = msg.encode();
            let _ = socket.send_to(&encoded, entry.addr).await;
        }

        Ok(())
    }

    fn local_id(&self) -> NodeId {
        self.local_id
    }

    async fn stats(&self) -> Result<DhtStats, QvodError> {
        let guard = self.inner.lock().await;
        let mut stats = guard.stats.clone();
        stats.routing_table_size = guard.krpc.routing_table_size().await;
        Ok(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_dht_node_creation() {
        let config = DhtConfig {
            listen_port: 0,
            ..Default::default()
        };
        let node = DhtNode::new(config).await;
        assert!(node.is_ok());
        let node = node.unwrap();
        assert_eq!(node.local_id().0.len(), 20);
    }

    #[tokio::test]
    async fn test_dht_node_stop() {
        let config = DhtConfig {
            listen_port: 0,
            ..Default::default()
        };
        let node = DhtNode::new(config).await.unwrap();
        assert!(!node.is_stopped());
        node.stop();
        assert!(node.is_stopped());
    }

    #[tokio::test]
    async fn test_dht_node_start_stop() {
        let config = DhtConfig {
            listen_port: 0,
            ..Default::default()
        };
        let node = DhtNode::new(config).await.unwrap();
        let handle = node.start().await;
        assert!(!handle.is_finished());
        node.stop();
        // Give it a moment to stop
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    #[tokio::test]
    async fn test_dht_node_setters() {
        let config = DhtConfig {
            listen_port: 0,
            ..Default::default()
        };
        let mut node = DhtNode::new(config).await.unwrap();
        node.set_alpha(5);
        node.set_k(16);
        assert_eq!(node.config().alpha, 5);
        assert_eq!(node.config().k, 16);
    }
}
