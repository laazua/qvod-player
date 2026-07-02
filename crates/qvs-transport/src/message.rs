use qvs_core::QvodError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MsgId {
    Choke = 0,
    Unchoke = 1,
    Interested = 2,
    NotInterested = 3,
    Have = 4,
    Bitfield = 5,
    Request = 6,
    Piece = 7,
    Cancel = 8,
    Port = 9,
    SuggestPiece = 13,
    HaveAll = 14,
    HaveNone = 15,
    RejectRequest = 16,
    AllowedFast = 17,
}

impl MsgId {
    #[must_use]
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Choke),
            1 => Some(Self::Unchoke),
            2 => Some(Self::Interested),
            3 => Some(Self::NotInterested),
            4 => Some(Self::Have),
            5 => Some(Self::Bitfield),
            6 => Some(Self::Request),
            7 => Some(Self::Piece),
            8 => Some(Self::Cancel),
            9 => Some(Self::Port),
            13 => Some(Self::SuggestPiece),
            14 => Some(Self::HaveAll),
            15 => Some(Self::HaveNone),
            16 => Some(Self::RejectRequest),
            17 => Some(Self::AllowedFast),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PeerMessage {
    pub msg_id: MsgId,
    pub payload: Vec<u8>,
}

impl PeerMessage {
    #[must_use]
    pub fn new(msg_id: MsgId, payload: Vec<u8>) -> Self {
        Self { msg_id, payload }
    }

