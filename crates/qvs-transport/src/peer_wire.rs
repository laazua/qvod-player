use qvs_core::{BlockRequest, QvodError};

use crate::message::{MsgId, PeerMessage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Initial,
    HandshakeSent,
    HandshakeReceived,
    Established,
    Disconnecting,
    Disconnected,
}

pub struct PeerWireProtocol {
    pub state: ConnectionState,
    pub am_choking: bool,
    pub am_interested: bool,
    pub peer_choking: bool,
    pub peer_interested: bool,
    bytes_sent: u64,
    bytes_received: u64,
    messages_sent: u64,
    messages_received: u64,
}

impl PeerWireProtocol {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: ConnectionState::Initial,
            am_choking: true,
            am_interested: false,
            peer_choking: true,
            peer_interested: false,
            bytes_sent: 0,
            bytes_received: 0,
            messages_sent: 0,
            messages_received: 0,
        }
    }

    pub fn on_handshake_sent(&mut self) {
        self.state = ConnectionState::HandshakeSent;
    }

    pub fn on_handshake_received(&mut self) {
        if self.state == ConnectionState::HandshakeSent {
            self.state = ConnectionState::Established;
        } else {
            self.state = ConnectionState::HandshakeReceived;
        }
    }

    pub fn on_established(&mut self) {
        self.state = ConnectionState::Established;
    }

    pub fn handle_message(&mut self, msg: &PeerMessage) -> Result<Option<PeerMessage>, QvodError> {
        self.messages_received += 1;
        match msg.msg_id {
            MsgId::Choke => {
                self.peer_choking = true;
                Ok(None)
            }
            MsgId::Unchoke => {
                self.peer_choking = false;
                Ok(None)
            }
            MsgId::Interested => {
                self.peer_interested = true;
                if self.am_choking {
                    Ok(Some(PeerMessage::new(MsgId::Unchoke, Vec::new())))
                } else {
                    Ok(None)
                }
            }
            MsgId::NotInterested => {
                self.peer_interested = false;
                Ok(None)
            }
            MsgId::Have
            | MsgId::Bitfield
            | MsgId::Piece
            | MsgId::Cancel
            | MsgId::Port
            | MsgId::HaveAll
            | MsgId::HaveNone
            | MsgId::SuggestPiece
            | MsgId::RejectRequest
            | MsgId::AllowedFast => Ok(None),
            MsgId::Request => {
                if self.am_choking {
                    return Err(QvodError::Protocol("requested while choked".into()));
                }
                Ok(None)
            }
        }
    }

    #[must_use]
    pub fn build_interested(&self) -> Option<PeerMessage> {
        if self.am_interested {
            None
        } else {
            Some(PeerMessage::new(MsgId::Interested, Vec::new()))
        }
    }

    #[must_use]
    pub fn build_not_interested(&self) -> Option<PeerMessage> {
        if self.am_interested {
            Some(PeerMessage::new(MsgId::NotInterested, Vec::new()))
        } else {
            None
        }
    }

    #[must_use]
    pub fn build_unchoke(&self) -> Option<PeerMessage> {
        if self.am_choking {
            Some(PeerMessage::new(MsgId::Unchoke, Vec::new()))
        } else {
            None
        }
    }

    #[must_use]
    pub fn build_choke(&self) -> Option<PeerMessage> {
        if self.am_choking {
            None
        } else {
            Some(PeerMessage::new(MsgId::Choke, Vec::new()))
        }
    }

    #[must_use]
    pub fn build_request(&self, block: &BlockRequest) -> PeerMessage {
        PeerMessage::request(block.piece_index, block.begin, block.length)
    }

    #[must_use]
    pub fn build_have(&self, piece_index: u32) -> PeerMessage {
        PeerMessage::have(piece_index)
    }

    #[must_use]
    pub fn build_bitfield(&self, bitfield: &[u8]) -> PeerMessage {
        PeerMessage::bitfield(bitfield.to_vec())
    }

    #[must_use]
    pub fn build_cancel(&self, block: &BlockRequest) -> PeerMessage {
        PeerMessage::cancel(block.piece_index, block.begin, block.length)
    }

    pub fn start_unchoke(&mut self) {
        self.am_choking = false;
    }

    pub fn start_choke(&mut self) {
        self.am_choking = true;
    }

    pub fn send_interested(&mut self) {
        self.am_interested = true;
    }

    pub fn send_not_interested(&mut self) {
        self.am_interested = false;
    }

    pub fn record_sent(&mut self, bytes: u64) {
        self.bytes_sent += bytes;
        self.messages_sent += 1;
    }

    pub fn record_received(&mut self, bytes: u64) {
        self.bytes_received += bytes;
        self.messages_received += 1;
    }

    #[must_use]
    pub fn is_ready_for_request(&self) -> bool {
        self.state == ConnectionState::Established && self.am_interested && !self.peer_choking
    }

    #[must_use]
    pub fn bytes_sent(&self) -> u64 {
        self.bytes_sent
    }
    #[must_use]
    pub fn bytes_received(&self) -> u64 {
        self.bytes_received
    }
    #[must_use]
    pub fn messages_sent(&self) -> u64 {
        self.messages_sent
    }
    #[must_use]
    pub fn messages_received(&self) -> u64 {
        self.messages_received
    }
}

