use qvs_core::{InfoHash, QvodError};

const HANDSHAKE_PROTOCOL: &str = "Qvod P2SP Protocol";

#[derive(Debug, Clone)]
pub struct Handshake {
    pub info_hash: InfoHash,
    pub peer_id: [u8; 20],
    pub reserved: [u8; 8],
    pub supports_metadata: bool,
}

impl Handshake {
    #[must_use]
    pub fn new(info_hash: InfoHash, peer_id: [u8; 20]) -> Self {
        let mut reserved = [0u8; 8];
        reserved[5] |= 0x10;
        Self {
            info_hash,
            peer_id,
            reserved,
            supports_metadata: true,
        }
    }

    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let pstr = HANDSHAKE_PROTOCOL.as_bytes();
        let total_len = 1 + pstr.len() + 8 + 20 + 20;
        let mut buf = vec![0u8; total_len];
        buf[0] = pstr.len() as u8;
        buf[1..1 + pstr.len()].copy_from_slice(pstr);
        let off = 1 + pstr.len();
        buf[off..off + 8].copy_from_slice(&self.reserved);
        let off = off + 8;
        buf[off..off + 20].copy_from_slice(&self.info_hash.0);
        let off = off + 20;
        buf[off..off + 20].copy_from_slice(&self.peer_id);
        buf
    }

    pub fn decode(data: &[u8]) -> Result<Self, QvodError> {
        if data.len() < 2 {
            return Err(QvodError::Protocol("handshake too short".into()));
        }
        let pstrlen = data[0] as usize;
        let total_len = 1 + pstrlen + 8 + 20 + 20;
        if data.len() < total_len {
            return Err(QvodError::Protocol("handshake truncated".into()));
        }
        let pstr = &data[1..1 + pstrlen];
        if pstr != HANDSHAKE_PROTOCOL.as_bytes() {
            return Err(QvodError::Protocol(format!(
                "unexpected protocol: {}",
                String::from_utf8_lossy(pstr)
            )));
        }
        let mut reserved = [0u8; 8];
        reserved.copy_from_slice(&data[1 + pstrlen..1 + pstrlen + 8]);
        let mut info_hash = [0u8; 20];
        info_hash.copy_from_slice(&data[1 + pstrlen + 8..1 + pstrlen + 8 + 20]);
        let mut peer_id = [0u8; 20];
        peer_id.copy_from_slice(&data[1 + pstrlen + 8 + 20..1 + pstrlen + 8 + 20 + 20]);
        let supports_metadata = (reserved[5] & 0x10) != 0;
        Ok(Self {
            info_hash: InfoHash(info_hash),
            peer_id,
            reserved,
            supports_metadata,
        })
    }

    #[must_use]
    pub fn verify(&self) -> bool {
        self.reserved[5] & 0x10 == 0x10
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handshake_roundtrip() {
        let info_hash = InfoHash([0xABu8; 20]);
        let peer_id = [0x42u8; 20];
        let hs = Handshake::new(info_hash, peer_id);
        let encoded = hs.encode();
        assert_eq!(encoded.len(), 67);
        let decoded = Handshake::decode(&encoded).unwrap();
        assert_eq!(decoded.info_hash, info_hash);
        assert_eq!(decoded.peer_id, peer_id);
        assert!(decoded.supports_metadata);
    }

    #[test]
    fn test_handshake_too_short() {
        let result = Handshake::decode(&[0u8; 10]);
        assert!(result.is_err());
    }

    #[test]
    fn test_handshake_wrong_protocol() {
        let mut buf = [0u8; 68];
        buf[0] = 4;
        buf[1..5].copy_from_slice(b"TEST");
        let result = Handshake::decode(&buf);
        assert!(result.is_err());
    }

    #[test]
    fn test_handshake_verify() {
        let hs = Handshake::new(InfoHash([0u8; 20]), [0u8; 20]);
        assert!(hs.verify());
    }
}
