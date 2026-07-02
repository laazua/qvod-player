use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

use qvs_core::{PeerInfo, QvodError, MAX_PEER_CONNECTIONS};

use crate::congestion::UdpCongestionControl;
use crate::stats::PoolStats;

#[derive(Debug, Clone)]
pub struct PeerConnection {
    pub peer_id: [u8; 20],
    pub addr: std::net::SocketAddr,
    pub am_choking: bool,
    pub am_interested: bool,
    pub peer_choking: bool,
    pub peer_interested: bool,
    pub bitfield: Vec<u8>,
    pub speed_down: f64,
    pub speed_up: f64,
    pub rtt: Duration,
    pub connected_at: Instant,
    pub last_active: Instant,
    pub bytes_downloaded: u64,
    pub bytes_uploaded: u64,
}

impl PeerConnection {
    #[must_use]
    pub fn new(peer_id: [u8; 20], addr: std::net::SocketAddr) -> Self {
        Self {
            peer_id,
            addr,
            am_choking: true,
            am_interested: false,
            peer_choking: true,
            peer_interested: false,
            bitfield: Vec::new(),
            speed_down: 0.0,
            speed_up: 0.0,
            rtt: Duration::from_millis(100),
            connected_at: Instant::now(),
            last_active: Instant::now(),
            bytes_downloaded: 0,
            bytes_uploaded: 0,
        }
    }

    pub fn update_activity(&mut self) {
        self.last_active = Instant::now();
    }

    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.last_active.elapsed() > Duration::from_secs(300)
    }

    #[must_use]
    pub fn is_interesting(&self) -> bool {
        !self.peer_choking
    }
}

#[derive(Debug)]
pub struct ConnectionPool {
    max_connections: u32,
    connections: HashMap<[u8; 20], PeerConnection>,
    congestion: HashMap<[u8; 20], UdpCongestionControl>,
}

impl ConnectionPool {
    #[must_use]
    pub fn new(max_connections: u32) -> Self {
        Self {
            max_connections: max_connections.min(MAX_PEER_CONNECTIONS),
            connections: HashMap::new(),
            congestion: HashMap::new(),
        }
    }

    pub fn add_peer(&mut self, peer: &PeerInfo) -> Result<(), QvodError> {
        if self.connections.len() >= self.max_connections as usize {
            return Err(QvodError::ConnectionLimitReached);
        }
        let peer_id = peer.peer_id;
        if self.connections.contains_key(&peer_id) {
            return Ok(());
        }
        self.connections
            .insert(peer_id, PeerConnection::new(peer_id, peer.addr));
        self.congestion.insert(peer_id, UdpCongestionControl::new());
        Ok(())
    }

    pub fn remove_peer(&mut self, peer_id: &[u8; 20]) {
        self.connections.remove(peer_id);
        self.congestion.remove(peer_id);
    }

    #[must_use]
    pub fn get_peer(&self, peer_id: &[u8; 20]) -> Option<&PeerConnection> {
        self.connections.get(peer_id)
    }

    pub fn get_peer_mut(&mut self, peer_id: &[u8; 20]) -> Option<&mut PeerConnection> {
        self.connections.get_mut(peer_id)
    }

    #[must_use]
    pub fn get_congestion(&self, peer_id: &[u8; 20]) -> Option<&UdpCongestionControl> {
        self.congestion.get(peer_id)
    }

    pub fn get_congestion_mut(&mut self, peer_id: &[u8; 20]) -> Option<&mut UdpCongestionControl> {
        self.congestion.get_mut(peer_id)
    }

    #[must_use]
    pub fn select_upload_peers(&self, count: usize) -> Vec<&PeerConnection> {
        self.connections
            .values()
            .filter(|c| c.peer_interested && !c.am_choking)
            .take(count)
            .collect()
    }

