use eframe::egui::{self, Context, Rect, Sense, Ui};

use super::palette;
use super::{ContextMenuAction, SkinEngine, TaskAction, TaskEntry, TitleBarAction};

pub struct Qvod6Skin;

impl Qvod6Skin {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Qvod6Skin {
    fn default() -> Self {
        Self::new()
    }
}

impl SkinEngine for Qvod6Skin {
    fn name(&self) -> &'static str {
        "Qvod 6.x"
    }

    fn apply_style(&self, ctx: &Context) {
        let mut style = (*ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.widgets.noninteractive.bg_fill = palette::SIDEBAR_BG;
        style.visuals.widgets.inactive.bg_fill = palette::LIST_ENTRY_SELECTED;
        style.visuals.widgets.active.bg_fill = palette::TAB_ACTIVE_BG;
        style.visuals.override_text_color = Some(palette::TEXT_PRIMARY);
        style.visuals.window_fill = palette::BG_GRADIENT_TOP;
        style.visuals.panel_fill = palette::BG_GRADIENT_BOTTOM;
        ctx.set_style(style);
    }

    fn draw_title_bar(&self, ui: &mut Ui, title: &str) -> TitleBarAction {
        let height = 32.0;
        let rect = ui
            .allocate_exact_size(
                egui::vec2(ui.available_width(), height),
                Sense::click_and_drag(),
            )
            .0;
        let painter = ui.painter_at(rect);

        painter.rect_filled(rect, 0.0, palette::TITLE_BAR_BG);
        painter.line_segment(
            [
                egui::pos2(rect.min.x, rect.max.y),
                egui::pos2(rect.max.x, rect.max.y),
            ],
            egui::Stroke::new(1.0, palette::TITLE_BAR_SEPARATOR),
        );

        painter.text(
            egui::pos2(rect.min.x + 8.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            format!("[Q] {title}"),
            egui::FontId::proportional(13.0),
            palette::TEXT_PRIMARY,
        );

        let btn_size = egui::vec2(34.0, 24.0);
        let btn_y = rect.center().y - btn_size.y / 2.0;

        let close_rect = egui::Rect::from_min_size(egui::pos2(rect.max.x - 34.0, btn_y), btn_size);
        let max_rect = egui::Rect::from_min_size(egui::pos2(rect.max.x - 68.0, btn_y), btn_size);
        let min_rect = egui::Rect::from_min_size(egui::pos2(rect.max.x - 102.0, btn_y), btn_size);

        let close_response = ui.allocate_rect(close_rect, Sense::click());
        let max_response = ui.allocate_rect(max_rect, Sense::click());
        let min_response = ui.allocate_rect(min_rect, Sense::click());

        let close_color = if close_response.hovered() {
            palette::ERROR
        } else {
            palette::BTN_DEFAULT
        };
        painter.text(
            close_rect.center(),
            egui::Align2::CENTER_CENTER,
            "✕",
            egui::FontId::proportional(14.0),
            close_color,
        );

        let max_color = if max_response.hovered() {
            palette::BTN_HOVER
        } else {
            palette::BTN_DEFAULT
        };
        painter.text(
            max_rect.center(),
            egui::Align2::CENTER_CENTER,
            "□",
            egui::FontId::proportional(14.0),
            max_color,
        );

        let min_color = if min_response.hovered() {
            palette::BTN_HOVER
        } else {
            palette::BTN_DEFAULT
        };
        painter.text(
            min_rect.center(),
            egui::Align2::CENTER_CENTER,
            "─",
            egui::FontId::proportional(14.0),
            min_color,
        );

        if close_response.clicked() {
            return TitleBarAction::Close;
        }
        if max_response.clicked() {
            return TitleBarAction::Maximize;
        }
        if min_response.clicked() {
            return TitleBarAction::Minimize;
        }
        if rect.contains(ui.input(|i| i.pointer.hover_pos().unwrap_or(egui::pos2(-1.0, -1.0))))
            && ui.input(|i| i.pointer.any_down())
        {
            return TitleBarAction::Drag;
        }

        TitleBarAction::None
    }

    fn draw_play_button(&self, ui: &mut Ui, playing: bool) -> bool {
        let size = egui::vec2(36.0, 36.0);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let painter = ui.painter_at(rect);

        let bg = if response.hovered() {
            palette::BTN_HOVER
        } else {
            palette::BTN_DEFAULT
        };
        painter.circle_filled(rect.center(), 16.0, bg);

        let text = if playing { "⏸" } else { "▶" };
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(16.0),
            palette::BG_GRADIENT_TOP,
        );

        response.clicked()
    }

    fn draw_stop_button(&self, ui: &mut Ui) -> bool {
        let size = egui::vec2(36.0, 36.0);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let painter = ui.painter_at(rect);
        let color = if response.hovered() {
            palette::BTN_HOVER
        } else {
            palette::BTN_DEFAULT
        };
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "■",
            egui::FontId::proportional(18.0),
            color,
        );
        response.clicked()
    }

