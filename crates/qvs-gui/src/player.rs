use eframe::egui;
use egui::TextureId;

use crate::skin::palette;

use crate::app::PlayerState;
use crate::controls::PlayerControls;
use crate::overlay::OverlayManager;

/// Aspect ratio mode for video display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AspectRatio {
    /// Free — stretch to fill the entire area (no aspect ratio preservation).
    Free,
    /// Force 4:3 aspect ratio.
    V4x3,
    /// Force 16:9 aspect ratio.
    V16x9,
    /// Use the video's original (native) aspect ratio.
    Original,
}

impl AspectRatio {
    /// Human-readable label for the aspect ratio mode.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Free => "自由",
            Self::V4x3 => "4:3",
            Self::V16x9 => "16:9",
            Self::Original => "原始",
        }
    }

    /// All aspect ratio variants, ordered most-to-least common.
    #[must_use]
    pub fn variants() -> &'static [Self] {
        static VARIANTS: [AspectRatio; 4] = [
            AspectRatio::Original,
            AspectRatio::V16x9,
            AspectRatio::V4x3,
            AspectRatio::Free,
        ];
        &VARIANTS
    }
}

/// Zoom mode for video display.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ZoomMode {
    /// Scale the video to fit the window while preserving aspect ratio.
    FitToWindow,
    /// Show at original (1:1) pixel resolution.
    OriginalSize,
    /// User-defined zoom factor (1.0 = 100%).
    Custom(f32),
}

impl ZoomMode {
    /// Human-readable label.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::FitToWindow => "适应窗口".into(),
            Self::OriginalSize => "原始大小".into(),
            Self::Custom(z) => format!("{}%", (z * 100.0) as i32),
        }
    }
}

/// The video player panel — renders video frames, manages zoom/pan, and
/// coordinates overlay + control rendering.
pub struct PlayerPanel {
    pub controls: PlayerControls,
    pub overlay: OverlayManager,
    video_texture: Option<TextureId>,
    pub buffer_progress: f32,

    // -- Video display control --
    pub aspect_ratio: AspectRatio,
    pub zoom_mode: ZoomMode,
    /// Native video width in pixels (0 = unknown).
    video_width: u32,
    /// Native video height in pixels (0 = unknown).
    video_height: u32,
    /// Accumulated pan offset in screen pixels (applied when zoomed in).
    pan_offset: egui::Vec2,
}

