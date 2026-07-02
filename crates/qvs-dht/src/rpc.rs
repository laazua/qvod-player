use qvs_core::{NodeId, QvodError};

pub const MAGIC: [u8; 4] = [0x51, 0x56, 0x44, 0x54];
pub const PROTOCOL_VERSION: u8 = 0x01;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    Ping = 0x00,
    FindNode = 0x01,
    FindPeers = 0x02,
    Announce = 0x03,
}

impl MessageType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x00 => Some(Self::Ping),
            0x01 => Some(Self::FindNode),
            0x02 => Some(Self::FindPeers),
            0x03 => Some(Self::Announce),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MessageHeader {
    pub magic: [u8; 4],
    pub msg_type: MessageType,
    pub txn_id: u16,
    pub ver: u8,
}

#[derive(Debug, Clone)]
pub enum DhtMessage {
    Ping {
        header: MessageHeader,
        node_id: NodeId,
    },
    PingResponse {
        header: MessageHeader,
        node_id: NodeId,
    },
    FindNode {
        header: MessageHeader,
        node_id: NodeId,
        target: NodeId,
    },
    FindNodeResponse {
        header: MessageHeader,
        node_id: NodeId,
        nodes: Vec<NodeInfo>,
    },
    FindPeers {
        header: MessageHeader,
        node_id: NodeId,
        info_hash: [u8; 20],
    },
    FindPeersResponse {
        header: MessageHeader,
        node_id: NodeId,
        values: FindPeersResult,
    },
    Announce {
        header: MessageHeader,
        node_id: NodeId,
        info_hash: [u8; 20],
        token: [u8; 4],
        port: u16,
    },
    AnnounceResponse {
        header: MessageHeader,
        node_id: NodeId,
    },
}

#[derive(Debug, Clone)]
pub enum FindPeersResult {
    Peers(Vec<[u8; 6]>),
    Nodes(Vec<NodeInfo>),
}

#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub node_id: NodeId,
    pub ip: [u8; 4],
    pub port: u16,
}

impl NodeInfo {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(26);
        buf.extend_from_slice(&self.node_id.0);
        buf.extend_from_slice(&self.ip);
        buf.extend_from_slice(&self.port.to_be_bytes());
        buf
    }

    pub fn decode(data: &[u8]) -> Result<(Self, &[u8]), QvodError> {
        if data.len() < 26 {
            return Err(QvodError::Protocol("node info too short".into()));
        }
        let mut node_id = [0u8; 20];
        node_id.copy_from_slice(&data[..20]);
        let mut ip = [0u8; 4];
        ip.copy_from_slice(&data[20..24]);
        let port = u16::from_be_bytes([data[24], data[25]]);
        Ok((
            Self {
                node_id: NodeId(node_id),
                ip,
                port,
            },
            &data[26..],
        ))
    }
}

