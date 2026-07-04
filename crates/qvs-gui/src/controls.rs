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
#[allow(clippy::struct_excessive_bools)]
pub struct PlayerControls {
    pub playing: bool,
    pub volume: f32,
    pub muted: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub buffered_seconds: f64,
    pub fullscreen: bool,
    pub stop_pressed: bool,
}

use crate::skin::SkinEngine;
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
            buffered_seconds: 0.0,
            fullscreen: false,
            stop_pressed: false,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, skin: &dyn SkinEngine) {
        ui.horizontal(|ui| {
            ui.style_mut().spacing.item_spacing.x = 4.0;

            if skin.draw_play_button(ui, self.playing) {
                self.toggle_play();
            }
            if skin.draw_stop_button(ui) {
                self.stop_pressed = true;
                self.reset();
            }

            ui.separator();

            skin.draw_time_display(ui, self.position_ms, self.duration_ms);

            if let Some(new_progress) =
                skin.draw_progress_bar(ui, self.progress(), self.buffered_progress())
            {
                self.seek_to((new_progress * self.duration_ms as f32) as u64);
            }

            ui.separator();

            skin.draw_volume_control(ui, &mut self.volume, &mut self.muted);

            if skin.draw_fullscreen_button(ui) {
                self.fullscreen = !self.fullscreen;
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
            }
        });
    }

    pub fn toggle_play(&mut self) {
        self.playing = !self.playing;
    }

    pub fn seek_to(&mut self, position_ms: u64) {
        self.position_ms = position_ms.min(self.duration_ms);
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

    #[must_use]
    pub fn buffered_progress(&self) -> f32 {
        if self.duration_ms == 0 {
            return 0.0;
        }
        self.buffered_seconds as f32 / (self.duration_ms as f32 / 1000.0)
    }

    pub fn reset(&mut self) {
        self.playing = false;
        self.position_ms = 0;
        self.buffered_seconds = 0.0;
        self.stop_pressed = false;
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