    #[must_use]
    pub fn select_download_peers(&self, count: usize) -> Vec<&PeerConnection> {
        let mut candidates: Vec<&PeerConnection> = self
            .connections
            .values()
            .filter(|c| !c.peer_choking)
            .collect();
        candidates.sort_by(|a, b| {
            b.speed_down
                .partial_cmp(&a.speed_down)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidates.truncate(count);
        candidates
    }

    pub fn cleanup_idle(&mut self) {
        let idle_threshold = Duration::from_secs(300);
        let peer_ids: Vec<[u8; 20]> = self
            .connections
            .iter()
            .filter(|(_, c)| c.last_active.elapsed() > idle_threshold)
            .map(|(id, _)| *id)
            .collect();
        for id in peer_ids {
            self.remove_peer(&id);
        }
    }

    pub fn maintain_connections(&mut self) {
        for conn in self.connections.values_mut() {
            conn.update_activity();
        }
    }

    #[must_use]
    pub fn stats(&self) -> PoolStats {
        let total = self.connections.len();
        let connected = self.connections.values().filter(|c| !c.is_idle()).count();
        let upload = self.connections.values().filter(|c| !c.am_choking).count();
        let download = self
            .connections
            .values()
            .filter(|c| !c.peer_choking)
            .count();
        let idle = self.connections.values().filter(|c| c.is_idle()).count();
        PoolStats {
            total_peers: total,
            connected_peers: connected,
            upload_peers: upload,
            download_peers: download,
            idle_peers: idle,
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.connections.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }

    #[must_use]
    pub fn all_connections(&self) -> &HashMap<[u8; 20], PeerConnection> {
        &self.connections
    }

    pub fn all_connections_mut(&mut self) -> &mut HashMap<[u8; 20], PeerConnection> {
        &mut self.connections
    }
}

impl Default for ConnectionPool {
    fn default() -> Self {
        Self::new(MAX_PEER_CONNECTIONS)
    }
}

pub type SharedConnectionPool = Arc<Mutex<ConnectionPool>>;

#[must_use]
pub fn new_shared_pool() -> SharedConnectionPool {
    Arc::new(Mutex::new(ConnectionPool::default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn sample_peer(id: u8) -> PeerInfo {
        PeerInfo {
            peer_id: [id; 20],
            addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8621 + id as u16),
            is_firewalled: false,
            bw_up: 0,
            bw_down: 0,
            location: None,
            latency: Duration::default(),
        }
    }

    #[test]
    fn test_add_and_remove_peer() {
        let mut pool = ConnectionPool::new(10);
        let peer = sample_peer(1);
        assert!(pool.add_peer(&peer).is_ok());
        assert_eq!(pool.len(), 1);
        pool.remove_peer(&peer.peer_id);
        assert!(pool.is_empty());
    }

    #[test]
    fn test_pool_limit() {
        let mut pool = ConnectionPool::new(2);
        for i in 0..3 {
            let peer = sample_peer(i);
            if i < 2 {
                assert!(pool.add_peer(&peer).is_ok());
            } else {
                assert!(pool.add_peer(&peer).is_err());
            }
        }
    }

    #[test]
    fn test_get_peer() {
        let mut pool = ConnectionPool::new(10);
        let peer = sample_peer(1);
        pool.add_peer(&peer).unwrap();
        let found = pool.get_peer(&peer.peer_id);
        assert!(found.is_some());
        assert_eq!(found.unwrap().peer_id, peer.peer_id);
    }

    #[test]
    fn test_cleanup_idle() {
        let mut pool = ConnectionPool::new(10);
        pool.add_peer(&sample_peer(1)).unwrap();
        // Manually set last_active far in the past
        if let Some(conn) = pool.get_peer_mut(&sample_peer(1).peer_id) {
            conn.last_active = Instant::now() - Duration::from_secs(301);
        }
        pool.cleanup_idle();
        assert!(pool.is_empty());
    }

    #[test]
    fn test_default_pool() {
        let pool = ConnectionPool::default();
        assert_eq!(pool.max_connections, MAX_PEER_CONNECTIONS);
    }

    #[test]
    fn test_duplicate_add() {
        let mut pool = ConnectionPool::new(10);
        let peer = sample_peer(1);
        assert!(pool.add_peer(&peer).is_ok());
        assert!(pool.add_peer(&peer).is_ok());
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn test_connection_stats() {
        let mut pool = ConnectionPool::new(10);
        for i in 0..3 {
            pool.add_peer(&sample_peer(i)).unwrap();
        }
        let s = pool.stats();
        assert_eq!(s.total_peers, 3);
    }
}