impl DhtMessage {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::Ping { header, node_id } | Self::PingResponse { header, node_id } => {
                let mut buf = encode_header(header);
                buf.extend_from_slice(&node_id.0);
                buf
            }
            Self::FindNode {
                header,
                node_id,
                target,
            } => {
                let mut buf = encode_header(header);
                buf.extend_from_slice(&node_id.0);
                buf.extend_from_slice(&target.0);
                buf
            }
            Self::FindNodeResponse {
                header,
                node_id,
                nodes,
            } => {
                let mut buf = encode_header(header);
                buf.extend_from_slice(&node_id.0);
                for node in nodes {
                    buf.extend_from_slice(&node.encode());
                }
                buf
            }
            Self::FindPeers {
                header,
                node_id,
                info_hash,
            } => {
                let mut buf = encode_header(header);
                buf.extend_from_slice(&node_id.0);
                buf.extend_from_slice(info_hash);
                buf
            }
            Self::FindPeersResponse {
                header,
                node_id,
                values,
            } => {
                let mut buf = encode_header(header);
                buf.extend_from_slice(&node_id.0);
                match values {
                    FindPeersResult::Peers(peers) => {
                        buf.push(0x00);
                        let count = peers.len() as u16;
                        buf.extend_from_slice(&count.to_be_bytes());
                        for peer in peers {
                            buf.extend_from_slice(peer);
                        }
                    }
                    FindPeersResult::Nodes(nodes) => {
                        buf.push(0x01);
                        let count = nodes.len() as u16;
                        buf.extend_from_slice(&count.to_be_bytes());
                        for node in nodes {
                            buf.extend_from_slice(&node.encode());
                        }
                    }
                }
                buf
            }
            Self::Announce {
                header,
                node_id,
                info_hash,
                token,
                port,
            } => {
                let mut buf = encode_header(header);
                buf.extend_from_slice(&node_id.0);
                buf.extend_from_slice(info_hash);
                buf.extend_from_slice(token);
                buf.extend_from_slice(&port.to_be_bytes());
                buf
            }
            Self::AnnounceResponse { header, node_id } => {
                let mut buf = encode_header(header);
                buf.extend_from_slice(&node_id.0);
                buf.push(0x00);
                buf
            }
        }
    }

    pub fn decode(data: &[u8]) -> Result<Self, QvodError> {
        let (header, rest) = decode_header(data)?;

        match header.msg_type {
            MessageType::Ping => {
                if rest.len() < 20 {
                    return Err(QvodError::Protocol("ping too short".into()));
                }
                let mut node_id = [0u8; 20];
                node_id.copy_from_slice(&rest[..20]);
                Ok(Self::Ping {
                    header,
                    node_id: NodeId(node_id),
                })
            }
            MessageType::FindNode => {
                if rest.len() < 20 {
                    return Err(QvodError::Protocol("find_node too short".into()));
                }
                let mut node_id = [0u8; 20];
                node_id.copy_from_slice(&rest[..20]);

                if rest.len() == 40 {
                    let mut target = [0u8; 20];
                    target.copy_from_slice(&rest[20..40]);
                    Ok(Self::FindNode {
                        header,
                        node_id: NodeId(node_id),
                        target: NodeId(target),
                    })
                } else {
                    let mut nodes = Vec::new();
                    let mut remaining = &rest[20..];
                    while remaining.len() >= 26 {
                        let (node, rem) = NodeInfo::decode(remaining)?;
                        nodes.push(node);
                        remaining = rem;
                    }
                    Ok(Self::FindNodeResponse {
                        header,
                        node_id: NodeId(node_id),
                        nodes,
                    })
                }
            }
            MessageType::FindPeers => {
                if rest.len() < 40 {
                    return Err(QvodError::Protocol("find_peers too short".into()));
                }
                let mut node_id = [0u8; 20];
                node_id.copy_from_slice(&rest[..20]);
                let mut info_hash = [0u8; 20];
                info_hash.copy_from_slice(&rest[20..40]);

                if rest.len() == 40 {
                    Ok(Self::FindPeers {
                        header,
                        node_id: NodeId(node_id),
                        info_hash,
                    })
                } else {
                    let tag = rest[40];
                    let payload = &rest[41..];
                    if payload.len() < 2 {
                        return Err(QvodError::Protocol("find_peers response too short".into()));
                    }
                    let count = u16::from_be_bytes([payload[0], payload[1]]) as usize;
                    let data = &payload[2..];
                    let result = if tag == 0x00 {
                        let mut peers = Vec::with_capacity(count);
                        let mut rem = data;
                        for _ in 0..count {
                            if rem.len() < 6 {
                                break;
                            }
                            let mut peer = [0u8; 6];
                            peer.copy_from_slice(&rem[..6]);
                            peers.push(peer);
                            rem = &rem[6..];
                        }
                        FindPeersResult::Peers(peers)
                    } else {
                        let mut nodes = Vec::with_capacity(count);
                        let mut rem = data;
                        for _ in 0..count {
                            if rem.len() < 26 {
                                break;
                            }
                            if let Ok((node, rest_data)) = NodeInfo::decode(rem) {
                                nodes.push(node);
                                rem = rest_data;
                            }
                        }
                        FindPeersResult::Nodes(nodes)
                    };
                    Ok(Self::FindPeersResponse {
                        header,
                        node_id: NodeId(node_id),
                        values: result,
                    })
                }
            }
            MessageType::Announce => {
                if rest.len() < 21 {
                    return Err(QvodError::Protocol("announce too short".into()));
                }
                let mut node_id = [0u8; 20];
                node_id.copy_from_slice(&rest[..20]);

                if rest.len() == 21 {
                    Ok(Self::AnnounceResponse {
                        header,
                        node_id: NodeId(node_id),
                    })
                } else if rest.len() >= 46 {
                    let mut info_hash = [0u8; 20];
                    info_hash.copy_from_slice(&rest[20..40]);
                    let mut token = [0u8; 4];
                    token.copy_from_slice(&rest[40..44]);
                    let port = u16::from_be_bytes([rest[44], rest[45]]);
                    Ok(Self::Announce {
                        header,
                        node_id: NodeId(node_id),
                        info_hash,
                        token,
                        port,
                    })
                } else {
                    Err(QvodError::Protocol("announce payload incomplete".into()))
                }
            }
        }
    }
}

impl DhtMessage {
    pub fn is_response(&self) -> bool {
        matches!(
            self,
            Self::PingResponse { .. }
                | Self::FindNodeResponse { .. }
                | Self::FindPeersResponse { .. }
                | Self::AnnounceResponse { .. }
        )
    }
}

fn encode_header(header: &MessageHeader) -> Vec<u8> {
    let mut buf = Vec::with_capacity(8);
    buf.extend_from_slice(&MAGIC);
    buf.push(header.msg_type as u8);
    buf.extend_from_slice(&header.txn_id.to_be_bytes());
    buf.push(PROTOCOL_VERSION);
    buf
}