impl PlayerPanel {
    #[must_use]
    pub fn new() -> Self {
        Self {
            controls: PlayerControls::new(0),
            overlay: OverlayManager::new(),
            video_texture: None,
            buffer_progress: 0.0,
            aspect_ratio: AspectRatio::Original,
            zoom_mode: ZoomMode::FitToWindow,
            video_width: 0,
            video_height: 0,
            pan_offset: egui::Vec2::ZERO,
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

    /// Set the native video dimensions (used for `Original` aspect ratio and
    /// `OriginalSize` zoom mode).
    pub fn set_video_dimensions(&mut self, width: u32, height: u32) {
        self.video_width = width;
        self.video_height = height;
    }

    /// Zoom in one step.
    pub fn zoom_in(&mut self) {
        match self.zoom_mode {
            ZoomMode::FitToWindow => {
                self.zoom_mode = ZoomMode::Custom(1.5);
            }
            ZoomMode::OriginalSize => {
                self.zoom_mode = ZoomMode::Custom(1.5);
            }
            ZoomMode::Custom(z) => {
                self.zoom_mode = ZoomMode::Custom((z * 1.25).min(10.0));
            }
        }
    }

    /// Zoom out one step.
    pub fn zoom_out(&mut self) {
        match self.zoom_mode {
            ZoomMode::FitToWindow => {
                self.zoom_mode = ZoomMode::Custom(0.75);
            }
            ZoomMode::OriginalSize => {
                self.zoom_mode = ZoomMode::Custom(0.75);
            }
            ZoomMode::Custom(z) => {
                let new_z = (z / 1.25).max(0.1);
                if (new_z - 1.0).abs() < 0.05 {
                    // Close enough to 1.0 — snap back to FitToWindow.
                    self.zoom_mode = ZoomMode::FitToWindow;
                    self.pan_offset = egui::Vec2::ZERO;
                } else {
                    self.zoom_mode = ZoomMode::Custom(new_z);
                }
            }
        }
    }

    /// Reset zoom to fit-to-window and clear pan offset.
    pub fn reset_zoom(&mut self) {
        self.zoom_mode = ZoomMode::FitToWindow;
        self.pan_offset = egui::Vec2::ZERO;
    }

    // ── private helpers ──────────────────────────────────────────────

    /// True when the display is larger than the available viewport (panning
    /// is useful).
    #[must_use]
    fn is_zoomed_in(display_size: egui::Vec2, viewport_size: egui::Vec2) -> bool {
        display_size.x > viewport_size.x + 1.0 || display_size.y > viewport_size.y + 1.0
    }

    /// Compute the letterbox rect — the largest rectangle that fits within
    /// `available` at the chosen aspect ratio.
    #[must_use]
    fn letterbox_rect(&self, available: egui::Rect) -> egui::Rect {
        let ar = self.effective_aspect_ratio();
        let avail = available.size();
        let (w, h) = if avail.x / avail.y > ar {
            // available is wider → horizontal letterbox (black bars on sides)
            (avail.y * ar, avail.y)
        } else {
            // available is taller → vertical letterbox (black bars top/bottom)
            (avail.x, avail.x / ar)
        };
        egui::Rect::from_center_size(available.center(), egui::vec2(w, h))
    }

    /// The aspect ratio value derived from the current mode, falling back to
    /// 16:9 when video dimensions are unknown.
    #[must_use]
    fn effective_aspect_ratio(&self) -> f32 {
        match self.aspect_ratio {
            AspectRatio::Free => {
                // Free mode is handled separately — this value is never used.
                1.0
            }
            AspectRatio::V4x3 => 4.0 / 3.0,
            AspectRatio::V16x9 => 16.0 / 9.0,
            AspectRatio::Original => {
                if self.video_width > 0 && self.video_height > 0 {
                    self.video_width as f32 / self.video_height as f32
                } else {
                    16.0 / 9.0 // sensible default
                }
            }
        }
    }

    /// Calculate the screen rectangle where the video texture is drawn.
    #[must_use]
    fn display_rect(&self, available: egui::Rect) -> egui::Rect {
        if self.aspect_ratio == AspectRatio::Free {
            return available;
        }

        let letterbox = self.letterbox_rect(available);
        let lb_size = letterbox.size();

        let scale = match self.zoom_mode {
            ZoomMode::FitToWindow => 1.0,
            ZoomMode::OriginalSize => {
                if self.video_width > 0 && lb_size.x > 0.0 {
                    (self.video_width as f32 / lb_size.x).max(0.1)
                } else {
                    1.0
                }
            }
            ZoomMode::Custom(z) => z.max(0.1),
        };

        let display_size = egui::vec2(lb_size.x * scale, lb_size.y * scale);

        // Clamp pan offset so there is never empty space between the display
        // edge and the viewport edge.
        let max_pan = ((display_size - available.size()) / 2.0).max(egui::Vec2::ZERO);
        let clamped_pan = egui::vec2(
            self.pan_offset.x.clamp(-max_pan.x, max_pan.x),
            self.pan_offset.y.clamp(-max_pan.y, max_pan.y),
        );

        let center = egui::pos2(
            available.center().x + clamped_pan.x,
            available.center().y + clamped_pan.y,
        );

        egui::Rect::from_center_size(center, display_size)
    }

    /// Reset everything for a new playback session.
    pub fn reset(&mut self, duration_ms: u64) {
        self.controls = PlayerControls::new(duration_ms);
        self.overlay = OverlayManager::new();
        self.video_texture = None;
        self.buffer_progress = 0.0;
        self.aspect_ratio = AspectRatio::Original;
        self.zoom_mode = ZoomMode::FitToWindow;
        self.video_width = 0;
        self.video_height = 0;
        self.pan_offset = egui::Vec2::ZERO;
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, state: &PlayerState) -> egui::Rect {
        let available = ui.available_size();
        let (rect, response) = ui.allocate_exact_size(available, egui::Sense::click_and_drag());

        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, palette::VIDEO_BG);

        // ── Display rect and video frame ─────────────────────────────
        let drect = self.display_rect(rect);
        let dsize = drect.size();

        if let Some(texture_id) = self.video_texture {
            painter.image(
                texture_id,
                drect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );

            // Subtle border so the video edge is visible when letterboxed.
            if self.aspect_ratio != AspectRatio::Free && drect != rect {
                painter.rect_stroke(
                    drect,
                    0.0,
                    egui::Stroke::new(
                        1.0,
                        egui::Color32::from_rgba_premultiplied(255, 255, 255, 24),
                    ),
                );
            }
        } else {
            let text = match state {
                PlayerState::Buffering => "Buffering...",
                PlayerState::Error(_) => {
                    return rect;
                }
                _ => "No Video",
            };
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                text,
                egui::FontId::proportional(24.0),
                egui::Color32::GRAY,
            );
        }

        // ── Mouse-wheel zoom ─────────────────────────────────────────
        if response.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta);
            if scroll.y.abs() > f32::EPSILON {
                if scroll.y > 0.0 {
                    self.zoom_in();
                } else {
                    self.zoom_out();
                }
                ui.ctx().request_repaint();
            }
        }

        // ── Pan when zoomed in ───────────────────────────────────────
        if Self::is_zoomed_in(dsize, rect.size()) {
            let drag = response.drag_delta();
            if drag != egui::Vec2::ZERO {
                self.pan_offset += drag;
                let max_pan = ((dsize - rect.size()) / 2.0).max(egui::Vec2::ZERO);
                self.pan_offset = egui::vec2(
                    self.pan_offset.x.clamp(-max_pan.x, max_pan.x),
                    self.pan_offset.y.clamp(-max_pan.y, max_pan.y),
                );
            }

            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
            if response.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
        }

        // Double-click resets zoom.
        if response.double_clicked() {
            self.reset_zoom();
        }

        // ── Buffer progress bar ──────────────────────────────────────
        if self.buffer_progress > 0.0 && self.buffer_progress < 1.0 {
            let bar_rect = egui::Rect::from_min_size(
                egui::pos2(rect.min.x, rect.max.y - 4.0),
                egui::vec2(rect.width() * self.buffer_progress, 4.0),
            );
            painter.rect_filled(bar_rect, 0.0, palette::BTN_ACTIVE);
        }

        // ── Zoom indicator (bottom-right corner, only for custom zoom) ─
        if let ZoomMode::Custom(z) = self.zoom_mode {
            if (z - 1.0).abs() > 0.01 {
                let label = format!("{}%", (z * 100.0) as i32);
                painter.text(
                    egui::pos2(rect.max.x - 10.0, rect.max.y - 30.0),
                    egui::Align2::RIGHT_BOTTOM,
                    label,
                    egui::FontId::proportional(14.0),
                    egui::Color32::from_rgba_premultiplied(255, 255, 255, 180),
                );
            }
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
    use egui::Vec2;

    #[test]
    fn test_player_panel_creation() {
        let panel = PlayerPanel::new();
        assert_eq!(panel.controls.duration_ms, 0);
        assert!(!panel.has_video());
        assert_eq!(panel.aspect_ratio, AspectRatio::Original);
        assert_eq!(panel.zoom_mode, ZoomMode::FitToWindow);
    }

    #[test]
    fn test_reset() {
        let mut panel = PlayerPanel::new();
        panel.reset(20000);
        assert_eq!(panel.controls.duration_ms, 20000);
        assert!(!panel.has_video());
        assert_eq!(panel.aspect_ratio, AspectRatio::Original);
        assert_eq!(panel.zoom_mode, ZoomMode::FitToWindow);
    }

    #[test]
    fn test_aspect_ratio_labels() {
        assert_eq!(AspectRatio::Free.label(), "自由");
        assert_eq!(AspectRatio::V4x3.label(), "4:3");
        assert_eq!(AspectRatio::V16x9.label(), "16:9");
        assert_eq!(AspectRatio::Original.label(), "原始");
    }

    #[test]
    fn test_zoom_in_out() {
        let mut panel = PlayerPanel::new();
        assert_eq!(panel.zoom_mode, ZoomMode::FitToWindow);

        panel.zoom_in();
        assert_eq!(panel.zoom_mode, ZoomMode::Custom(1.5));

        panel.zoom_in();
        assert_eq!(panel.zoom_mode, ZoomMode::Custom(1.875)); // 1.5 * 1.25

        panel.zoom_out();
        assert_eq!(panel.zoom_mode, ZoomMode::Custom(1.5)); // 1.875 / 1.25

        panel.zoom_out();
        assert_eq!(panel.zoom_mode, ZoomMode::Custom(1.2)); // 1.5 / 1.25

        panel.reset_zoom();
        assert_eq!(panel.zoom_mode, ZoomMode::FitToWindow);
    }

    #[test]
    fn test_letterbox_rect_16x9() {
        let panel = PlayerPanel::new();
        // 16:9 video in a square viewport → vertical letterbox
        let available = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(400.0, 400.0));
        let lb = panel.letterbox_rect(available);
        let expected_w = 400.0;
        let expected_h = 400.0 / (16.0 / 9.0); // 225.0
        assert!((lb.width() - expected_w).abs() < 0.01);
        assert!((lb.height() - expected_h).abs() < 0.01);
    }

    #[test]
    fn test_letterbox_rect_v4x3() {
        let mut panel = PlayerPanel::new();
        panel.aspect_ratio = AspectRatio::V4x3;
        let available = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(400.0, 300.0));
        let lb = panel.letterbox_rect(available);
        // 4:3 matches 400:300 exactly → no letterbox
        assert!((lb.width() - 400.0).abs() < 0.01);
        assert!((lb.height() - 300.0).abs() < 0.01);
    }

    #[test]
    fn test_display_rect_free() {
        let mut panel = PlayerPanel::new();
        panel.aspect_ratio = AspectRatio::Free;
        let available = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(800.0, 600.0));
        let drect = panel.display_rect(available);
        assert_eq!(drect, available);
    }

    #[test]
    fn test_video_dimensions() {
        let mut panel = PlayerPanel::new();
        assert!(!panel.has_video());
        panel.set_video_dimensions(1920, 1080);
        let ar = panel.effective_aspect_ratio();
        assert!((ar - 16.0 / 9.0).abs() < 0.01);
    }

    #[test]
    fn test_zoom_mode_labels() {
        assert_eq!(ZoomMode::FitToWindow.label(), "适应窗口");
        assert_eq!(ZoomMode::OriginalSize.label(), "原始大小");
        assert_eq!(ZoomMode::Custom(1.5).label(), "150%");
    }

    #[test]
    fn test_aspect_ratio_variants() {
        let variants = AspectRatio::variants();
        assert_eq!(variants.len(), 4);
        assert_eq!(variants[0], AspectRatio::Original);
    }
}
