use std::time::Duration;

use qvs_core::{FileMeta, FrameType, InfoHash, KeyFrameEntry, KeyFrameIndex, MediaStream};
use qvs_stream::adaptive::{AdaptiveBuffer, BufferCommand};
use qvs_stream::buffer::RingBuffer;
use qvs_stream::config::EngineConfig;
use qvs_stream::engine::QvodEngine;
use qvs_stream::hls::HlsAdapter;
use qvs_stream::playback::{MediaStream as StreamCtrl, StreamState, StreamStats};
use qvs_stream::seek::SeekEngine;

use qvs_tests::fixtures;

fn sample_meta() -> FileMeta {
    let kfi = KeyFrameIndex {
        entries: vec![
            KeyFrameEntry {
                timestamp_ms: 0,
                file_offset: 0,
                frame_size: 262144,
                frame_type: FrameType::I,
            },
            KeyFrameEntry {
                timestamp_ms: 5000,
                file_offset: 262144,
                frame_size: 131072,
                frame_type: FrameType::I,
            },
            KeyFrameEntry {
                timestamp_ms: 10000,
                file_offset: 393216,
                frame_size: 131072,
                frame_type: FrameType::P,
            },
        ],
    };
    FileMeta {
        info_hash: InfoHash::from_bytes([0x11; 20]),
        filename: "test.mp4".into(),
        file_size: 524288,
        piece_length: 262144,
        pieces: vec![[0xAA; 20], [0xBB; 20]],
        keyframe_index: Some(kfi),
        duration_ms: 10000,
        video_codec: Some("h264".into()),
        audio_codec: Some("aac".into()),
        width: 1280,
        height: 720,
        bitrate: 500000,
        from_cache: false,
    }
}

#[test]
fn test_ringbuffer_write_and_read() {
    let mut buf = RingBuffer::new(1024 * 1024, 524288);
    assert!(!buf.is_playable());
    assert_eq!(buf.filled_percentage(), 0.0);

    buf.write(0, &vec![0xABu8; 262144]);
    assert!(buf.is_playable());

    let read = buf.read(0, 100).unwrap();
    assert_eq!(read, vec![0xABu8; 100]);

    assert!(buf.read(262144, 100).is_none());

    buf.write(262144, &vec![0xCDu8; 131072]);
    assert!(buf.read(262144, 100).is_some());
    assert_eq!(buf.read(262144, 100).unwrap(), vec![0xCDu8; 100]);
}

#[test]
fn test_ringbuffer_buffered_duration() {
    let mut buf = RingBuffer::new(1024 * 1024, 524288);
    buf.write(0, &vec![0u8; 262144]);
    let dur = buf.buffered_duration(10000);
    assert!(dur > Duration::ZERO);
    assert!(dur <= Duration::from_secs(10));
}

#[test]
fn test_ringbuffer_clear() {
    let mut buf = RingBuffer::new(1024 * 1024, 524288);
    buf.write(0, &vec![0xABu8; 262144]);
    assert!(buf.is_playable());
    buf.clear();
    assert!(!buf.is_playable());
    assert_eq!(buf.filled_percentage(), 0.0);
}

#[test]
fn test_ringbuffer_non_sequential_write() {
    let mut buf = RingBuffer::new(1024 * 1024, 524288);
    buf.write(400000, &vec![0xFFu8; 1000]);
    assert!(!buf.is_playable());
    assert!(buf.read(400000, 1000).is_some());
}

#[test]
fn test_ringbuffer_write_past_total() {
    let mut buf = RingBuffer::new(1024, 5000);
    buf.write(5000, &[0xFFu8; 100]);
    assert!(buf.read(5000, 100).is_none());
    assert_eq!(buf.filled_percentage(), 0.0);
}

#[test]
fn test_ringbuffer_set_play_position() {
    let mut buf = RingBuffer::new(1024 * 1024, 524288);
    buf.set_play_position(262144);
    assert!(buf.buffered_duration(10000) <= Duration::from_secs(10));
}

#[test]
fn test_seek_engine_keyframe_lookup() {
    let engine = SeekEngine::new(sample_meta());
    let offset = engine.find_nearest_keyframe(3000).unwrap();
    assert_eq!(offset, 0);

    let offset = engine.find_nearest_keyframe(7000).unwrap();
    assert_eq!(offset, 262144);

    assert_eq!(engine.piece_for_offset(0), 0);
    assert_eq!(engine.piece_for_offset(262144), 1);
}

#[test]
fn test_seek_engine_no_keyframe_index() {
    let mut meta = sample_meta();
    meta.keyframe_index = None;
    let engine = SeekEngine::new(meta);
    assert!(engine.find_nearest_keyframe(5000).is_err());
}

#[test]
fn test_hls_m3u8_generation() {
    let adapter = HlsAdapter::new(sample_meta());
    let playlist = adapter.generate_m3u8().unwrap();
    assert!(playlist.starts_with("#EXTM3U"));
    assert!(playlist.contains("#EXT-X-VERSION:3"));
    assert!(playlist.contains("/segment?offset=0"));
    assert!(playlist.contains("/segment?offset=262144"));
    assert_eq!(adapter.segment_count(), 2);
}

#[test]
fn test_hls_segment_info() {
    let adapter = HlsAdapter::new(sample_meta());
    let (offset, length) = adapter.segment_info(0).unwrap();
    assert_eq!(offset, 0);
    assert_eq!(length, 262144);

    let (offset, _) = adapter.segment_info(1).unwrap();
    assert_eq!(offset, 262144);
}

#[test]
fn test_hls_wrap_as_ts() {
    let adapter = HlsAdapter::new(sample_meta());
    let data = vec![0x47u8; 188];
    assert_eq!(adapter.wrap_as_ts(&data), data);
}

