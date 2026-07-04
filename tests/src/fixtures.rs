use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use qvs_core::{
    Bitfield, ConnectionStats, FileMeta, FrameType, InfoHash, KeyFrameEntry, KeyFrameIndex,
    PeerInfo,
};
use qvs_format::bencode::BencodeValue;
use qvs_format::qvs_file::QvsFile;
use qvs_format::uri::QvodUri;

pub fn sample_info_hash() -> InfoHash {
    InfoHash::from_bytes([
        0xAB, 0xCD, 0xEF, 0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0x01, 0x23, 0x45, 0x67,
        0x89, 0xAB, 0xCD, 0xEF, 0x01,
    ])
}

pub fn sample_peer_id() -> [u8; 20] {
    let mut id = [0u8; 20];
    id[..8].copy_from_slice(b"-QV0001-");
    id[8..].copy_from_slice(&[0xAA; 12]);
    id
}

pub fn sample_peer_info() -> PeerInfo {
    PeerInfo {
        peer_id: sample_peer_id(),
        addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), 8621),
        is_firewalled: false,
        bw_up: 1024000,
        bw_down: 5120000,
        location: Some("test".into()),
        latency: Duration::from_millis(50),
    }
}

pub fn sample_qvs_file() -> QvsFile {
    QvsFile {
        info_hash: sample_info_hash(),
        filename: "test_video.mp4".into(),
        file_size: 524_288,
        piece_length: 262_144,
        pieces: vec![[1u8; 20], [2u8; 20]],
        trackers: vec!["http://tracker.example.com:8621/announce".into()],
        keyframe_index: None,
    }
}

pub fn sample_qvs_bytes() -> Vec<u8> {
    sample_qvs_file().encode().unwrap()
}

pub fn sample_qvod_uri() -> QvodUri {
    QvodUri::new(
        sample_info_hash(),
        "test_video.mp4".into(),
        524_288,
        "mp4".into(),
    )
}

pub fn sample_uri_string() -> String {
    "qvod://abcdef0123456789abcdef0123456789abcdef01|test_video.mp4|524288|mp4|".into()
}

pub fn sample_bitfield(num_pieces: u32) -> Bitfield {
    let mut bf = Bitfield::new(num_pieces);
    for i in 0..num_pieces.min(10) {
        bf.set(i, true);
    }
    bf
}

pub fn sample_file_meta() -> FileMeta {
    let kfi = KeyFrameIndex {
        entries: vec![
            KeyFrameEntry {
                timestamp_ms: 0,
                file_offset: 0,
                frame_size: 262_144,
                frame_type: FrameType::I,
            },
            KeyFrameEntry {
                timestamp_ms: 5000,
                file_offset: 262_144,
                frame_size: 131_072,
                frame_type: FrameType::I,
            },
            KeyFrameEntry {
                timestamp_ms: 10000,
                file_offset: 393_216,
                frame_size: 131_072,
                frame_type: FrameType::P,
            },
        ],
    };

    FileMeta {
        info_hash: sample_info_hash(),
        filename: "test_video.mp4".into(),
        file_size: 524_288,
        piece_length: 262_144,
        pieces: vec![[1u8; 20], [2u8; 20]],
        keyframe_index: Some(kfi),
        duration_ms: 10_000,
        video_codec: Some("h264".into()),
        audio_codec: Some("aac".into()),
        width: 1280,
        height: 720,
        bitrate: 500_000,
        from_cache: false,
    }
}

pub fn sample_connection_stats() -> ConnectionStats {
    ConnectionStats {
        speed_down: 1_000_000.0,
        speed_up: 200_000.0,
        rtt: Duration::from_millis(50),
        loss_rate: 0.01,
        total_downloaded: 262_144,
        total_uploaded: 65_536,
    }
}

pub fn sample_bencode_tracker_response() -> Vec<u8> {
    let mut dict = BTreeMap::new();
    dict.insert(b"interval".to_vec(), BencodeValue::Int(1800));
    dict.insert(b"complete".to_vec(), BencodeValue::Int(10));
    dict.insert(b"incomplete".to_vec(), BencodeValue::Int(5));
    dict.insert(b"downloaded".to_vec(), BencodeValue::Int(100));
    let compact_peers: Vec<u8> = vec![
        192, 168, 1, 1, 0x21, 0xAD, 10, 0, 0, 1, 0x1A, 0xE1, 172, 16, 0, 1, 0x1F, 0x90,
    ];
    dict.insert(b"peers".to_vec(), BencodeValue::Str(compact_peers));
    BencodeValue::Dict(dict).encode()
}

pub fn sample_dht_bootstrap_nodes() -> Vec<String> {
    vec![
        "router.bittorrent.com:6881".into(),
        "router.utorrent.com:6881".into(),
        "dht.transmissionbt.com:6881".into(),
        "dht.aelitis.com:6881".into(),
    ]
}

pub fn dht_bootstrap_txt_content() -> &'static str {
    "router.bittorrent.com:6881\nrouter.utorrent.com:6881\ndht.transmissionbt.com:6881\ndht.aelitis.com:6881\n"
}
