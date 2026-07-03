use eframe::egui::{self, Rect};

use crate::app::PlayerState;
use crate::skin::SkinEngine;

#[derive(Debug)]
pub struct OverlayManager;

impl OverlayManager {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    pub fn draw(
        &mut self,
        ctx: &egui::Context,
        state: &PlayerState,
        skin: &dyn SkinEngine,
        video_area: Rect,
        time: f64,
    ) {
        match state {
            PlayerState::Buffering => {
                egui::Area::new("overlay_buffering".into())
                    .fixed_pos(video_area.min)
                    .show(ctx, |ui| {
                        let painter = ui.painter();
                        skin.draw_buffering_overlay(painter, video_area, time);
                    });
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            PlayerState::Error(msg) => {
                egui::Area::new("overlay_error".into())
                    .fixed_pos(video_area.min)
                    .show(ctx, |ui| {
                        let painter = ui.painter();
                        skin.draw_error_overlay(painter, video_area, msg);
                    });
            }
            PlayerState::Paused => {
                egui::Area::new("overlay_paused".into())
                    .fixed_pos(video_area.min)
                    .show(ctx, |ui| {
                        let painter = ui.painter();
                        skin.draw_info_overlay(painter, video_area, "⏸ 已暂停");
                    });
            }
            _ => {}
        }
    }
}

impl Default for OverlayManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_overlay_new() {
        let mgr = OverlayManager::new();
        // OverlayManager is stateless, just tests construction works
        let _ = mgr;
    }
}
