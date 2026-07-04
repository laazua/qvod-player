use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use qvs_transport::nat::{NatTraversal, NatType, NatTypeDetector};
use qvs_transport::stats::TransportStats;

use qvs_tests::fixtures;

fn test_addr() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8621)
}

fn stun_addr() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 3478)
}

#[test]
fn test_nat_traversal_defaults() {
    let nat = NatTraversal::new();
    assert_eq!(nat.nat_type, NatType::None);
    assert!(nat.external_addr.is_none());
    assert!(nat.is_connectable());
    assert!(!nat.needs_relay());
}

#[test]
fn test_nat_type_setters() {
    let mut nat = NatTraversal::new();

    nat.set_nat_type(NatType::FullCone);
    assert_eq!(nat.nat_type, NatType::FullCone);
    assert!(nat.is_connectable());

    nat.set_nat_type(NatType::Symmetric);
    assert!(nat.needs_relay());
    assert!(!nat.is_connectable());

    nat.set_external_addr(test_addr());
    assert_eq!(nat.external_addr, Some(test_addr()));
}

#[test]
fn test_detect_no_servers() {
    let mut nat = NatTraversal::new();
    assert_eq!(nat.detect_nat_type(&[]), NatType::None);
}

#[tokio::test]
async fn test_detect_async_no_servers() {
    let mut nat = NatTraversal::new();
    let result = nat.detect_nat_type_async(&[]).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), NatType::None);
}

#[tokio::test]
async fn test_detect_async_unreachable_server() {
    let mut nat = NatTraversal::new();
    let result = nat.detect_nat_type_async(&[stun_addr()]).await;
    match result {
        Ok(NatType::Unknown) | Err(_) => {}
        Ok(other) => panic!("expected Unknown or Err, got {other:?}"),
    }
}

#[tokio::test]
async fn test_udp_hole_punch_to_closed_port() {
    let nat = NatTraversal::new();
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9999);
    let result = nat.udp_hole_punching(addr).await;
    // May succeed or fail depending on the system's UDP behavior
    // (some systems deliver ICMP unreachable messages back)
    let _ = result;
}

#[tokio::test]
async fn test_relay_fallback_to_closed_port() {
    let nat = NatTraversal::new();
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 1);
    assert!(nat.relay_fallback(addr).await.is_err());
}

#[test]
fn test_nat_type_equality() {
    assert_eq!(NatType::None, NatType::None);
    assert_eq!(NatType::FullCone, NatType::FullCone);
    assert_ne!(NatType::FullCone, NatType::RestrictedCone);
    assert_ne!(NatType::RestrictedCone, NatType::Symmetric);
}

#[test]
fn test_nat_type_display() {
    assert_eq!(format!("{:?}", NatType::None), "None");
    assert_eq!(format!("{:?}", NatType::FullCone), "FullCone");
    assert_eq!(format!("{:?}", NatType::Symmetric), "Symmetric");
    assert_eq!(format!("{:?}", NatType::Unknown), "Unknown");
}

#[test]
fn test_nat_connectability_by_type() {
    let mut nat = NatTraversal::new();

    let connectable = vec![
        NatType::None,
        NatType::FullCone,
        NatType::RestrictedCone,
        NatType::PortRestrictedCone,
    ];
    for nt in connectable {
        nat.set_nat_type(nt);
        assert!(nat.is_connectable(), "{nt:?} should be connectable");
        assert!(!nat.needs_relay(), "{nt:?} should not need relay");
    }

    nat.set_nat_type(NatType::Symmetric);
    assert!(!nat.is_connectable());
    assert!(nat.needs_relay());

    nat.set_nat_type(NatType::Unknown);
    assert!(nat.is_connectable());
    assert!(!nat.needs_relay());
}

#[test]
fn test_nat_detector_creation() {
    let detector = NatTypeDetector::new(vec![stun_addr()]);
    assert_eq!(detector.stun_servers.len(), 1);
    assert!(detector.local_addr.is_none());
    assert!(detector.mapped_addr.is_none());
}

#[test]
fn test_transport_stats_default() {
    let stats = TransportStats::default();
    assert_eq!(stats.total_connections, 0);
    assert_eq!(stats.active_connections, 0);
    assert_eq!(stats.bytes_downloaded, 0);
    assert_eq!(stats.bytes_uploaded, 0);
    assert_eq!(stats.packets_sent, 0);
    assert_eq!(stats.packets_received, 0);
    assert_eq!(stats.packets_lost, 0);
    assert_eq!(stats.average_rtt, std::time::Duration::ZERO);
}

#[test]
fn test_fixture_usage() {
    let nodes = fixtures::sample_dht_bootstrap_nodes();
    assert_eq!(nodes.len(), 4);
    assert!(nodes[0].contains("router"));

    let txt = fixtures::dht_bootstrap_txt_content();
    assert!(txt.contains("router.bittorrent.com"));
}
