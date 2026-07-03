use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::atomic::{AtomicU32, Ordering},
    time::{Duration, Instant},
};
use tokio::net::UdpSocket;

use qvs_core::QvodError;

use crate::congestion::UdpCongestionControl;

const MAX_PACKET_SIZE: usize = 1400;
const ACK_TIMEOUT: Duration = Duration::from_millis(500);
const MAX_RETRANSMITS: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UdpMsgType {
    Data = 0,
    Ack = 1,
    Nack = 2,
    Ping = 3,
    Pong = 4,
}

impl UdpMsgType {
    #[must_use]
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Data),
            1 => Some(Self::Ack),
            2 => Some(Self::Nack),
            3 => Some(Self::Ping),
            4 => Some(Self::Pong),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UdpPacket {
    pub msg_type: UdpMsgType,
    pub seq: u32,
    pub piece_index: u32,
    pub block_offset: u32,
    pub payload: Vec<u8>,
}

impl UdpPacket {
    #[must_use]
    pub fn new(
        msg_type: UdpMsgType,
        seq: u32,
        piece_index: u32,
        block_offset: u32,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            msg_type,
            seq,
            piece_index,
            block_offset,
            payload,
        }
    }

    #[must_use]
    pub fn ack(seq: u32) -> Self {
        Self::new(UdpMsgType::Ack, seq, 0, 0, Vec::new())
    }

    #[must_use]
    pub fn nack(seq: u32) -> Self {
        Self::new(UdpMsgType::Nack, seq, 0, 0, Vec::new())
    }

    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(15 + self.payload.len());
        buf.push(self.msg_type as u8);
        buf.extend_from_slice(&self.seq.to_be_bytes());
        buf.extend_from_slice(&self.piece_index.to_be_bytes());
        buf.extend_from_slice(&self.block_offset.to_be_bytes());
        buf.extend_from_slice(&(self.payload.len() as u16).to_be_bytes());
        buf.extend_from_slice(&self.payload);
        buf
    }

    pub fn decode(data: &[u8]) -> Result<Self, QvodError> {
        if data.len() < 15 {
            return Err(QvodError::Protocol("udp packet too short".into()));
        }
        let msg_type = UdpMsgType::from_u8(data[0])
            .ok_or_else(|| QvodError::Protocol(format!("unknown udp msg type: {}", data[0])))?;
        let seq = u32::from_be_bytes([data[1], data[2], data[3], data[4]]);
        let piece_index = u32::from_be_bytes([data[5], data[6], data[7], data[8]]);
        let block_offset = u32::from_be_bytes([data[9], data[10], data[11], data[12]]);
        let payload_len = u16::from_be_bytes([data[13], data[14]]) as usize;
        if 15 + payload_len > data.len() {
            return Err(QvodError::Protocol("udp packet payload truncated".into()));
        }
        let payload = data[15..15 + payload_len].to_vec();
        Ok(Self {
            msg_type,
            seq,
            piece_index,
            block_offset,
            payload,
        })
    }

    #[must_use]
    pub fn size(&self) -> usize {
        15 + self.payload.len()
    }
}

struct PendingPacket {
    packet: UdpPacket,
    sent_at: Instant,
    retransmits: u32,
    addr: SocketAddr,
}

pub struct UdpTransport {
    socket: UdpSocket,
    congestion: UdpCongestionControl,
    seq_counter: AtomicU32,
    pending: HashMap<u32, PendingPacket>,
    #[allow(dead_code)]
    next_check: Instant,
}

impl UdpTransport {
    pub async fn new(bind_addr: SocketAddr) -> Result<Self, QvodError> {
        let socket = UdpSocket::bind(bind_addr)
            .await
            .map_err(|e| QvodError::Network(e))?;
        Ok(Self {
            socket,
            congestion: UdpCongestionControl::new(),
            seq_counter: AtomicU32::new(1),
            pending: HashMap::new(),
            next_check: Instant::now(),
        })
    }

    pub async fn send_data(
        &mut self,
        piece_index: u32,
        block_offset: u32,
        payload: Vec<u8>,
        addr: SocketAddr,
    ) -> Result<(), QvodError> {
        if !self.congestion.can_send() {
            return Err(QvodError::Protocol("congestion window full".into()));
        }
        let seq = self.seq_counter.fetch_add(1, Ordering::SeqCst);
        let packet = UdpPacket::new(UdpMsgType::Data, seq, piece_index, block_offset, payload);
        self.congestion.on_packet_sent();
        let encoded = packet.encode();
        self.socket
            .send_to(&encoded, addr)
            .await
            .map_err(|e| QvodError::Network(e))?;
        self.pending.insert(
            seq,
            PendingPacket {
                packet,
                sent_at: Instant::now(),
                retransmits: 0,
                addr,
            },
        );
        Ok(())
    }

