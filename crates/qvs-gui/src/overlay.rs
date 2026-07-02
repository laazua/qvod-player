use eframe::egui::{self, Color32};

use crate::app::PlayerState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayType {
    Buffering,
    Error(String),
    Info(String),
}

#[derive(Debug)]
pub struct OverlayManager {
    current: Option<OverlayType>,
    visible: bool,
    fade_alpha: f32,
}

impl OverlayManager {
    #[must_use]
    pub fn new() -> Self {
        Self {
            current: None,
            visible: false,
            fade_alpha: 1.0,
        }
    }

    pub fn show(&mut self, overlay: OverlayType) {
        self.current = Some(overlay);
        self.visible = true;
        self.fade_alpha = 1.0;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.fade_alpha = 0.0;
    }

    #[must_use]
    pub fn update(&mut self, _dt_secs: f32) -> Option<&OverlayType> {
        if !self.visible {
            self.current = None;
        }
        self.current.as_ref()
    }

    #[must_use]
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn draw(&mut self, ctx: &egui::Context, state: &PlayerState) {
        match state {
            PlayerState::Buffering => {
                egui::Area::new("buffering".into())
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, -100.0])
                    .show(ctx, |ui| {
                        ui.label(
                            egui::RichText::new("⏳ Buffering...")
                                .size(24.0)
                                .color(Color32::WHITE),
                        );
                    });
            }
            PlayerState::Error(msg) => {
                egui::Area::new("error".into())
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(
                            egui::RichText::new(format!("⚠ {msg}"))
                                .size(18.0)
                                .color(Color32::RED),
                        );
                    });
            }
            PlayerState::Paused => {
                egui::Area::new("paused".into())
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(
                            egui::RichText::new("⏸ Paused")
                                .size(32.0)
                                .color(Color32::from_rgba_premultiplied(255, 255, 255, 128)),
                        );
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

#[must_use]
pub fn overlay_color(overlay: &OverlayType) -> Color32 {
    match overlay {
        OverlayType::Buffering => Color32::from_rgba_premultiplied(0, 0, 0, 180),
        OverlayType::Error(_) => Color32::from_rgba_premultiplied(180, 0, 0, 200),
        OverlayType::Info(_) => Color32::from_rgba_premultiplied(0, 0, 0, 160),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_overlay_show_hide() {
        let mut mgr = OverlayManager::new();
        assert!(!mgr.is_visible());
        mgr.show(OverlayType::Buffering);
        assert!(mgr.is_visible());
        mgr.hide();
        assert!(!mgr.is_visible());
    }

    #[test]
    fn test_overlay_update() {
        let mut mgr = OverlayManager::new();
        mgr.show(OverlayType::Info("test".into()));
        let result = mgr.update(0.1);
        assert!(result.is_some());
    }

    #[test]
    fn test_overlay_color() {
        let c = overlay_color(&OverlayType::Buffering);
        assert_eq!(c, Color32::from_rgba_premultiplied(0, 0, 0, 180));
        let c = overlay_color(&OverlayType::Error("err".into()));
        assert_eq!(c, Color32::from_rgba_premultiplied(180, 0, 0, 200));
    }
}