    #[must_use]
    pub fn keep_alive() -> Vec<u8> {
        vec![0u8; 4]
    }

    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let len = 1 + self.payload.len() as u32;
        let mut buf = Vec::with_capacity(4 + len as usize);
        buf.extend_from_slice(&len.to_be_bytes());
        buf.push(self.msg_id as u8);
        buf.extend_from_slice(&self.payload);
        buf
    }

    pub fn decode(data: &[u8]) -> Result<Self, QvodError> {
        if data.len() < 4 {
            return Err(QvodError::Protocol("message too short".into()));
        }
        let len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        if len == 0 {
            return Err(QvodError::Protocol("keep-alive has no msg_id".into()));
        }
        if 4 + len > data.len() {
            return Err(QvodError::Protocol("message truncated".into()));
        }
        let msg_id = MsgId::from_u8(data[4])
            .ok_or_else(|| QvodError::Protocol(format!("unknown msg_id: {}", data[4])))?;
        let payload = data[5..4 + len].to_vec();
        Ok(Self { msg_id, payload })
    }

    #[must_use]
    pub fn have(piece_index: u32) -> Self {
        Self {
            msg_id: MsgId::Have,
            payload: piece_index.to_be_bytes().to_vec(),
        }
    }

    #[must_use]
    pub fn bitfield(bytes: Vec<u8>) -> Self {
        Self {
            msg_id: MsgId::Bitfield,
            payload: bytes,
        }
    }

    #[must_use]
    pub fn request(index: u32, begin: u32, length: u32) -> Self {
        let mut payload = Vec::with_capacity(12);
        payload.extend_from_slice(&index.to_be_bytes());
        payload.extend_from_slice(&begin.to_be_bytes());
        payload.extend_from_slice(&length.to_be_bytes());
        Self {
            msg_id: MsgId::Request,
            payload,
        }
    }

    #[must_use]
    pub fn piece(index: u32, begin: u32, data: &[u8]) -> Self {
        let mut payload = Vec::with_capacity(8 + data.len());
        payload.extend_from_slice(&index.to_be_bytes());
        payload.extend_from_slice(&begin.to_be_bytes());
        payload.extend_from_slice(data);
        Self {
            msg_id: MsgId::Piece,
            payload,
        }
    }

    #[must_use]
    pub fn cancel(index: u32, begin: u32, length: u32) -> Self {
        let mut payload = Vec::with_capacity(12);
        payload.extend_from_slice(&index.to_be_bytes());
        payload.extend_from_slice(&begin.to_be_bytes());
        payload.extend_from_slice(&length.to_be_bytes());
        Self {
            msg_id: MsgId::Cancel,
            payload,
        }
    }

    #[must_use]
    pub fn port(port: u16) -> Self {
        Self {
            msg_id: MsgId::Port,
            payload: port.to_be_bytes().to_vec(),
        }
    }

    #[must_use]
    pub fn have_all() -> Self {
        Self {
            msg_id: MsgId::HaveAll,
            payload: Vec::new(),
        }
    }

    #[must_use]
    pub fn have_none() -> Self {
        Self {
            msg_id: MsgId::HaveNone,
            payload: Vec::new(),
        }
    }

    #[must_use]
    pub fn reject_request(index: u32, begin: u32, length: u32) -> Self {
        let mut payload = Vec::with_capacity(12);
        payload.extend_from_slice(&index.to_be_bytes());
        payload.extend_from_slice(&begin.to_be_bytes());
        payload.extend_from_slice(&length.to_be_bytes());
        Self {
            msg_id: MsgId::RejectRequest,
            payload,
        }
    }

    #[must_use]
    pub fn allowed_fast(index: u32) -> Self {
        Self {
            msg_id: MsgId::AllowedFast,
            payload: index.to_be_bytes().to_vec(),
        }
    }

    #[must_use]
    pub fn suggest_piece(index: u32) -> Self {
        Self {
            msg_id: MsgId::SuggestPiece,
            payload: index.to_be_bytes().to_vec(),
        }
    }

    #[must_use]
    pub fn parse_have(&self) -> Option<u32> {
        if self.msg_id != MsgId::Have || self.payload.len() < 4 {
            return None;
        }
        Some(u32::from_be_bytes([
            self.payload[0],
            self.payload[1],
            self.payload[2],
            self.payload[3],
        ]))
    }

    #[must_use]
    pub fn parse_request(&self) -> Option<(u32, u32, u32)> {
        if self.msg_id != MsgId::Request || self.payload.len() < 12 {
            return None;
        }
        let index = u32::from_be_bytes([
            self.payload[0],
            self.payload[1],
            self.payload[2],
            self.payload[3],
        ]);
        let begin = u32::from_be_bytes([
            self.payload[4],
            self.payload[5],
            self.payload[6],
            self.payload[7],
        ]);
        let length = u32::from_be_bytes([
            self.payload[8],
            self.payload[9],
            self.payload[10],
            self.payload[11],
        ]);
        Some((index, begin, length))
    }

    #[must_use]
    pub fn parse_piece(&self) -> Option<(u32, u32, &[u8])> {
        if self.msg_id != MsgId::Piece || self.payload.len() < 8 {
            return None;
        }
        let index = u32::from_be_bytes([
            self.payload[0],
            self.payload[1],
            self.payload[2],
            self.payload[3],
        ]);
        let begin = u32::from_be_bytes([
            self.payload[4],
            self.payload[5],
            self.payload[6],
            self.payload[7],
        ]);
        Some((index, begin, &self.payload[8..]))
    }

    #[must_use]
    pub fn parse_bitfield(&self) -> Option<Vec<u8>> {
        if self.msg_id != MsgId::Bitfield || self.payload.is_empty() {
            return None;
        }
        Some(self.payload.clone())
    }

    #[must_use]
    pub fn parse_cancel(&self) -> Option<(u32, u32, u32)> {
        if self.msg_id != MsgId::Cancel || self.payload.len() < 12 {
            return None;
        }
        let index = u32::from_be_bytes([
            self.payload[0],
            self.payload[1],
            self.payload[2],
            self.payload[3],
        ]);
        let begin = u32::from_be_bytes([
            self.payload[4],
            self.payload[5],
            self.payload[6],
            self.payload[7],
        ]);
        let length = u32::from_be_bytes([
            self.payload[8],
            self.payload[9],
            self.payload[10],
            self.payload[11],
        ]);
        Some((index, begin, length))
    }

    #[must_use]
    pub fn parse_port(&self) -> Option<u16> {
        if self.msg_id != MsgId::Port || self.payload.len() < 2 {
            return None;
        }
        Some(u16::from_be_bytes([self.payload[0], self.payload[1]]))
    }

    #[must_use]
    pub fn parse_suggest_piece(&self) -> Option<u32> {
        if self.msg_id != MsgId::SuggestPiece || self.payload.len() < 4 {
            return None;
        }
        Some(u32::from_be_bytes([
            self.payload[0],
            self.payload[1],
            self.payload[2],
            self.payload[3],
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(msg: &PeerMessage) -> PeerMessage {
        let encoded = msg.encode();
        PeerMessage::decode(&encoded).unwrap()
    }

    #[test]
    fn test_keep_alive() {
        let ka = PeerMessage::keep_alive();
        assert_eq!(ka.len(), 4);
        assert_eq!(&ka, &[0u8; 4]);
    }

    #[test]
    fn test_have_roundtrip() {
        let msg = PeerMessage::have(42);
        let decoded = roundtrip(&msg);
        assert_eq!(decoded.msg_id, MsgId::Have);
        assert_eq!(decoded.parse_have(), Some(42));
    }

    #[test]
    fn test_bitfield_roundtrip() {
        let bf = vec![0xFF, 0x00, 0xAA, 0x55];
        let msg = PeerMessage::bitfield(bf.clone());
        let decoded = roundtrip(&msg);
        assert_eq!(decoded.msg_id, MsgId::Bitfield);
        assert_eq!(decoded.parse_bitfield(), Some(bf));
    }

    #[test]
    fn test_request_roundtrip() {
        let msg = PeerMessage::request(1, 2, 16384);
        let decoded = roundtrip(&msg);
        assert_eq!(decoded.msg_id, MsgId::Request);
        assert_eq!(decoded.parse_request(), Some((1, 2, 16384)));
    }

    #[test]
    fn test_piece_roundtrip() {
        let data = vec![0xABu8; 1024];
        let msg = PeerMessage::piece(5, 0, &data);
        let decoded = roundtrip(&msg);
        assert_eq!(decoded.msg_id, MsgId::Piece);
        let (index, begin, payload) = decoded.parse_piece().unwrap();
        assert_eq!(index, 5);
        assert_eq!(begin, 0);
        assert_eq!(payload, &data[..]);
    }

    #[test]
    fn test_cancel_roundtrip() {
        let msg = PeerMessage::cancel(10, 20, 30);
        let decoded = roundtrip(&msg);
        assert_eq!(decoded.msg_id, MsgId::Cancel);
        assert_eq!(decoded.parse_cancel(), Some((10, 20, 30)));
    }

    #[test]
    fn test_port_roundtrip() {
        let msg = PeerMessage::port(8621);
        let decoded = roundtrip(&msg);
        assert_eq!(decoded.msg_id, MsgId::Port);
        assert_eq!(decoded.parse_port(), Some(8621));
    }

    #[test]
    fn test_suggest_piece_roundtrip() {
        let msg = PeerMessage::suggest_piece(77);
        let decoded = roundtrip(&msg);
        assert_eq!(decoded.msg_id, MsgId::SuggestPiece);
        assert_eq!(decoded.parse_suggest_piece(), Some(77));
    }

    #[test]
    fn test_have_all_roundtrip() {
        let msg = PeerMessage::have_all();
        let decoded = roundtrip(&msg);
        assert_eq!(decoded.msg_id, MsgId::HaveAll);
        assert!(decoded.payload.is_empty());
    }

    #[test]
    fn test_have_none_roundtrip() {
        let msg = PeerMessage::have_none();
        let decoded = roundtrip(&msg);
        assert_eq!(decoded.msg_id, MsgId::HaveNone);
        assert!(decoded.payload.is_empty());
    }

    #[test]
    fn test_reject_request_roundtrip() {
        let msg = PeerMessage::reject_request(3, 4096, 16384);
        let decoded = roundtrip(&msg);
        assert_eq!(decoded.msg_id, MsgId::RejectRequest);
        assert_eq!(decoded.payload.len(), 12);
        let idx = u32::from_be_bytes([
            decoded.payload[0],
            decoded.payload[1],
            decoded.payload[2],
            decoded.payload[3],
        ]);
        let begin = u32::from_be_bytes([
            decoded.payload[4],
            decoded.payload[5],
            decoded.payload[6],
            decoded.payload[7],
        ]);
        let len = u32::from_be_bytes([
            decoded.payload[8],
            decoded.payload[9],
            decoded.payload[10],
            decoded.payload[11],
        ]);
        assert_eq!((idx, begin, len), (3, 4096, 16384));
    }

    #[test]
    fn test_allowed_fast_roundtrip() {
        let msg = PeerMessage::allowed_fast(99);
        let decoded = roundtrip(&msg);
        assert_eq!(decoded.msg_id, MsgId::AllowedFast);
        assert_eq!(decoded.payload.len(), 4);
        let idx = u32::from_be_bytes([
            decoded.payload[0],
            decoded.payload[1],
            decoded.payload[2],
            decoded.payload[3],
        ]);
        assert_eq!(idx, 99);
    }

    #[test]
    fn test_choke_unchoke_roundtrip() {
        let choke = PeerMessage::new(MsgId::Choke, vec![]);
        let decoded = roundtrip(&choke);
        assert_eq!(decoded.msg_id, MsgId::Choke);

        let unchoke = PeerMessage::new(MsgId::Unchoke, vec![]);
        let decoded = roundtrip(&unchoke);
        assert_eq!(decoded.msg_id, MsgId::Unchoke);
    }

    #[test]
    fn test_interested_not_interested_roundtrip() {
        let interested = PeerMessage::new(MsgId::Interested, vec![]);
        let decoded = roundtrip(&interested);
        assert_eq!(decoded.msg_id, MsgId::Interested);

        let not_interested = PeerMessage::new(MsgId::NotInterested, vec![]);
        let decoded = roundtrip(&not_interested);
        assert_eq!(decoded.msg_id, MsgId::NotInterested);
    }

    #[test]
    fn test_unknown_msg_id() {
        let data = vec![0u8, 0, 0, 1, 99];
        let result = PeerMessage::decode(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_msg_id_from_u8() {
        assert_eq!(MsgId::from_u8(0), Some(MsgId::Choke));
        assert_eq!(MsgId::from_u8(7), Some(MsgId::Piece));
        assert_eq!(MsgId::from_u8(255), None);
    }

    #[test]
    fn test_keep_alive_decode_error() {
        let encoded = PeerMessage::keep_alive();
        let result = PeerMessage::decode(&encoded);
        assert!(result.is_err());
    }

    #[test]
    fn test_truncated_message() {
        let data = vec![0u8, 0, 0, 10, 4]; // length=10 but only 1 payload byte
        let result = PeerMessage::decode(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_bitfield_parse_empty_fails() {
        let msg = PeerMessage::new(MsgId::Bitfield, vec![]);
        assert!(msg.parse_bitfield().is_none());
    }

    #[test]
    fn test_parse_wrong_msg_id() {
        let msg = PeerMessage::have(5);
        assert!(msg.parse_bitfield().is_none());
        assert!(msg.parse_cancel().is_none());
        assert!(msg.parse_port().is_none());
        assert!(msg.parse_suggest_piece().is_none());
    }

    #[test]
    fn test_reject_request_uses_cancel_parse() {
        let msg = PeerMessage::reject_request(7, 8192, 32768);
        assert_eq!(msg.payload.len(), 12);
        let idx = u32::from_be_bytes([
            msg.payload[0],
            msg.payload[1],
            msg.payload[2],
            msg.payload[3],
        ]);
        let begin = u32::from_be_bytes([
            msg.payload[4],
            msg.payload[5],
            msg.payload[6],
            msg.payload[7],
        ]);
        let len = u32::from_be_bytes([
            msg.payload[8],
            msg.payload[9],
            msg.payload[10],
            msg.payload[11],
        ]);
        assert_eq!((idx, begin, len), (7, 8192, 32768));
    }

    #[test]
    fn test_allowed_fast_uses_have_parse() {
        let msg = PeerMessage::allowed_fast(42);
        assert_eq!(msg.payload.len(), 4);
        let idx = u32::from_be_bytes([
            msg.payload[0],
            msg.payload[1],
            msg.payload[2],
            msg.payload[3],
        ]);
        assert_eq!(idx, 42);
    }
}