    pub async fn send_ack(&self, seq: u32, addr: SocketAddr) -> Result<(), QvodError> {
        let ack = UdpPacket::ack(seq);
        self.socket
            .send_to(&ack.encode(), addr)
            .await
            .map_err(|e| QvodError::Network(e))?;
        Ok(())
    }

    pub async fn receive(&mut self) -> Result<(UdpPacket, SocketAddr), QvodError> {
        let mut buf = [0u8; MAX_PACKET_SIZE];
        let (len, addr) = self
            .socket
            .recv_from(&mut buf)
            .await
            .map_err(|e| QvodError::Network(e))?;
        let packet = UdpPacket::decode(&buf[..len])?;

        match packet.msg_type {
            UdpMsgType::Ack => {
                self.congestion.on_ack(Duration::from_millis(50));
                self.pending.remove(&packet.seq);
            }
            UdpMsgType::Nack => {
                if let Some(p) = self.pending.get(&packet.seq) {
                    if p.retransmits < MAX_RETRANSMITS {
                        self.socket
                            .send_to(&p.packet.encode(), p.addr)
                            .await
                            .map_err(|e| QvodError::Network(e))?;
                    }
                }
            }
            UdpMsgType::Ping => {
                let pong = UdpPacket::new(UdpMsgType::Pong, packet.seq, 0, 0, Vec::new());
                let _ = self.socket.send_to(&pong.encode(), addr).await;
            }
            _ => {}
        }

        Ok((packet, addr))
    }

    pub async fn retransmit_timeout(&mut self) {
        let now = Instant::now();
        let to_retransmit: Vec<u32> = self
            .pending
            .iter()
            .filter(|(_, p)| {
                now.duration_since(p.sent_at) > ACK_TIMEOUT && p.retransmits < MAX_RETRANSMITS
            })
            .map(|(seq, _)| *seq)
            .collect();

        for seq in to_retransmit {
            if let Some(p) = self.pending.get_mut(&seq) {
                p.retransmits += 1;
                p.sent_at = now;
                self.congestion.on_loss();
                if self
                    .socket
                    .send_to(&p.packet.encode(), p.addr)
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }

        let expired: Vec<u32> = self
            .pending
            .iter()
            .filter(|(_, p)| p.retransmits >= MAX_RETRANSMITS)
            .map(|(seq, _)| *seq)
            .collect();
        for seq in expired {
            self.congestion.on_timeout();
            self.pending.remove(&seq);
        }
    }

    pub fn congestion_control(&self) -> &UdpCongestionControl {
        &self.congestion
    }

    pub fn congestion_control_mut(&mut self) -> &mut UdpCongestionControl {
        &mut self.congestion
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.socket.local_addr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_udp_packet_roundtrip() {
        let packet = UdpPacket::new(UdpMsgType::Data, 42, 1, 0, vec![0xAB; 100]);
        let encoded = packet.encode();
        let decoded = UdpPacket::decode(&encoded).unwrap();
        assert_eq!(decoded.msg_type, UdpMsgType::Data);
        assert_eq!(decoded.seq, 42);
        assert_eq!(decoded.piece_index, 1);
        assert_eq!(decoded.block_offset, 0);
        assert_eq!(decoded.payload.len(), 100);
    }

    #[test]
    fn test_ack_packet() {
        let ack = UdpPacket::ack(100);
        assert_eq!(ack.msg_type, UdpMsgType::Ack);
        assert_eq!(ack.seq, 100);
        let encoded = ack.encode();
        let decoded = UdpPacket::decode(&encoded).unwrap();
        assert_eq!(decoded.msg_type, UdpMsgType::Ack);
        assert_eq!(decoded.seq, 100);
    }

    #[test]
    fn test_packet_too_short() {
        let result = UdpPacket::decode(&[0u8; 5]);
        assert!(result.is_err());
    }

    #[test]
    fn test_packet_size_limit() {
        let payload_size = MAX_PACKET_SIZE - 15;
        let packet = UdpPacket::new(UdpMsgType::Data, 1, 0, 0, vec![0u8; payload_size]);
        assert!(packet.size() <= MAX_PACKET_SIZE);
        assert_eq!(packet.size(), MAX_PACKET_SIZE);
    }

    #[test]
    fn test_udp_msg_type_from_u8() {
        assert_eq!(UdpMsgType::from_u8(0), Some(UdpMsgType::Data));
        assert_eq!(UdpMsgType::from_u8(4), Some(UdpMsgType::Pong));
        assert_eq!(UdpMsgType::from_u8(255), None);
    }
}