fn decode_header(data: &[u8]) -> Result<(MessageHeader, &[u8]), QvodError> {
    if data.len() < 8 {
        return Err(QvodError::Protocol("header too short".into()));
    }
    let mut magic = [0u8; 4];
    magic.copy_from_slice(&data[..4]);
    if magic != MAGIC {
        return Err(QvodError::Protocol("invalid magic".into()));
    }
    let msg_type = MessageType::from_u8(data[4])
        .ok_or_else(|| QvodError::Protocol("unknown msg type".into()))?;
    let txn_id = u16::from_be_bytes([data[5], data[6]]);
    let ver = data[7];
    if ver != PROTOCOL_VERSION {
        return Err(QvodError::Protocol("unsupported version".into()));
    }
    Ok((
        MessageHeader {
            magic,
            msg_type,
            txn_id,
            ver,
        },
        &data[8..],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_header(msg_type: MessageType) -> MessageHeader {
        MessageHeader {
            magic: MAGIC,
            msg_type,
            txn_id: 1,
            ver: PROTOCOL_VERSION,
        }
    }

    #[test]
    fn test_ping_roundtrip() {
        let msg = DhtMessage::Ping {
            header: sample_header(MessageType::Ping),
            node_id: NodeId([1u8; 20]),
        };
        let encoded = msg.encode();
        let decoded = DhtMessage::decode(&encoded).unwrap();
        match decoded {
            DhtMessage::Ping { node_id, .. } => {
                assert_eq!(node_id.0, [1u8; 20]);
            }
            _ => panic!("wrong type"),
        }
    }

    #[test]
    fn test_find_node_request_roundtrip() {
        let msg = DhtMessage::FindNode {
            header: sample_header(MessageType::FindNode),
            node_id: NodeId([1u8; 20]),
            target: NodeId([2u8; 20]),
        };
        let encoded = msg.encode();
        assert_eq!(encoded.len(), 8 + 20 + 20);
        let decoded = DhtMessage::decode(&encoded).unwrap();
        match decoded {
            DhtMessage::FindNode {
                node_id, target, ..
            } => {
                assert_eq!(node_id.0, [1u8; 20]);
                assert_eq!(target.0, [2u8; 20]);
            }
            other => panic!("wrong type: {:?}", other),
        }
    }

    #[test]
    fn test_find_node_response_roundtrip() {
        let nodes = vec![
            NodeInfo {
                node_id: NodeId([1u8; 20]),
                ip: [192, 168, 1, 1],
                port: 8621,
            },
            NodeInfo {
                node_id: NodeId([2u8; 20]),
                ip: [10, 0, 0, 1],
                port: 6881,
            },
        ];
        let msg = DhtMessage::FindNodeResponse {
            header: sample_header(MessageType::FindNode),
            node_id: NodeId([3u8; 20]),
            nodes,
        };
        let encoded = msg.encode();
        let decoded = DhtMessage::decode(&encoded).unwrap();
        match decoded {
            DhtMessage::FindNodeResponse { nodes, .. } => {
                assert_eq!(nodes.len(), 2);
                assert_eq!(nodes[0].ip, [192, 168, 1, 1]);
                assert_eq!(nodes[1].ip, [10, 0, 0, 1]);
            }
            other => panic!("wrong type: {:?}", other),
        }
    }

    #[test]
    fn test_invalid_magic() {
        let bad = vec![0u8; 8];
        let err = DhtMessage::decode(&bad).unwrap_err();
        assert!(err.to_string().contains("invalid magic"));
    }

    #[test]
    fn test_announce_request_roundtrip() {
        let msg = DhtMessage::Announce {
            header: sample_header(MessageType::Announce),
            node_id: NodeId([1u8; 20]),
            info_hash: [2u8; 20],
            token: [0xAB, 0xCD, 0xEF, 0x01],
            port: 8621,
        };
        let encoded = msg.encode();
        let decoded = DhtMessage::decode(&encoded).unwrap();
        match decoded {
            DhtMessage::Announce { token, port, .. } => {
                assert_eq!(token, [0xAB, 0xCD, 0xEF, 0x01]);
                assert_eq!(port, 8621);
            }
            other => panic!("wrong type: {:?}", other),
        }
    }

    #[test]
    fn test_announce_response_roundtrip() {
        let msg = DhtMessage::AnnounceResponse {
            header: sample_header(MessageType::Announce),
            node_id: NodeId([1u8; 20]),
        };
        let encoded = msg.encode();
        let decoded = DhtMessage::decode(&encoded).unwrap();
        match decoded {
            DhtMessage::AnnounceResponse { .. } => {}
            other => panic!("wrong type: {:?}", other),
        }
    }

    #[test]
    fn test_find_peers_request_roundtrip() {
        let msg = DhtMessage::FindPeers {
            header: sample_header(MessageType::FindPeers),
            node_id: NodeId([1u8; 20]),
            info_hash: [2u8; 20],
        };
        let encoded = msg.encode();
        let decoded = DhtMessage::decode(&encoded).unwrap();
        match decoded {
            DhtMessage::FindPeers {
                node_id, info_hash, ..
            } => {
                assert_eq!(node_id.0, [1u8; 20]);
                assert_eq!(info_hash, [2u8; 20]);
            }
            other => panic!("wrong type: {:?}", other),
        }
    }
}
