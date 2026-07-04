use std::time::Duration;

use qvs_core::{Bitfield, InfoHash};
use qvs_transport::congestion::UdpCongestionControl;
use qvs_transport::handshake::Handshake;
use qvs_transport::message::{MsgId, PeerMessage};
use qvs_transport::stats::PeerConnectionStats;

use qvs_tests::fixtures;

fn test_info_hash() -> InfoHash {
    InfoHash::from_bytes([0xAB; 20])
}

fn test_peer_id(seed: u8) -> [u8; 20] {
    let mut id = [0u8; 20];
    id[..8].copy_from_slice(b"-QV0001-");
    id[8..].fill(seed);
    id
}

#[test]
fn test_handshake_roundtrip() {
    let info_hash = test_info_hash();
    let peer_id = test_peer_id(0x42);
    let hs = Handshake::new(info_hash, peer_id);
    assert!(hs.supports_metadata);

    let encoded = hs.encode();
    assert_eq!(encoded.len(), 67);

    let decoded = Handshake::decode(&encoded).unwrap();
    assert_eq!(decoded.info_hash.0, info_hash.0);
    assert_eq!(decoded.peer_id, peer_id);
    assert!(decoded.supports_metadata);
    assert!(decoded.verify());
}

#[test]
fn test_handshake_rejects_wrong_protocol() {
    let info_hash = test_info_hash();
    let peer_id = test_peer_id(0xFF);
    let hs = Handshake::new(info_hash, peer_id);
    let mut encoded = hs.encode();
    encoded[1] = b'X';
    assert!(Handshake::decode(&encoded).is_err());
}

#[test]
fn test_handshake_verify_invalid() {
    let mut hs = Handshake::new(test_info_hash(), test_peer_id(0x01));
    hs.reserved = [0u8; 8];
    assert!(!hs.verify());
}

#[test]
fn test_message_roundtrip_all_types() {
    let test_cases: Vec<(PeerMessage, Box<dyn Fn(&PeerMessage) -> bool>)> = vec![
        (
            PeerMessage::new(MsgId::Choke, vec![]),
            Box::new(|m| m.msg_id == MsgId::Choke),
        ),
        (
            PeerMessage::new(MsgId::Unchoke, vec![]),
            Box::new(|m| m.msg_id == MsgId::Unchoke),
        ),
        (
            PeerMessage::new(MsgId::Interested, vec![]),
            Box::new(|m| m.msg_id == MsgId::Interested),
        ),
        (
            PeerMessage::new(MsgId::NotInterested, vec![]),
            Box::new(|m| m.msg_id == MsgId::NotInterested),
        ),
        (
            PeerMessage::have(42),
            Box::new(|m| m.parse_have() == Some(42)),
        ),
        (
            PeerMessage::bitfield(vec![0xFF, 0x00, 0xAA]),
            Box::new(|m| m.parse_bitfield() == Some(vec![0xFF, 0x00, 0xAA])),
        ),
        (
            PeerMessage::request(1, 0, 16384),
            Box::new(|m| m.parse_request() == Some((1, 0, 16384))),
        ),
        (
            PeerMessage::piece(5, 0, &[0xABu8; 1024]),
            Box::new(|m| m.parse_piece() == Some((5, 0, &[0xABu8; 1024][..]))),
        ),
        (
            PeerMessage::cancel(10, 4096, 8192),
            Box::new(|m| m.parse_cancel() == Some((10, 4096, 8192))),
        ),
        (
            PeerMessage::port(8621),
            Box::new(|m| m.parse_port() == Some(8621)),
        ),
        (
            PeerMessage::suggest_piece(77),
            Box::new(|m| m.parse_suggest_piece() == Some(77)),
        ),
        (
            PeerMessage::have_all(),
            Box::new(|m| m.msg_id == MsgId::HaveAll),
        ),
        (
            PeerMessage::have_none(),
            Box::new(|m| m.msg_id == MsgId::HaveNone),
        ),
        (
            PeerMessage::reject_request(3, 8192, 16384),
            Box::new(|m| m.msg_id == MsgId::RejectRequest),
        ),
        (
            PeerMessage::allowed_fast(99),
            Box::new(|m| m.msg_id == MsgId::AllowedFast),
        ),
    ];

    for (msg, validator) in &test_cases {
        let encoded = msg.encode();
        let decoded = PeerMessage::decode(&encoded).unwrap();
        assert_eq!(decoded.msg_id, msg.msg_id);
        assert!(
            validator(&decoded),
            "validation failed for {:?}",
            msg.msg_id
        );
    }
}

