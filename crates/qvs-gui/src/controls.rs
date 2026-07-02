#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackAction {
    Play,
    Pause,
    Toggle,
    SeekForward,
    SeekBackward,
    VolumeUp,
    VolumeDown,
    Mute,
    Stop,
}

#[derive(Debug)]
pub struct PlayerControls {
    pub playing: bool,
    pub volume: f32,
    pub muted: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub seek_position: Option<u64>,
    pub buffered_seconds: f64,
}

use eframe::egui;

impl PlayerControls {
    #[must_use]
    pub fn new(duration_ms: u64) -> Self {
        Self {
            playing: false,
            volume: 0.8,
            muted: false,
            position_ms: 0,
            duration_ms,
            seek_position: None,
            buffered_seconds: 0.0,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.style_mut().spacing.item_spacing.x = 8.0;

            let play_label = if self.playing { "⏸" } else { "▶" };
            if ui
                .add(egui::Button::new(play_label).min_size(egui::vec2(40.0, 40.0)))
                .clicked()
            {
                self.toggle_play();
            }

            let pos_str = format!(
                "{:02}:{:02}",
                self.position_ms / 60000,
                (self.position_ms / 1000) % 60
            );
            let dur_str = format!(
                "{:02}:{:02}",
                self.duration_ms / 60000,
                (self.duration_ms / 1000) % 60
            );
            ui.label(format!("{pos_str} / {dur_str}"));

            if self.duration_ms > 0 {
                let mut pos_f32 = self.position_ms as f32 / self.duration_ms as f32;
                let slider = ui.add(
                    egui::Slider::new(&mut pos_f32, 0.0..=1.0)
                        .text("")
                        .show_value(false)
                        .fixed_decimals(3),
                );
                if slider.changed() {
                    let new_pos = (pos_f32 * self.duration_ms as f32) as u64;
                    self.seek_to(new_pos);
                }
            }

            ui.separator();

            ui.label("🔊");
            ui.add(
                egui::Slider::new(&mut self.volume, 0.0..=1.0)
                    .text("")
                    .show_value(false),
            );

            let mute_label = if self.muted { "🔇" } else { "🔊" };
            if ui.button(mute_label).clicked() {
                self.muted = !self.muted;
            }
        });
    }

    pub fn toggle_play(&mut self) {
        self.playing = !self.playing;
    }

    pub fn seek_to(&mut self, position_ms: u64) {
        self.position_ms = position_ms.min(self.duration_ms);
        self.seek_position = None;
    }

    pub fn start_seek(&mut self) {
        self.seek_position = Some(self.position_ms);
    }

    pub fn update_seek(&mut self, position_ms: u64) {
        self.seek_position = Some(position_ms.min(self.duration_ms));
    }

    pub fn end_seek(&mut self) {
        if let Some(pos) = self.seek_position {
            self.position_ms = pos;
            self.seek_position = None;
        }
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
    }

    pub fn toggle_mute(&mut self) {
        self.muted = !self.muted;
    }

    pub fn seek_forward(&mut self, amount_ms: u64) {
        self.position_ms = (self.position_ms + amount_ms).min(self.duration_ms);
    }

    pub fn seek_backward(&mut self, amount_ms: u64) {
        self.position_ms = self.position_ms.saturating_sub(amount_ms);
    }

    #[must_use]
    pub fn effective_volume(&self) -> f32 {
        if self.muted {
            0.0
        } else {
            self.volume
        }
    }

    #[must_use]
    pub fn progress(&self) -> f32 {
        if self.duration_ms == 0 {
            return 0.0;
        }
        self.position_ms as f32 / self.duration_ms as f32
    }

    pub fn reset(&mut self) {
        self.playing = false;
        self.position_ms = 0;
        self.seek_position = None;
        self.buffered_seconds = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let ctrl = PlayerControls::new(10000);
        assert!(!ctrl.playing);
        assert!((ctrl.volume - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn test_toggle_play() {
        let mut ctrl = PlayerControls::new(10000);
        ctrl.toggle_play();
        assert!(ctrl.playing);
        ctrl.toggle_play();
        assert!(!ctrl.playing);
    }

    #[test]
    fn test_seek() {
        let mut ctrl = PlayerControls::new(10000);
        ctrl.seek_to(5000);
        assert_eq!(ctrl.position_ms, 5000);
    }

    #[test]
    fn test_volume_clamp() {
        let mut ctrl = PlayerControls::new(10000);
        ctrl.set_volume(1.5);
        assert!((ctrl.volume - 1.0).abs() < f32::EPSILON);
        ctrl.set_volume(-0.5);
        assert!((ctrl.volume - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_mute() {
        let mut ctrl = PlayerControls::new(10000);
        assert!((ctrl.effective_volume() - 0.8).abs() < f32::EPSILON);
        ctrl.toggle_mute();
        assert!((ctrl.effective_volume() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_seek_forward_backward() {
        let mut ctrl = PlayerControls::new(10000);
        ctrl.seek_forward(5000);
        assert_eq!(ctrl.position_ms, 5000);
        ctrl.seek_backward(1000);
        assert_eq!(ctrl.position_ms, 4000);
    }

    #[test]
    fn test_progress() {
        let mut ctrl = PlayerControls::new(10000);
        ctrl.seek_to(2500);
        assert!((ctrl.progress() - 0.25).abs() < f32::EPSILON);
    }

    #[test]
    fn test_reset() {
        let mut ctrl = PlayerControls::new(10000);
        ctrl.toggle_play();
        ctrl.seek_to(5000);
        ctrl.reset();
        assert!(!ctrl.playing);
        assert_eq!(ctrl.position_ms, 0);
    }
}
