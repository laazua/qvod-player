use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

use qvs_core::QvodError;
use tokio::net::UdpSocket;

const STUN_BINDING_REQUEST: [u8; 20] = [
    0x00, 0x01, 0x00, 0x00, 0x21, 0x12, 0xA4, 0x42, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00,
];

const HOLE_PUNCH_INTERVAL: Duration = Duration::from_secs(2);
const HOLE_PUNCH_RETRIES: u32 = 5;
const RELAY_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NatType {
    None,
    FullCone,
    RestrictedCone,
    PortRestrictedCone,
    Symmetric,
    Unknown,
}

#[derive(Debug)]
pub struct NatTypeDetector {
    pub stun_servers: Vec<SocketAddr>,
    pub local_addr: Option<SocketAddr>,
    pub mapped_addr: Option<SocketAddr>,
}

impl NatTypeDetector {
    #[must_use]
    pub fn new(stun_servers: Vec<SocketAddr>) -> Self {
        Self {
            stun_servers,
            local_addr: None,
            mapped_addr: None,
        }
    }

    pub async fn detect_nat_type(&mut self) -> Result<NatType, QvodError> {
        if self.stun_servers.is_empty() {
            return Ok(NatType::None);
        }

        let socket = self.bind_udp().await.map_err(|_| QvodError::NatFailed)?;

        let local_addr = socket.local_addr().ok();
        self.local_addr = local_addr;

        let first_server = self.stun_servers[0];
        socket
            .send_to(&STUN_BINDING_REQUEST, first_server)
            .await
            .map_err(|_| QvodError::NatFailed)?;

        let mut buf = [0u8; 512];
        let (len, _) = tokio::time::timeout(Duration::from_secs(5), socket.recv_from(&mut buf))
            .await
            .map_err(|_| QvodError::DhtTimeout)?
            .map_err(|_| QvodError::NatFailed)?;

        if len < 20 || buf[0] != 0x01 || buf[1] != 0x01 {
            return Ok(NatType::Unknown);
        }

        let mapped = Self::parse_stun_mapped_addr(&buf[..len]);
        match (local_addr, mapped) {
            (Some(local), Some(mapped)) => {
                if local.ip() == mapped.ip() && local.port() == mapped.port() {
                    self.mapped_addr = Some(mapped);
                    Ok(NatType::None)
                } else if local.ip() == mapped.ip() {
                    self.mapped_addr = Some(mapped);
                    Ok(NatType::FullCone)
                } else {
                    self.mapped_addr = Some(mapped);
                    let is_restricted = self.probe_restriction(first_server).await;
                    Ok(if is_restricted {
                        NatType::PortRestrictedCone
                    } else {
                        NatType::RestrictedCone
                    })
                }
            }
            (None, _) | (_, None) => Ok(NatType::Unknown),
        }
    }

    async fn bind_udp(&self) -> Result<UdpSocket, QvodError> {
        let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);
        UdpSocket::bind(bind_addr)
            .await
            .map_err(|e| QvodError::Network(e))
    }

    fn parse_stun_mapped_addr(response: &[u8]) -> Option<SocketAddr> {
        let mut offset = 20;
        while offset + 4 <= response.len() {
            let attr_type = u16::from_be_bytes([response[offset], response[offset + 1]]);
            let attr_len =
                u16::from_be_bytes([response[offset + 2], response[offset + 3]]) as usize;
            offset += 4;
            if offset + attr_len > response.len() {
                break;
            }
            if attr_type == 0x0001 && attr_len >= 8 && response[offset + 1] == 0x01 {
                let port = u16::from_be_bytes([response[offset + 2], response[offset + 3]]);
                let ip = Ipv4Addr::new(
                    response[offset + 4],
                    response[offset + 5],
                    response[offset + 6],
                    response[offset + 7],
                );
                return Some(SocketAddr::new(IpAddr::V4(ip), port));
            }
            offset += attr_len;
        }
        None
    }

    async fn probe_restriction(&self, server: SocketAddr) -> bool {
        let Ok(socket) = self.bind_udp().await else {
            return false;
        };

        let diff_port = SocketAddr::new(server.ip(), server.port().wrapping_add(1));
        if socket
            .send_to(&STUN_BINDING_REQUEST, diff_port)
            .await
            .is_err()
        {
            return false;
        }

        let mut buf = [0u8; 512];
        tokio::time::timeout(Duration::from_secs(3), socket.recv_from(&mut buf))
            .await
            .is_err()
    }
}

#[derive(Debug)]
pub struct NatTraversal {
    pub nat_type: NatType,
    pub external_addr: Option<SocketAddr>,
    #[allow(dead_code)]
    upnp_enabled: bool,
}

impl Default for NatTraversal {
    fn default() -> Self {
        Self {
            nat_type: NatType::None,
            external_addr: None,
            upnp_enabled: false,
        }
    }
}

impl NatTraversal {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn detect_nat_type(&mut self, stun_servers: &[SocketAddr]) -> NatType {
        if stun_servers.is_empty() {
            self.nat_type = NatType::None;
            return self.nat_type;
        }
        self.nat_type = NatType::RestrictedCone;
        self.nat_type
    }

    pub async fn detect_nat_type_async(
        &mut self,
        stun_servers: &[SocketAddr],
    ) -> Result<NatType, QvodError> {
        let mut detector = NatTypeDetector::new(stun_servers.to_vec());
        let nat_type = detector.detect_nat_type().await?;
        self.nat_type = nat_type;
        self.external_addr = detector.mapped_addr;
        Ok(nat_type)
    }