    fn draw_time_display(&self, ui: &mut Ui, position_ms: u64, duration_ms: u64) {
        let pos = format!(
            "{:02}:{:02}",
            position_ms / 60000,
            (position_ms / 1000) % 60
        );
        let dur = format!(
            "{:02}:{:02}",
            duration_ms / 60000,
            (duration_ms / 1000) % 60
        );
        ui.label(
            egui::RichText::new(format!("{pos} / {dur}"))
                .color(palette::TEXT_PRIMARY)
                .size(12.0),
        );
    }

    fn draw_progress_bar(&self, ui: &mut Ui, progress: f32, buffered: f32) -> Option<f32> {
        let height = 8.0;
        let width = ui.available_width().max(100.0);
        let size = egui::vec2(width, height);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let painter = ui.painter_at(rect);

        painter.rect_filled(rect, 4.0, palette::PROGRESS_BG);

        if buffered > 0.0 {
            let buf_rect = egui::Rect::from_min_size(
                rect.min,
                egui::vec2(rect.width() * buffered.min(1.0), height),
            );
            painter.rect_filled(buf_rect, 4.0, palette::PROGRESS_BUFFERED);
        }

        let fill_rect = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(rect.width() * progress.min(1.0), height),
        );
        painter.rect_filled(fill_rect, 4.0, palette::PROGRESS_FILL);

        let thumb_x = rect.min.x + rect.width() * progress;
        let thumb_center = egui::pos2(thumb_x, rect.center().y);
        painter.circle_filled(thumb_center, 5.0, palette::BTN_HOVER);

        if let Some(mouse_pos) = response.interact_pointer_pos() {
            let rel_x = (mouse_pos.x - rect.min.x) / rect.width();
            let new_progress = rel_x.clamp(0.0, 1.0);
            return Some(new_progress);
        }

        None
    }

    fn draw_volume_control(&self, ui: &mut Ui, volume: &mut f32, muted: &mut bool) {
        ui.horizontal(|ui| {
            let icon = if *muted {
                "🔇"
            } else if *volume < 0.33 {
                "🔈"
            } else if *volume < 0.66 {
                "🔉"
            } else {
                "🔊"
            };
            if ui
                .add(egui::Button::new(icon).min_size(egui::vec2(28.0, 28.0)))
                .clicked()
            {
                *muted = !*muted;
            }

            let slider_width = 80.0;
            let slider_height = 6.0;
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(slider_width, slider_height),
                Sense::click_and_drag(),
            );
            let painter = ui.painter_at(rect);

            painter.rect_filled(rect, 3.0, palette::VOLUME_SLIDER_BG);
            let effective_vol = if *muted { 0.0 } else { *volume };
            let fill_rect = egui::Rect::from_min_size(
                rect.min,
                egui::vec2(rect.width() * effective_vol, slider_height),
            );
            painter.rect_filled(fill_rect, 3.0, palette::VOLUME_SLIDER_FILL);

            let thumb_center =
                egui::pos2(rect.min.x + rect.width() * effective_vol, rect.center().y);
            painter.circle_filled(thumb_center, 4.0, palette::BTN_HOVER);

            if let Some(mouse_pos) = response.interact_pointer_pos() {
                let rel = ((mouse_pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
                *volume = rel;
                *muted = false;
            }
        });
    }

    fn draw_fullscreen_button(&self, ui: &mut Ui) -> bool {
        let btn = egui::Button::new(egui::RichText::new("□").color(palette::BTN_DEFAULT))
            .min_size(egui::vec2(28.0, 28.0));
        ui.add(btn).clicked()
    }

    fn draw_tab_bar(&self, _ui: &mut Ui, _tabs: &[&str], _active: &mut usize) {}

    fn draw_task_entry(
        &self,
        _ui: &mut Ui,
        _entry: &TaskEntry,
        _index: usize,
        _selected: bool,
    ) -> TaskAction {
        TaskAction::None
    }

    fn draw_buffering_overlay(&self, _painter: &egui::Painter, _area: Rect, _time: f64) {}

    fn draw_error_overlay(&self, _painter: &egui::Painter, _area: Rect, _msg: &str) {}

    fn draw_info_overlay(&self, _painter: &egui::Painter, _area: Rect, _info: &str) {}

    fn draw_context_menu(
        &self,
        _ui: &mut Ui,
        _items: &[(&str, Vec<ContextMenuAction>)],
    ) -> Option<ContextMenuAction> {
        None
    }
}