impl Default for PeerWireProtocol {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let pw = PeerWireProtocol::new();
        assert_eq!(pw.state, ConnectionState::Initial);
        assert!(pw.am_choking);
        assert!(pw.peer_choking);
    }

    #[test]
    fn test_handshake_exchange() {
        let mut pw = PeerWireProtocol::new();
        pw.on_handshake_sent();
        assert_eq!(pw.state, ConnectionState::HandshakeSent);
        pw.on_handshake_received();
        assert_eq!(pw.state, ConnectionState::Established);
    }

    #[test]
    fn test_choke_unchoke() {
        let mut pw = PeerWireProtocol::new();
        let choke = PeerMessage::new(MsgId::Choke, Vec::new());
        pw.handle_message(&choke).unwrap();
        assert!(pw.peer_choking);

        let unchoke = PeerMessage::new(MsgId::Unchoke, Vec::new());
        pw.handle_message(&unchoke).unwrap();
        assert!(!pw.peer_choking);
    }

    #[test]
    fn test_interested_triggers_unchoke() {
        let mut pw = PeerWireProtocol::new();
        pw.start_unchoke();
        let interested = PeerMessage::new(MsgId::Interested, Vec::new());
        let response = pw.handle_message(&interested).unwrap();
        assert!(response.is_none());
        assert!(pw.peer_interested);
    }

    #[test]
    fn test_not_interested() {
        let mut pw = PeerWireProtocol::new();
        let msg = PeerMessage::new(MsgId::NotInterested, Vec::new());
        pw.handle_message(&msg).unwrap();
        assert!(!pw.peer_interested);
    }

    #[test]
    fn test_request_while_choked() {
        let mut pw = PeerWireProtocol::new();
        let req = PeerMessage::request(0, 0, 16384);
        let result = pw.handle_message(&req);
        assert!(result.is_err());
    }

    #[test]
    fn test_request_while_unchoked() {
        let mut pw = PeerWireProtocol::new();
        pw.am_choking = false;
        let req = PeerMessage::request(0, 0, 16384);
        let result = pw.handle_message(&req);
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_interested() {
        let pw = PeerWireProtocol::new();
        assert!(pw.build_interested().is_some());
    }

    #[test]
    fn test_build_unchoke() {
        let pw = PeerWireProtocol::new();
        assert!(pw.build_unchoke().is_some());
    }

    #[test]
    fn test_is_ready_for_request() {
        let mut pw = PeerWireProtocol::new();
        assert!(!pw.is_ready_for_request());
        pw.on_handshake_sent();
        pw.on_handshake_received();
        pw.send_interested();
        pw.peer_choking = false;
        assert!(pw.is_ready_for_request());
    }

    #[test]
    fn test_state_transitions() {
        let mut pw = PeerWireProtocol::new();
        assert_eq!(pw.state, ConnectionState::Initial);
        pw.on_handshake_sent();
        assert_eq!(pw.state, ConnectionState::HandshakeSent);
        pw.on_handshake_received();
        assert_eq!(pw.state, ConnectionState::Established);
    }

    #[test]
    fn test_build_request_message() {
        let pw = PeerWireProtocol::new();
        let block = BlockRequest {
            piece_index: 1,
            begin: 0,
            length: 16384,
        };
        let msg = pw.build_request(&block);
        assert_eq!(msg.msg_id, MsgId::Request);
    }

    #[test]
    fn test_build_have() {
        let pw = PeerWireProtocol::new();
        let msg = pw.build_have(42);
        assert_eq!(msg.msg_id, MsgId::Have);
        assert_eq!(msg.parse_have(), Some(42));
    }

    #[test]
    fn test_build_keepalive() {
        let ka = PeerMessage::keep_alive();
        assert_eq!(ka.len(), 4);
        assert_eq!(&ka, &[0u8; 4]);
    }
}
