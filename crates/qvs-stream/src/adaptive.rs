use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferCommand {
    PauseAndBuffer,
    ThrottleUpload,
    Normal,
    IncreaseHttpRatio,
}

#[derive(Debug)]
pub struct AdaptiveBuffer {
    state: BufferCommand,
    speed_samples: Vec<f64>,
    rtt_samples: Vec<Duration>,
    last_tick: Instant,
}

impl AdaptiveBuffer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: BufferCommand::Normal,
            speed_samples: Vec::with_capacity(100),
            rtt_samples: Vec::with_capacity(100),
            last_tick: Instant::now(),
        }
    }

    pub fn tick(&mut self, speed_bps: f64, rtt: Duration, buffered_seconds: f64) -> BufferCommand {
        self.speed_samples.push(speed_bps);
        if self.speed_samples.len() > 100 {
            self.speed_samples.remove(0);
        }
        self.rtt_samples.push(rtt);
        if self.rtt_samples.len() > 100 {
            self.rtt_samples.remove(0);
        }
        self.last_tick = Instant::now();

        if buffered_seconds < 2.0 && speed_bps < 100_000.0 {
            self.state = BufferCommand::PauseAndBuffer;
        } else if buffered_seconds < 5.0 {
            self.state = BufferCommand::Normal;
        } else if buffered_seconds > 30.0 {
            self.state = BufferCommand::ThrottleUpload;
        } else {
            self.state = BufferCommand::Normal;
        }

        if rtt > Duration::from_millis(500) && speed_bps < 200_000.0 {
            self.state = BufferCommand::IncreaseHttpRatio;
        }

        self.state
    }

    #[must_use]
    pub fn avg_speed(&self) -> f64 {
        if self.speed_samples.is_empty() {
            return 0.0;
        }
        self.speed_samples.iter().sum::<f64>() / self.speed_samples.len() as f64
    }

    #[must_use]
    pub fn avg_rtt(&self) -> Duration {
        if self.rtt_samples.is_empty() {
            return Duration::from_millis(100);
        }
        let sum: Duration = self.rtt_samples.iter().copied().sum();
        sum / self.rtt_samples.len() as u32
    }

    pub fn reset(&mut self) {
        self.speed_samples.clear();
        self.rtt_samples.clear();
        self.state = BufferCommand::Normal;
        self.last_tick = Instant::now();
    }
}

impl Default for AdaptiveBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let ab = AdaptiveBuffer::new();
        assert_eq!(ab.avg_speed(), 0.0);
    }

    #[test]
    fn test_normal_playback() {
        let mut ab = AdaptiveBuffer::new();
        let cmd = ab.tick(1_000_000.0, Duration::from_millis(50), 10.0);
        assert_eq!(cmd, BufferCommand::Normal);
    }

    #[test]
    fn test_pause_on_low_buffer() {
        let mut ab = AdaptiveBuffer::new();
        let cmd = ab.tick(50_000.0, Duration::from_millis(100), 1.0);
        assert_eq!(cmd, BufferCommand::PauseAndBuffer);
    }

    #[test]
    fn test_increase_http_on_high_rtt() {
        let mut ab = AdaptiveBuffer::new();
        let cmd = ab.tick(100_000.0, Duration::from_millis(600), 5.0);
        assert_eq!(cmd, BufferCommand::IncreaseHttpRatio);
    }

    #[test]
    fn test_throttle_on_full_buffer() {
        let mut ab = AdaptiveBuffer::new();
        let cmd = ab.tick(1_000_000.0, Duration::from_millis(50), 35.0);
        assert_eq!(cmd, BufferCommand::ThrottleUpload);
    }

    #[test]
    fn test_reset() {
        let mut ab = AdaptiveBuffer::new();
        ab.tick(1_000_000.0, Duration::from_millis(50), 10.0);
        ab.reset();
        assert_eq!(ab.avg_speed(), 0.0);
    }

    #[test]
    fn test_avg_rtt() {
        let mut ab = AdaptiveBuffer::new();
        ab.tick(1_000_000.0, Duration::from_millis(100), 10.0);
        ab.tick(1_000_000.0, Duration::from_millis(200), 10.0);
        let avg = ab.avg_rtt();
        assert!(avg >= Duration::from_millis(100));
        assert!(avg <= Duration::from_millis(200));
    }
}