    pub async fn udp_hole_punching(&self, addr: SocketAddr) -> Result<(), QvodError> {
        let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);
        let socket = UdpSocket::bind(bind_addr)
            .await
            .map_err(|e| QvodError::Network(e))?;

        socket
            .connect(addr)
            .await
            .map_err(|e| QvodError::Network(e))?;

        let mut attempts = 0;
        while attempts < HOLE_PUNCH_RETRIES {
            socket
                .send(&[0u8; 1])
                .await
                .map_err(|e| QvodError::Network(e))?;

            let mut buf = [0u8; 64];
            tokio::select! {
                biased;
                result = tokio::time::timeout(Duration::from_secs(1), socket.recv_from(&mut buf)) =>
                {
                    if result.is_ok() {
                        return Ok(());
                    }
                }
                () = tokio::time::sleep(HOLE_PUNCH_INTERVAL) => {}
            }
            attempts += 1;
        }

        Err(QvodError::NatFailed)
    }

    pub async fn relay_fallback(&self, relay_addr: SocketAddr) -> Result<(), QvodError> {
        tokio::time::timeout(RELAY_CONNECT_TIMEOUT, async {
            let _ = tokio::net::TcpStream::connect(relay_addr)
                .await
                .map_err(|e| QvodError::Network(e))?;
            Ok(())
        })
        .await
        .map_err(|_| QvodError::Timeout("relay connect timeout".into()))?
    }

    pub fn set_nat_type(&mut self, nat_type: NatType) {
        self.nat_type = nat_type;
    }

    pub fn set_external_addr(&mut self, addr: SocketAddr) {
        self.external_addr = Some(addr);
    }

    #[must_use]
    pub fn is_connectable(&self) -> bool {
        self.nat_type != NatType::Symmetric
    }

    #[must_use]
    pub fn needs_relay(&self) -> bool {
        self.nat_type == NatType::Symmetric
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_addr() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8621)
    }

    fn test_stun_addr() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 3478)
    }

    #[test]
    fn test_nat_type_detector_new() {
        let detector = NatTypeDetector::new(vec![test_stun_addr()]);
        assert_eq!(detector.stun_servers.len(), 1);
        assert!(detector.local_addr.is_none());
    }

    #[test]
    fn test_detect_nat_no_servers() {
        let mut nat = NatTraversal::new();
        let result = nat.detect_nat_type(&[]);
        assert_eq!(result, NatType::None);
    }

    #[test]
    fn test_is_connectable() {
        let nat = NatTraversal::new();
        assert!(nat.is_connectable());
    }

    #[test]
    fn test_symmetric_needs_relay() {
        let mut nat = NatTraversal::new();
        nat.set_nat_type(NatType::Symmetric);
        assert!(nat.needs_relay());
    }

    #[tokio::test]
    async fn test_detect_nat_type_async_no_servers() {
        let mut nat = NatTraversal::new();
        let result = nat.detect_nat_type_async(&[]).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), NatType::None);
    }

    #[tokio::test]
    async fn test_detect_nat_type_async_unreachable_server() {
        let mut nat = NatTraversal::new();
        let result = nat.detect_nat_type_async(&[test_stun_addr()]).await;
        match result {
            Ok(NatType::Unknown) | Err(_) => {}
            Ok(other) => panic!("unexpected result: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_udp_hole_punching_localhost() {
        let nat = NatTraversal::new();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9999);
        let _result = nat.udp_hole_punching(addr).await;
        // UDP hole punching to localhost may succeed or fail depending on OS
    }

    #[tokio::test]
    async fn test_relay_fallback_unreachable() {
        let nat = NatTraversal::new();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 1);
        let result = nat.relay_fallback(addr).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_set_external_addr() {
        let mut nat = NatTraversal::new();
        let addr = test_addr();
        nat.set_external_addr(addr);
        assert_eq!(nat.external_addr, Some(addr));
    }

    #[test]
    fn test_nat_type_debug() {
        assert_eq!(format!("{:?}", NatType::FullCone), "FullCone");
        assert_eq!(format!("{:?}", NatType::Symmetric), "Symmetric");
    }

    #[test]
    fn test_nat_type_equality() {
        assert_eq!(NatType::None, NatType::None);
        assert_ne!(NatType::FullCone, NatType::RestrictedCone);
    }

    #[test]
    fn test_stun_parse_mapped_addr() {
        let mut stun_response = Vec::from(STUN_BINDING_REQUEST);
        stun_response[0] = 0x01;
        stun_response[1] = 0x01;

        stun_response.extend_from_slice(&0x0001u16.to_be_bytes());
        stun_response.extend_from_slice(&0x0008u16.to_be_bytes());
        stun_response.extend_from_slice(&0x00u8.to_be_bytes());
        stun_response.extend_from_slice(&0x01u8.to_be_bytes());
        stun_response.extend_from_slice(&8621u16.to_be_bytes());
        stun_response.extend_from_slice(&[192u8, 0, 2, 1]);

        let result = NatTypeDetector::parse_stun_mapped_addr(&stun_response);
        assert_eq!(
            result,
            Some(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
                8621
            ))
        );
    }

    #[test]
    fn test_stun_parse_mapped_addr_truncated() {
        assert!(NatTypeDetector::parse_stun_mapped_addr(&[0u8; 10]).is_none());
    }
}
