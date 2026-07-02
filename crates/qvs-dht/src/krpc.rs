use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use qvs_core::{InfoHash, KBucketEntry, NodeId, PeerInfo, QvodError};
use tokio::sync::Mutex;

use crate::routing::{RoutingTable, K};
use crate::rpc::{DhtMessage, FindPeersResult, MessageHeader, NodeInfo};
use crate::token::TokenManager;

const MAX_PEERS_PER_HASH: usize = 50;

#[derive(Clone)]
pub struct KademliaRpc {
    routing_table: Arc<Mutex<RoutingTable>>,
    token_manager: Arc<Mutex<TokenManager>>,
    peers: Arc<Mutex<HashMap<[u8; 20], Vec<PeerInfo>>>>,
}

pub struct IterativeFindPeersResult {
    pub peers: Vec<PeerInfo>,
    pub closest_nodes: Vec<KBucketEntry>,
}

impl KademliaRpc {
    pub fn new(
        routing_table: Arc<Mutex<RoutingTable>>,
        token_manager: Arc<Mutex<TokenManager>>,
    ) -> Self {
        Self {
            routing_table,
            token_manager,
            peers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn routing_table(&self) -> &Arc<Mutex<RoutingTable>> {
        &self.routing_table
    }

    pub fn token_manager(&self) -> &Arc<Mutex<TokenManager>> {
        &self.token_manager
    }

    pub async fn handle_message(
        &self,
        msg: &DhtMessage,
        sender: SocketAddr,
    ) -> Result<Option<DhtMessage>, QvodError> {
        match msg {
            DhtMessage::Ping { header, .. } => {
                let rt = self.routing_table.lock().await;
                Ok(Some(DhtMessage::PingResponse {
                    header: MessageHeader {
                        magic: crate::rpc::MAGIC,
                        msg_type: header.msg_type,
                        txn_id: header.txn_id,
                        ver: crate::rpc::PROTOCOL_VERSION,
                    },
                    node_id: *rt.local_id(),
                }))
            }
            DhtMessage::FindNode { header, target, .. } => {
                let rt = self.routing_table.lock().await;
                let closest = rt.find_closest(target, K);
                let nodes: Vec<NodeInfo> = closest
                    .iter()
                    .map(|e| {
                        let ip = match e.addr.ip() {
                            std::net::IpAddr::V4(v4) => v4.octets(),
                            std::net::IpAddr::V6(_) => [0, 0, 0, 0],
                        };
                        NodeInfo {
                            node_id: e.node_id,
                            ip,
                            port: e.addr.port(),
                        }
                    })
                    .collect();
                Ok(Some(DhtMessage::FindNodeResponse {
                    header: MessageHeader {
                        magic: crate::rpc::MAGIC,
                        msg_type: header.msg_type,
                        txn_id: header.txn_id,
                        ver: crate::rpc::PROTOCOL_VERSION,
                    },
                    node_id: *rt.local_id(),
                    nodes,
                }))
            }
            DhtMessage::FindPeers {
                header, info_hash, ..
            } => {
                let rt = self.routing_table.lock().await;
                let peer_list = {
                    let peers_map = self.peers.lock().await;
                    peers_map.get(info_hash).cloned().unwrap_or_default()
                };

                if !peer_list.is_empty() {
                    let compact: Vec<[u8; 6]> = peer_list
                        .iter()
                        .map(|p| {
                            let ip = match p.addr.ip() {
                                std::net::IpAddr::V4(v4) => v4.octets(),
                                std::net::IpAddr::V6(_) => [0, 0, 0, 0],
                            };
                            let mut buf = [0u8; 6];
                            buf[..4].copy_from_slice(&ip);
                            buf[4..].copy_from_slice(&p.addr.port().to_be_bytes());
                            buf
                        })
                        .collect();

                    Ok(Some(DhtMessage::FindPeersResponse {
                        header: MessageHeader {
                            magic: crate::rpc::MAGIC,
                            msg_type: header.msg_type,
                            txn_id: header.txn_id,
                            ver: crate::rpc::PROTOCOL_VERSION,
                        },
                        node_id: *rt.local_id(),
                        values: FindPeersResult::Peers(compact),
                    }))
                } else {
                    let closest = rt.find_closest(&NodeId(*info_hash), K);
                    let nodes: Vec<NodeInfo> = closest
                        .iter()
                        .map(|e| {
                            let ip = match e.addr.ip() {
                                std::net::IpAddr::V4(v4) => v4.octets(),
                                std::net::IpAddr::V6(_) => [0, 0, 0, 0],
                            };
                            NodeInfo {
                                node_id: e.node_id,
                                ip,
                                port: e.addr.port(),
                            }
                        })
                        .collect();
                    Ok(Some(DhtMessage::FindPeersResponse {
                        header: MessageHeader {
                            magic: crate::rpc::MAGIC,
                            msg_type: header.msg_type,
                            txn_id: header.txn_id,
                            ver: crate::rpc::PROTOCOL_VERSION,
                        },
                        node_id: *rt.local_id(),
                        values: FindPeersResult::Nodes(nodes),
                    }))
                }
            }
            DhtMessage::Announce {
                header,
                info_hash,
                token,
                port,
                ..
            } => {
                let tm = self.token_manager.lock().await;
                if !tm.verify_token(&sender, token) {
                    return Err(QvodError::Protocol("invalid announce token".into()));
                }
                drop(tm);

                let peer = PeerInfo {
                    peer_id: [0u8; 20],
                    addr: SocketAddr::new(sender.ip(), *port),
                    is_firewalled: false,
                    bw_up: 0,
                    bw_down: 0,
                    location: None,
                    latency: Duration::default(),
                };

                let mut peers_map = self.peers.lock().await;
                let entry = peers_map.entry(*info_hash).or_default();
                if entry.len() < MAX_PEERS_PER_HASH {
                    entry.push(peer);
                }
                drop(peers_map);

                let rt = self.routing_table.lock().await;
                Ok(Some(DhtMessage::AnnounceResponse {
                    header: MessageHeader {
                        magic: crate::rpc::MAGIC,
                        msg_type: header.msg_type,
                        txn_id: header.txn_id,
                        ver: crate::rpc::PROTOCOL_VERSION,
                    },
                    node_id: *rt.local_id(),
                }))
            }
            _ => Err(QvodError::Protocol("unhandled message".into())),
        }
    }

    pub async fn get_peers(&self, info_hash: &InfoHash) -> Vec<PeerInfo> {
        let peers_map = self.peers.lock().await;
        peers_map.get(&info_hash.0).cloned().unwrap_or_default()
    }

    pub async fn iterative_find_peers(
        &self,
        info_hash: &InfoHash,
        alpha: usize,
        socket: &tokio::net::UdpSocket,
        local_id: &NodeId,
    ) -> Result<IterativeFindPeersResult, QvodError> {
        const MAX_ROUNDS: u32 = 8;
        let target = NodeId(info_hash.0);
        let mut queried: std::collections::HashSet<SocketAddr> = std::collections::HashSet::new();

        let mut shortlist = {
            let rt = self.routing_table.lock().await;
            rt.find_closest(&target, K)
        };

        if shortlist.is_empty() {
            return Err(QvodError::DhtRoutingFailed(
                "no nodes in routing table for iterative find_peers".into(),
            ));
        }

        let mut closest_peer: Option<[u8; 20]> = None;

        for _round in 0..MAX_ROUNDS {
            shortlist.sort_by(|a, b| {
                target
                    .xor_distance(&a.node_id)
                    .cmp(&target.xor_distance(&b.node_id))
            });

            let to_query: Vec<_> = shortlist
                .iter()
                .filter(|e| !queried.contains(&e.addr))
                .take(alpha)
                .cloned()
                .collect();

            if to_query.is_empty() {
                break;
            }

            let mut found_any = false;

            for entry in &to_query {
                queried.insert(entry.addr);

                let msg = DhtMessage::FindPeers {
                    header: MessageHeader {
                        magic: crate::rpc::MAGIC,
                        msg_type: crate::rpc::MessageType::FindPeers,
                        txn_id: 0,
                        ver: crate::rpc::PROTOCOL_VERSION,
                    },
                    node_id: *local_id,
                    info_hash: info_hash.0,
                };

                let encoded = msg.encode();
                if socket.send_to(&encoded, entry.addr).await.is_err() {
                    continue;
                }

                let mut buf = [0u8; 1400];
                match tokio::time::timeout(Duration::from_secs(5), socket.recv_from(&mut buf)).await
                {
                    Ok(Ok((len, _))) => {
                        if let Ok(response) = DhtMessage::decode(&buf[..len]) {
                            if let DhtMessage::FindPeersResponse { values, .. } = &response {
                                match values {
                                    FindPeersResult::Peers(compact_peers) => {
                                        let mut decoded_peers = Vec::new();
                                        for cp in compact_peers {
                                            let ip =
                                                std::net::Ipv4Addr::new(cp[0], cp[1], cp[2], cp[3]);
                                            let port = u16::from_be_bytes([cp[4], cp[5]]);
                                            decoded_peers.push(PeerInfo {
                                                peer_id: [0u8; 20],
                                                addr: SocketAddr::new(
                                                    std::net::IpAddr::V4(ip),
                                                    port,
                                                ),
                                                is_firewalled: false,
                                                bw_up: 0,
                                                bw_down: 0,
                                                location: None,
                                                latency: Duration::default(),
                                            });
                                        }
                                        if !decoded_peers.is_empty() {
                                            let found_peers = {
                                                let mut p = self.peers.lock().await;
                                                let e = p.entry(info_hash.0).or_default();
                                                for peer in &decoded_peers {
                                                    if e.len() < MAX_PEERS_PER_HASH {
                                                        e.push(peer.clone());
                                                    }
                                                }
                                                decoded_peers
                                            };
                                            return Ok(IterativeFindPeersResult {
                                                peers: found_peers,
                                                closest_nodes: shortlist.clone(),
                                            });
                                        }
                                    }
                                    FindPeersResult::Nodes(node_infos) => {
                                        for node_info in node_infos {
                                            let node_entry = KBucketEntry {
                                                node_id: node_info.node_id,
                                                addr: SocketAddr::new(
                                                    std::net::IpAddr::V4(std::net::Ipv4Addr::from(
                                                        node_info.ip,
                                                    )),
                                                    node_info.port,
                                                ),
                                                last_seen: std::time::Instant::now(),
                                                latency: Duration::default(),
                                                is_firewalled: false,
                                            };
                                            if !shortlist.iter().any(|e| e.addr == node_entry.addr)
                                                && !queried.contains(&node_entry.addr)
                                            {
                                                shortlist.push(node_entry);
                                                found_any = true;
                                            }
                                        }
                                    }
                                }
                            } else if let DhtMessage::FindNodeResponse { nodes, .. } = &response {
                                for node_info in nodes {
                                    let node_entry = KBucketEntry {
                                        node_id: node_info.node_id,
                                        addr: SocketAddr::new(
                                            std::net::IpAddr::V4(std::net::Ipv4Addr::from(
                                                node_info.ip,
                                            )),
                                            node_info.port,
                                        ),
                                        last_seen: std::time::Instant::now(),
                                        latency: Duration::default(),
                                        is_firewalled: false,
                                    };
                                    if !shortlist.iter().any(|e| e.addr == node_entry.addr)
                                        && !queried.contains(&node_entry.addr)
                                    {
                                        shortlist.push(node_entry);
                                        found_any = true;
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }

            // Track closest peer distance for termination
            if !shortlist.is_empty() {
                shortlist.sort_by(|a, b| {
                    target
                        .xor_distance(&a.node_id)
                        .cmp(&target.xor_distance(&b.node_id))
                });
                let current_closest = shortlist[0].node_id.0;
                match closest_peer {
                    None => closest_peer = Some(current_closest),
                    Some(prev) => {
                        if current_closest == prev {
                            break;
                        }
                        closest_peer = Some(current_closest);
                    }
                }
            }

            if !found_any {
                break;
            }
        }

        // Insert discovered nodes into routing table
        {
            let mut rt = self.routing_table.lock().await;
            for entry in &shortlist {
                rt.insert(entry.clone());
            }
        }

        Ok(IterativeFindPeersResult {
            peers: Vec::new(),
            closest_nodes: shortlist,
        })
    }

    pub async fn refresh_buckets(&self) {
        let rt = self.routing_table.lock().await;
        let list = rt.refresh_list();
        drop(rt);
        if !list.is_empty() {
            let mut rt = self.routing_table.lock().await;
            for idx in &list {
                rt.mark_refreshed(*idx);
            }
        }
    }

    pub async fn routing_table_size(&self) -> usize {
        let rt = self.routing_table.lock().await;
        rt.size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::RoutingTable;
    use crate::rpc::MessageType;
    use crate::token::TokenManager;
    use qvs_core::generate_node_id;
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::Arc;

    fn test_addr() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), 8621)
    }

    #[tokio::test]
    async fn test_handle_ping() {
        let local_id = NodeId(generate_node_id());
        let routing_table = Arc::new(Mutex::new(RoutingTable::new(local_id)));
        let token_manager = Arc::new(Mutex::new(TokenManager::new()));
        let rpc = KademliaRpc::new(routing_table, token_manager);

        let msg = DhtMessage::Ping {
            header: MessageHeader {
                magic: crate::rpc::MAGIC,
                msg_type: MessageType::Ping,
                txn_id: 1,
                ver: crate::rpc::PROTOCOL_VERSION,
            },
            node_id: NodeId([1u8; 20]),
        };

        let response = rpc.handle_message(&msg, test_addr()).await.unwrap();
        assert!(response.is_some());
        match response.unwrap() {
            DhtMessage::PingResponse { .. } => {}
            _ => panic!("expected ping response"),
        }
    }

    #[tokio::test]
    async fn test_handle_announce_valid_token() {
        let local_id = NodeId(generate_node_id());
        let routing_table = Arc::new(Mutex::new(RoutingTable::new(local_id)));
        let token_manager = Arc::new(Mutex::new(TokenManager::new()));
        let addr = test_addr();
        let token = {
            let tm = token_manager.lock().await;
            tm.generate_token(&addr)
        };
        let rpc = KademliaRpc::new(routing_table, token_manager);

        let msg = DhtMessage::Announce {
            header: MessageHeader {
                magic: crate::rpc::MAGIC,
                msg_type: MessageType::Announce,
                txn_id: 1,
                ver: crate::rpc::PROTOCOL_VERSION,
            },
            node_id: NodeId([1u8; 20]),
            info_hash: [2u8; 20],
            token,
            port: 8621,
        };

        let response = rpc.handle_message(&msg, addr).await.unwrap();
        assert!(response.is_some());
    }
}
