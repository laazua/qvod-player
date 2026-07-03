use eframe::egui;
use egui::TextureId;

use crate::app::PlayerState;
use crate::controls::PlayerControls;
use crate::overlay::OverlayManager;

pub struct PlayerPanel {
    pub controls: PlayerControls,
    pub overlay: OverlayManager,
    video_texture: Option<TextureId>,
    pub buffer_progress: f32,
}

impl PlayerPanel {
    #[must_use]
    pub fn new() -> Self {
        Self {
            controls: PlayerControls::new(0),
            overlay: OverlayManager::new(),
            video_texture: None,
            buffer_progress: 0.0,
        }
    }

    pub fn set_video_texture(&mut self, texture: TextureId) {
        self.video_texture = Some(texture);
    }

    #[must_use]
    pub fn video_texture(&self) -> Option<TextureId> {
        self.video_texture
    }

    pub fn clear_video(&mut self) {
        self.video_texture = None;
    }

    #[must_use]
    pub fn has_video(&self) -> bool {
        self.video_texture.is_some()
    }

    pub fn reset(&mut self, duration_ms: u64) {
        self.controls = PlayerControls::new(duration_ms);
        self.overlay = OverlayManager::new();
        self.video_texture = None;
        self.buffer_progress = 0.0;
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, state: &PlayerState) -> egui::Rect {
        let available = ui.available_size();
        let (rect, _response) = ui.allocate_exact_size(available, egui::Sense::click());

        ui.painter()
            .rect_filled(rect, 0.0, egui::Color32::from_rgb(20, 20, 20));

        if let Some(texture_id) = self.video_texture {
            ui.painter().image(
                texture_id,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        } else {
            let text = match state {
                PlayerState::Buffering => "Buffering...",
                PlayerState::Error(_) => {
                    return rect;
                }
                _ => "No Video",
            };
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                text,
                egui::FontId::proportional(24.0),
                egui::Color32::GRAY,
            );
        }

        if self.buffer_progress > 0.0 && self.buffer_progress < 1.0 {
            let bar_rect = egui::Rect::from_min_size(
                egui::pos2(rect.min.x, rect.max.y - 4.0),
                egui::vec2(rect.width() * self.buffer_progress, 4.0),
            );
            ui.painter()
                .rect_filled(bar_rect, 0.0, egui::Color32::from_rgb(0, 120, 215));
        }

        rect
    }
}

impl Default for PlayerPanel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_player_panel_creation() {
        let panel = PlayerPanel::new();
        assert_eq!(panel.controls.duration_ms, 0);
        assert!(!panel.has_video());
    }

    #[test]
    fn test_reset() {
        let mut panel = PlayerPanel::new();
        panel.reset(20000);
        assert_eq!(panel.controls.duration_ms, 20000);
        assert!(!panel.has_video());
    }
}
