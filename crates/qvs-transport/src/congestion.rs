use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CongestionState {
    SlowStart,
    CongestionAvoidance,
    FastRecovery,
}

#[derive(Debug)]
pub struct UdpCongestionControl {
    state: CongestionState,
    cwnd: f64,
    ssthresh: f64,
    max_cwnd: f64,
    rtt_estimate: Duration,
    rtt_dev: Duration,
    loss_rate: f64,
    packets_in_flight: u32,
    packets_lost: u32,
    packets_sent: u32,
    srtt_history: VecDeque<Duration>,
    last_ack_time: Option<Instant>,
    streaming_mode: bool,
}

impl UdpCongestionControl {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: CongestionState::SlowStart,
            cwnd: 2.0,
            ssthresh: 64.0,
            max_cwnd: 256.0,
            rtt_estimate: Duration::from_millis(100),
            rtt_dev: Duration::from_millis(20),
            loss_rate: 0.0,
            packets_in_flight: 0,
            packets_lost: 0,
            packets_sent: 0,
            srtt_history: VecDeque::with_capacity(100),
            last_ack_time: None,
            streaming_mode: false,
        }
    }

    #[must_use]
    pub fn state(&self) -> CongestionState {
        self.state
    }

    #[must_use]
    pub fn cwnd(&self) -> f64 {
        self.cwnd
    }

    #[must_use]
    pub fn ssthresh(&self) -> f64 {
        self.ssthresh
    }

    pub fn on_ack(&mut self, rtt: Duration) {
        self.update_rtt(rtt);
        self.packets_in_flight = self.packets_in_flight.saturating_sub(1);

        match self.state {
            CongestionState::SlowStart | CongestionState::FastRecovery => {
                self.cwnd += 1.0;
                if self.cwnd >= self.ssthresh {
                    self.state = CongestionState::CongestionAvoidance;
                }
            }
            CongestionState::CongestionAvoidance => {
                self.cwnd += 1.0 / self.cwnd;
            }
        }
        self.cwnd = self.cwnd.min(self.max_cwnd);
        self.last_ack_time = Some(Instant::now());
    }

    pub fn on_loss(&mut self) {
        self.ssthresh = (self.cwnd / 2.0).max(2.0);
        self.cwnd = self.ssthresh;
        self.packets_in_flight = self.packets_in_flight.saturating_sub(1);
        self.packets_lost += 1;
        self.state = CongestionState::FastRecovery;
        self.update_loss_rate();
    }

    pub fn on_timeout(&mut self) {
        self.ssthresh = (self.cwnd / 2.0).max(2.0);
        self.cwnd = 2.0;
        self.packets_in_flight = 0;
        self.packets_lost += 1;
        self.state = CongestionState::SlowStart;
        self.update_loss_rate();
    }

    pub fn on_packet_sent(&mut self) {
        self.packets_sent += 1;
        self.packets_in_flight += 1;
        self.update_loss_rate();
    }

    #[must_use]
    pub fn can_send(&self) -> bool {
        self.packets_in_flight < self.cwnd as u32
    }

    #[must_use]
    pub fn wait_time(&self) -> Duration {
        let rtt = self.rtt_estimate;
        let window = self.cwnd;
        if window <= 0.0 {
            return Duration::from_millis(10);
        }
        let interval = rtt.div_f64(window);
        let clamped = interval
            .max(Duration::from_millis(1))
            .min(Duration::from_millis(50));
        if self.streaming_mode {
            clamped.max(Duration::from_millis(5))
        } else {
            clamped
        }
    }

    #[must_use]
    pub fn rtt(&self) -> Duration {
        self.rtt_estimate
    }

    #[must_use]
    pub fn loss_rate(&self) -> f64 {
        self.loss_rate
    }

    fn update_loss_rate(&mut self) {
        let total = self.packets_sent;
        if total > 0 {
            self.loss_rate = f64::from(self.packets_lost) / f64::from(total);
            if self.loss_rate > 0.10 {
                self.streaming_mode = true;
            }
        }
    }

    fn update_rtt(&mut self, rtt: Duration) {
        self.srtt_history.push_back(rtt);
        if self.srtt_history.len() > 100 {
            self.srtt_history.pop_front();
        }
        let n = self.srtt_history.len() as u32;
        if n > 0 {
            let sum: Duration = self.srtt_history.iter().copied().sum();
            self.rtt_estimate = sum / n;
        }
        let abs_diff = if rtt > self.rtt_estimate {
            rtt.checked_sub(self.rtt_estimate).unwrap_or_default()
        } else {
            self.rtt_estimate.checked_sub(rtt).unwrap_or_default()
        };
        self.rtt_dev = (self.rtt_dev * 3 + abs_diff) / 4;
    }

    #[must_use]
    pub fn is_streaming_mode(&self) -> bool {
        self.streaming_mode
    }

    pub fn set_streaming_mode(&mut self, enabled: bool) {
        self.streaming_mode = enabled;
    }

    #[must_use]
    pub fn timeout_duration(&self) -> Duration {
        self.rtt_estimate + self.rtt_dev * 4
    }
}

impl Default for UdpCongestionControl {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let cc = UdpCongestionControl::new();
        assert_eq!(cc.state(), CongestionState::SlowStart);
        assert!(cc.cwnd() > 0.0);
        assert!(cc.can_send());
    }

    #[test]
    fn test_slow_start_transition() {
        let mut cc = UdpCongestionControl::new();
        cc.ssthresh = 10.0;
        for _ in 0..10 {
            cc.on_ack(Duration::from_millis(50));
        }
        assert_eq!(cc.state(), CongestionState::CongestionAvoidance);
    }

    #[test]
    fn test_loss_event() {
        let mut cc = UdpCongestionControl::new();
        cc.cwnd = 20.0;
        cc.on_packet_sent();
        let cwnd_before = cc.cwnd();
        cc.on_loss();
        assert!(cc.cwnd() < cwnd_before);
        assert_eq!(cc.state(), CongestionState::FastRecovery);
    }

    #[test]
    fn test_timeout_event() {
        let mut cc = UdpCongestionControl::new();
        cc.cwnd = 20.0;
        cc.ssthresh = 64.0;
        cc.on_packet_sent();
        cc.on_timeout();
        assert_eq!(cc.cwnd(), 2.0);
        assert_eq!(cc.state(), CongestionState::SlowStart);
    }

    #[test]
    fn test_streaming_mode_switch() {
        let mut cc = UdpCongestionControl::new();
        cc.packets_lost = 10;
        cc.packets_sent = 50;
        cc.update_loss_rate();
        assert!(cc.is_streaming_mode());
    }

    #[test]
    fn test_can_send_limits() {
        let mut cc = UdpCongestionControl::new();
        cc.cwnd = 5.0;
        for _ in 0..5 {
            assert!(cc.can_send());
            cc.on_packet_sent();
        }
        assert!(!cc.can_send());
    }

    #[test]
    fn test_rtt_estimate() {
        let mut cc = UdpCongestionControl::new();
        cc.on_ack(Duration::from_millis(80));
        cc.on_ack(Duration::from_millis(90));
        cc.on_ack(Duration::from_millis(100));
        assert!(cc.rtt() >= Duration::from_millis(80));
        assert!(cc.rtt() <= Duration::from_millis(100));
    }

    #[test]
    fn test_timeout_duration() {
        let mut cc = UdpCongestionControl::new();
        cc.on_ack(Duration::from_millis(100));
        let timeout = cc.timeout_duration();
        assert!(timeout >= Duration::from_millis(100));
    }
}
