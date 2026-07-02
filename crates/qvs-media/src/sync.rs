#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyncStrategy {
    #[default]
    AudioMaster,
    VideoMaster,
    ExternalMaster,
}

#[derive(Debug)]
pub struct AudioVideoSync {
    strategy: SyncStrategy,
    last_video_pts: u64,
    last_audio_pts: u64,
    drift_ms: i64,
    dropped_frames: u64,
}

impl AudioVideoSync {
    #[must_use]
    pub fn new() -> Self {
        Self {
            strategy: SyncStrategy::AudioMaster,
            last_video_pts: 0,
            last_audio_pts: 0,
            drift_ms: 0,
            dropped_frames: 0,
        }
    }

    pub fn set_strategy(&mut self, strategy: SyncStrategy) {
        self.strategy = strategy;
    }

    pub fn sync_video(&mut self, video_pts_ms: u64, audio_pts_ms: u64) -> SyncAction {
        self.last_video_pts = video_pts_ms;
        self.last_audio_pts = audio_pts_ms;

        let diff = video_pts_ms as i64 - audio_pts_ms as i64;
        self.drift_ms = diff;

        match self.strategy {
            SyncStrategy::AudioMaster => {
                if diff > 20 {
                    SyncAction::DropFrame
                } else if diff < -100 {
                    SyncAction::RepeatFrame
                } else {
                    SyncAction::Render
                }
            }
            SyncStrategy::VideoMaster => SyncAction::Render,
            SyncStrategy::ExternalMaster => {
                if diff.abs() > 50 {
                    SyncAction::DropFrame
                } else {
                    SyncAction::Render
                }
            }
        }
    }

    pub fn sync_audio(&mut self, _audio_pts_ms: u64) -> SyncAction {
        SyncAction::Render
    }

    #[must_use]
    pub fn drift_ms(&self) -> i64 {
        self.drift_ms
    }

    #[must_use]
    pub fn dropped_frames(&self) -> u64 {
        self.dropped_frames
    }

    pub fn reset(&mut self) {
        self.last_video_pts = 0;
        self.last_audio_pts = 0;
        self.drift_ms = 0;
        self.dropped_frames = 0;
    }
}

impl Default for AudioVideoSync {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncAction {
    Render,
    DropFrame,
    RepeatFrame,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let sync = AudioVideoSync::new();
        assert_eq!(sync.drift_ms(), 0);
        assert_eq!(sync.dropped_frames(), 0);
    }

    #[test]
    fn test_in_sync() {
        let mut sync = AudioVideoSync::new();
        let action = sync.sync_video(1000, 1000);
        assert_eq!(action, SyncAction::Render);
    }

    #[test]
    fn test_video_too_far_ahead() {
        let mut sync = AudioVideoSync::new();
        let action = sync.sync_video(1100, 1000);
        // diff = 100ms > 20ms → DropFrame
        assert_eq!(action, SyncAction::DropFrame);
    }

    #[test]
    fn test_video_slightly_ahead() {
        let mut sync = AudioVideoSync::new();
        let action = sync.sync_video(1010, 1000);
        assert_eq!(action, SyncAction::Render);
    }

    #[test]
    fn test_video_behind() {
        let mut sync = AudioVideoSync::new();
        let action = sync.sync_video(800, 1000);
        assert_eq!(action, SyncAction::RepeatFrame);
    }

    #[test]
    fn test_strategy_change() {
        let mut sync = AudioVideoSync::new();
        sync.set_strategy(SyncStrategy::VideoMaster);
        let action = sync.sync_video(1100, 1000);
        assert_eq!(action, SyncAction::Render);
    }

    #[test]
    fn test_reset() {
        let mut sync = AudioVideoSync::new();
        sync.sync_video(1100, 1000);
        assert!(sync.drift_ms() > 0);
        sync.reset();
        assert_eq!(sync.drift_ms(), 0);
    }
}