#[test]
fn test_request_piece_flow() {
    let index = 7u32;
    let begin = 0u32;
    let length = 16384u32;
    let data = vec![0x42u8; length as usize];

    let req = PeerMessage::request(index, begin, length);
    let encoded_req = req.encode();
    let decoded_req = PeerMessage::decode(&encoded_req).unwrap();
    let (d_idx, d_begin, d_len) = decoded_req.parse_request().unwrap();
    assert_eq!((d_idx, d_begin, d_len), (index, begin, length));

    let piece = PeerMessage::piece(index, begin, &data);
    let encoded_piece = piece.encode();
    let decoded_piece = PeerMessage::decode(&encoded_piece).unwrap();
    let (p_idx, p_begin, p_data) = decoded_piece.parse_piece().unwrap();
    assert_eq!(p_idx, index);
    assert_eq!(p_begin, begin);
    assert_eq!(p_data, &data[..]);
}

#[test]
fn test_bitfield_integration() {
    let num_pieces = 32;
    let mut bf = Bitfield::new(num_pieces);
    bf.set(0, true);
    bf.set(15, true);
    bf.set(31, true);

    let bf_bytes = bf.to_bytes().to_vec();
    let msg = PeerMessage::bitfield(bf_bytes.clone());

    let encoded = msg.encode();
    let decoded = PeerMessage::decode(&encoded).unwrap();
    let parsed = decoded.parse_bitfield().unwrap();
    assert_eq!(parsed, bf_bytes);

    let bf2 = Bitfield::from_bytes(parsed, num_pieces);
    assert!(bf2.has(0));
    assert!(bf2.has(15));
    assert!(bf2.has(31));
    assert!(!bf2.has(1));
    assert_eq!(bf2.count(), 3);
}

#[test]
fn test_keep_alive_is_invalid_message() {
    let ka = PeerMessage::keep_alive();
    assert_eq!(ka.len(), 4);
    assert!(PeerMessage::decode(&ka).is_err());
}

#[test]
fn test_truncated_messages() {
    let cases = vec![
        (vec![0u8; 3], "too short"),
        (vec![0, 0, 0, 10, 4], "payload truncated"),
        (vec![0, 0, 0, 1, 99], "unknown msg_id"),
    ];
    for (data, desc) in &cases {
        assert!(PeerMessage::decode(data).is_err(), "should reject: {desc}");
    }
}

#[test]
fn test_congestion_control_transitions() {
    let mut cc = UdpCongestionControl::new();

    assert!(cc.can_send());
    assert_eq!(cc.cwnd(), 2.0);

    for _ in 0..10 {
        cc.on_packet_sent();
        cc.on_ack(Duration::from_millis(50));
    }
    assert!(cc.cwnd() > 2.0);
}

#[test]
fn test_congestion_loss_event() {
    let mut cc = UdpCongestionControl::new();
    // Increase cwnd above 2.0 first, then test that loss reduces it
    for _ in 0..10 {
        cc.on_packet_sent();
        cc.on_ack(Duration::from_millis(50));
    }
    let cwnd_before = cc.cwnd();
    assert!(cwnd_before > 2.0);
    cc.on_loss();
    assert!(cc.cwnd() < cwnd_before);
}

#[test]
fn test_congestion_timeout_event() {
    let mut cc = UdpCongestionControl::new();
    cc.on_packet_sent();
    cc.on_timeout();
    assert_eq!(cc.cwnd(), 2.0);
}

#[test]
fn test_congestion_streaming_mode() {
    let mut cc = UdpCongestionControl::new();
    cc.on_packet_sent();
    for _ in 0..5 {
        cc.on_loss();
    }
    assert!(cc.is_streaming_mode());
}

#[test]
fn test_peer_connection_stats() {
    let stats = PeerConnectionStats::default();
    assert_eq!(stats.speed_down, 0.0);
    assert_eq!(stats.total_downloaded, 0);
}

#[test]
fn test_fixture_consistency() {
    let hash = fixtures::sample_info_hash();
    assert_eq!(hash.0[0], 0xAB);
    assert_eq!(hash.to_string().len(), 40);

    let qvs_bytes = fixtures::sample_qvs_bytes();
    assert!(!qvs_bytes.is_empty());

    let peers = fixtures::sample_peer_info();
    assert_eq!(peers.addr.port(), 8621);
}
