use qvs_core::QvodError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    Initializing,
    Buffering,
    Playing,
    Paused,
    Seeking,
    Ended,
    Error,
}

#[derive(Debug, Clone)]
pub struct StreamStats {
    pub position_ms: u64,
    pub duration_ms: u64,
    pub speed_bps: f64,
    pub buffered_seconds: f64,
    pub peer_count: usize,
    pub state: StreamState,
    pub download_progress: f64,
    pub bytes_downloaded: u64,
}

impl StreamStats {
    #[must_use]
    pub fn new(duration_ms: u64) -> Self {
        Self {
            position_ms: 0,
            duration_ms,
            speed_bps: 0.0,
            buffered_seconds: 0.0,
            peer_count: 0,
            state: StreamState::Initializing,
            download_progress: 0.0,
            bytes_downloaded: 0,
        }
    }
}

pub struct MediaStream {
    stats: StreamStats,
    paused: bool,
    seek_target: Option<u64>,
}

impl MediaStream {
    #[must_use]
    pub fn new(stats: StreamStats) -> Self {
        tracing::debug!("MediaStream::new: duration={}ms", stats.duration_ms);
        Self {
            stats,
            paused: true,
            seek_target: None,
        }
    }

    pub fn play(&mut self) -> Result<(), QvodError> {
        tracing::info!("MediaStream::play: state {:?} -> Playing", self.stats.state);
        self.paused = false;
        self.stats.state = StreamState::Playing;
        Ok(())
    }

    pub fn pause(&mut self) {
        tracing::info!("MediaStream::pause: state {:?} -> Paused", self.stats.state);
        self.paused = true;
        self.stats.state = StreamState::Paused;
    }

    pub fn resume(&mut self) {
        tracing::info!(
            "MediaStream::resume: state {:?} -> Playing",
            self.stats.state
        );
        self.paused = false;
        self.stats.state = StreamState::Playing;
    }

    pub fn seek(&mut self, timestamp_ms: u64) {
        tracing::info!("MediaStream::seek: -> {}ms", timestamp_ms);
        self.seek_target = Some(timestamp_ms);
        self.stats.state = StreamState::Seeking;
        self.stats.position_ms = timestamp_ms;
    }

    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    #[must_use]
    pub fn state(&self) -> StreamState {
        self.stats.state
    }

    #[must_use]
    pub fn stats(&self) -> &StreamStats {
        &self.stats
    }

    pub fn stats_mut(&mut self) -> &mut StreamStats {
        &mut self.stats
    }

    pub fn end(&mut self) {
        tracing::info!("MediaStream::end: state {:?} -> Ended", self.stats.state);
        self.stats.state = StreamState::Ended;
    }

    pub fn update_position(&mut self, position_ms: u64) {
        if self.stats.state == StreamState::Seeking {
            tracing::info!(
                "MediaStream::update_position: seek complete at {}ms",
                position_ms
            );
            self.stats.state = StreamState::Playing;
            self.seek_target = None;
        }
        self.stats.position_ms = position_ms;
    }

    pub fn update_speed(&mut self, speed_bps: f64) {
        if (self.stats.speed_bps - speed_bps).abs() > self.stats.speed_bps * 0.5 {
            tracing::debug!(
                "MediaStream::update_speed: {:.0} -> {:.0} B/s",
                self.stats.speed_bps,
                speed_bps
            );
        }
        self.stats.speed_bps = speed_bps;
    }

    pub fn update_buffered(&mut self, buffered_seconds: f64) {
        self.stats.buffered_seconds = buffered_seconds;
    }

    pub fn update_peers(&mut self, count: usize) {
        self.stats.peer_count = count;
    }

    pub fn update_progress(&mut self, progress: f64, bytes: u64) {
        if (self.stats.download_progress - progress).abs() > 0.05 {
            tracing::debug!(
                "MediaStream::update_progress: {:.1}% -> {:.1}%, {} bytes",
                self.stats.download_progress * 100.0,
                progress * 100.0,
                bytes
            );
        }
        self.stats.download_progress = progress;
        self.stats.bytes_downloaded = bytes;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let stream = MediaStream::new(StreamStats::new(10000));
        assert_eq!(stream.state(), StreamState::Initializing);
        assert!(stream.is_paused());
    }

    #[test]
    fn test_play_pause() {
        let mut stream = MediaStream::new(StreamStats::new(10000));
        stream.play().unwrap();
        assert_eq!(stream.state(), StreamState::Playing);
        assert!(!stream.is_paused());
        stream.pause();
        assert_eq!(stream.state(), StreamState::Paused);
        assert!(stream.is_paused());
    }

    #[test]
    fn test_seek() {
        let mut stream = MediaStream::new(StreamStats::new(10000));
        stream.seek(5000);
        assert_eq!(stream.state(), StreamState::Seeking);
        assert_eq!(stream.stats().position_ms, 5000);
        stream.update_position(5000);
        assert_eq!(stream.state(), StreamState::Playing);
    }

    #[test]
    fn test_end() {
        let mut stream = MediaStream::new(StreamStats::new(10000));
        stream.end();
        assert_eq!(stream.state(), StreamState::Ended);
    }

    #[test]
    fn test_stats_update() {
        let mut stream = MediaStream::new(StreamStats::new(10000));
        stream.update_speed(1_000_000.0);
        assert_eq!(stream.stats().speed_bps, 1_000_000.0);
        stream.update_buffered(15.0);
        assert_eq!(stream.stats().buffered_seconds, 15.0);
        stream.update_peers(5);
        assert_eq!(stream.stats().peer_count, 5);
        stream.update_progress(0.5, 1024);
        assert_eq!(stream.stats().download_progress, 0.5);
        assert_eq!(stream.stats().bytes_downloaded, 1024);
    }
}
