use std::net::{SocketAddr, ToSocketAddrs};
use std::time::Duration;

use qvs_core::{NodeId, QvodError};

use crate::routing::RoutingTable;
use crate::rpc::{DhtMessage, MessageHeader, MessageType};

pub async fn bootstrap(
    routing_table: &mut RoutingTable,
    seed_nodes: &[String],
) -> Result<(), QvodError> {
    if seed_nodes.is_empty() {
        return Err(QvodError::DhtRoutingFailed("no seed nodes".into()));
    }

    let socket = tokio::net::UdpSocket::bind("0.0.0.0:0")
        .await
        .map_err(QvodError::Network)?;

    let local_id = *routing_table.local_id();

    for seed_str in seed_nodes {
        let addr = match seed_str.to_socket_addrs() {
            Ok(mut iter) => match iter.next() {
                Some(a) => a,
                None => continue,
            },
            Err(_) => continue,
        };

        let msg = DhtMessage::Ping {
            header: MessageHeader {
                magic: crate::rpc::MAGIC,
                msg_type: MessageType::Ping,
                txn_id: 0,
                ver: crate::rpc::PROTOCOL_VERSION,
            },
            node_id: local_id,
        };

        let encoded = msg.encode();
        if socket.send_to(&encoded, addr).await.is_ok() {
            let mut buf = [0u8; 1400];
            match tokio::time::timeout(Duration::from_secs(5), socket.recv_from(&mut buf)).await {
                Ok(Ok((len, _))) => {
                    if let Ok(response) = DhtMessage::decode(&buf[..len]) {
                        if let DhtMessage::PingResponse { node_id, .. } = response {
                            routing_table.insert(qvs_core::KBucketEntry {
                                node_id,
                                addr,
                                last_seen: std::time::Instant::now(),
                                latency: Duration::default(),
                                is_firewalled: false,
                            });
                        }
                    }
                }
                _ => continue,
            }
        }
    }

    if routing_table.size() == 0 {
        return Err(QvodError::DhtTimeout);
    }

    iterative_find_nodes(routing_table, &socket, &local_id, 3).await
}

async fn iterative_find_nodes(
    routing_table: &mut RoutingTable,
    socket: &tokio::net::UdpSocket,
    target: &NodeId,
    max_rounds: u32,
) -> Result<(), QvodError> {
    let mut queried = std::collections::HashSet::new();

    for round in 0..max_rounds {
        let closest = routing_table.find_closest(target, 8);
        if closest.is_empty() {
            break;
        }

        let mut found_any = false;
        for entry in &closest {
            if queried.contains(&entry.addr) {
                continue;
            }
            queried.insert(entry.addr);

            let msg = DhtMessage::FindNode {
                header: MessageHeader {
                    magic: crate::rpc::MAGIC,
                    msg_type: MessageType::FindNode,
                    txn_id: round as u16,
                    ver: crate::rpc::PROTOCOL_VERSION,
                },
                node_id: *routing_table.local_id(),
                target: *target,
            };

            let encoded = msg.encode();
            if socket.send_to(&encoded, entry.addr).await.is_ok() {
                let mut buf = [0u8; 1400];
                match tokio::time::timeout(Duration::from_secs(5), socket.recv_from(&mut buf)).await
                {
                    Ok(Ok((len, _))) => {
                        if let Ok(response) = DhtMessage::decode(&buf[..len]) {
                            if let DhtMessage::FindNodeResponse { nodes, .. } = response {
                                for node_info in &nodes {
                                    let node_addr = SocketAddr::new(
                                        std::net::IpAddr::V4(std::net::Ipv4Addr::from(
                                            node_info.ip,
                                        )),
                                        node_info.port,
                                    );
                                    routing_table.insert(qvs_core::KBucketEntry {
                                        node_id: node_info.node_id,
                                        addr: node_addr,
                                        last_seen: std::time::Instant::now(),
                                        latency: Duration::default(),
                                        is_firewalled: false,
                                    });
                                    found_any = true;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        if !found_any {
            break;
        }
    }

    Ok(())
}