#[test]
fn test_adaptive_buffer_state_transitions() {
    let mut ab = AdaptiveBuffer::new();

    let cmd = ab.tick(1_000_000.0, Duration::from_millis(50), 10.0);
    assert_eq!(cmd, BufferCommand::Normal);

    let cmd = ab.tick(50_000.0, Duration::from_millis(100), 1.0);
    assert_eq!(cmd, BufferCommand::PauseAndBuffer);

    let cmd = ab.tick(1_000_000.0, Duration::from_millis(50), 35.0);
    assert_eq!(cmd, BufferCommand::ThrottleUpload);

    let cmd = ab.tick(100_000.0, Duration::from_millis(600), 5.0);
    assert_eq!(cmd, BufferCommand::IncreaseHttpRatio);
}

#[test]
fn test_adaptive_buffer_averages() {
    let mut ab = AdaptiveBuffer::new();
    assert_eq!(ab.avg_speed(), 0.0);

    ab.tick(1_000_000.0, Duration::from_millis(100), 10.0);
    ab.tick(2_000_000.0, Duration::from_millis(200), 10.0);
    assert!(ab.avg_speed() > 1_000_000.0);
    assert!(ab.avg_speed() < 2_000_000.0);
}

#[test]
fn test_adaptive_buffer_reset() {
    let mut ab = AdaptiveBuffer::new();
    ab.tick(1_000_000.0, Duration::from_millis(50), 10.0);
    ab.reset();
    assert_eq!(ab.avg_speed(), 0.0);
}

#[test]
fn test_media_stream_lifecycle() {
    let stats = StreamStats::new(10000);
    let mut stream = StreamCtrl::new(stats);

    assert_eq!(stream.state(), StreamState::Initializing);
    assert!(stream.is_paused());

    stream.play().unwrap();
    assert_eq!(stream.state(), StreamState::Playing);
    assert!(!stream.is_paused());

    stream.pause();
    assert_eq!(stream.state(), StreamState::Paused);

    stream.resume();
    assert_eq!(stream.state(), StreamState::Playing);

    stream.seek(5000);
    assert_eq!(stream.state(), StreamState::Seeking);
    assert_eq!(stream.stats().position_ms, 5000);

    stream.update_position(5000);
    assert_eq!(stream.state(), StreamState::Playing);

    stream.end();
    assert_eq!(stream.state(), StreamState::Ended);
}

#[test]
fn test_media_stream_stats_updates() {
    let mut stream = StreamCtrl::new(StreamStats::new(20000));
    stream.play().unwrap();
    stream.update_speed(2_000_000.0);
    stream.update_buffered(15.5);
    stream.update_peers(8);
    stream.update_progress(0.75, 393216);

    assert_eq!(stream.stats().speed_bps, 2_000_000.0);
    assert!((stream.stats().buffered_seconds - 15.5).abs() < 0.001);
    assert_eq!(stream.stats().peer_count, 8);
    assert!((stream.stats().download_progress - 0.75).abs() < 0.001);
    assert_eq!(stream.stats().bytes_downloaded, 393216);
}

#[tokio::test]
async fn test_engine_play_stop() {
    let config = EngineConfig {
        dht_enabled: false,
        tracker_enabled: false,
        cache_enabled: false,
        ..Default::default()
    };
    let mut engine = QvodEngine::new(config).await;

    let uri = "qvod://1111111111111111111111111111111111111111|test.mp4|524288|mp4|";
    let result = engine.play(uri).await;
    assert!(result.is_ok());
    assert_eq!(engine.active_streams().len(), 1);

    let ih = InfoHash::from_bytes([0x11; 20]);
    assert!(engine.status(&ih).await.is_some());
    assert_eq!(engine.file_size(&ih), Some(524288));

    engine.stop(&ih);
    assert_eq!(engine.active_streams().len(), 0);
}

#[tokio::test]
async fn test_engine_pause_resume() {
    let config = EngineConfig {
        dht_enabled: false,
        tracker_enabled: false,
        cache_enabled: false,
        ..Default::default()
    };
    let mut engine = QvodEngine::new(config).await;

    let uri = "qvod://2222222222222222222222222222222222222222|test.mp4|524288|mp4|";
    let _ = engine.play(uri).await;
    engine.pause().await;
    engine.resume().await;
}

#[tokio::test]
async fn test_engine_seek_without_keyframes() {
    let config = EngineConfig {
        dht_enabled: false,
        tracker_enabled: false,
        cache_enabled: false,
        ..Default::default()
    };
    let mut engine = QvodEngine::new(config).await;

    let uri = "qvod://3333333333333333333333333333333333333333|test.mp4|524288|mp4|";
    let _ = engine.play(uri).await;
    // Seek succeeds even without keyframes (graceful fallback)
    assert!(engine.seek(5000).await.is_ok());
}

#[tokio::test]
async fn test_engine_invalid_uri() {
    let config = EngineConfig {
        dht_enabled: false,
        tracker_enabled: false,
        cache_enabled: false,
        ..Default::default()
    };
    let mut engine = QvodEngine::new(config).await;
    assert!(engine.play("invalid://uri").await.is_err());
}

#[test]
fn test_media_stream_from_fixture() {
    let meta = fixtures::sample_file_meta();
    let ms = MediaStream::new(meta);
    assert_eq!(ms.num_pieces(), 2);
}

#[test]
fn test_engine_config_defaults() {
    let config = EngineConfig::default();
    assert_eq!(config.listen_port, 8621);
    assert!(config.http_fallback);
    assert_eq!(config.buffer_capacity(), 64 * 1024 * 1024);
}
