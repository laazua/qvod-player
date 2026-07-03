use eframe::egui::{self, Context, Rect, Sense, Ui};
use std::f64::consts::TAU;

use super::palette;
use super::{ContextMenuAction, SkinEngine, TaskAction, TaskEntry, TaskStatus, TitleBarAction};

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

    fn draw_tab_bar(&self, ui: &mut Ui, tabs: &[&str], active: &mut usize) {
        ui.horizontal(|ui| {
            ui.style_mut().spacing.item_spacing.x = 0.0;
            for (i, tab) in tabs.iter().enumerate() {
                let bg = if i == *active {
                    palette::TAB_ACTIVE_BG
                } else {
                    palette::TAB_INACTIVE_BG
                };
                let response = egui::Frame::none()
                    .fill(bg)
                    .inner_margin(egui::Margin::symmetric(12.0, 4.0))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(*tab)
                                .color(palette::TEXT_PRIMARY)
                                .size(12.0),
                        );
                    })
                    .response;
                if response.clicked() {
                    *active = i;
                }
            }
        });
    }

    fn draw_task_entry(
        &self,
        ui: &mut Ui,
        entry: &TaskEntry,
        index: usize,
        selected: bool,
    ) -> TaskAction {
        let height = 52.0;
        let width = ui.available_width();
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
        let painter = ui.painter_at(rect);

        let bg = if selected {
            palette::LIST_ENTRY_SELECTED
        } else if rect
            .contains(ui.input(|i| i.pointer.hover_pos().unwrap_or(egui::pos2(-1.0, -1.0))))
        {
            palette::LIST_ENTRY_HOVER
        } else {
            egui::Color32::TRANSPARENT
        };
        painter.rect_filled(rect, 0.0, bg);

        let icon = match entry.status {
            TaskStatus::Downloading => "▶",
            TaskStatus::Paused => "⏸",
            TaskStatus::Completed => "✓",
            TaskStatus::Error(_) => "⚠",
        };
        let icon_color = match entry.status {
            TaskStatus::Completed => palette::SUCCESS,
            TaskStatus::Error(_) => palette::ERROR,
            _ => palette::BTN_HOVER,
        };
        painter.text(
            egui::pos2(rect.min.x + 8.0, rect.min.y + 10.0),
            egui::Align2::LEFT_TOP,
            icon,
            egui::FontId::proportional(14.0),
            icon_color,
        );

        painter.text(
            egui::pos2(rect.min.x + 28.0, rect.min.y + 8.0),
            egui::Align2::LEFT_TOP,
            &entry.title,
            egui::FontId::proportional(12.0),
            palette::TEXT_PRIMARY,
        );

        let info = match entry.status {
            TaskStatus::Downloading => {
                format!(
                    "{:.1}/{:.1}MB ↓{:.0}KB/s",
                    entry.downloaded as f64 / 1_048_576.0,
                    entry.total as f64 / 1_048_576.0,
                    entry.speed_down / 1024.0
                )
            }
            TaskStatus::Completed => "已下载  ✓".into(),
            TaskStatus::Paused => "已暂停".into(),
            TaskStatus::Error(ref e) => format!("错误: {e}"),
        };
        painter.text(
            egui::pos2(rect.min.x + 28.0, rect.min.y + 26.0),
            egui::Align2::LEFT_TOP,
            info,
            egui::FontId::proportional(10.0),
            palette::TEXT_SECONDARY,
        );

        let bar_rect = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + 8.0, rect.max.y - 8.0),
            egui::vec2(rect.width() - 16.0, 4.0),
        );
        painter.rect_filled(bar_rect, 2.0, palette::PROGRESS_BG);
        if entry.progress > 0.0 {
            let fill = egui::Rect::from_min_size(
                bar_rect.min,
                egui::vec2(bar_rect.width() * entry.progress as f32, 4.0),
            );
            painter.rect_filled(fill, 2.0, palette::PROGRESS_FILL);
        }

        if selected && ui.input(|i| i.pointer.button_clicked(egui::PointerButton::Secondary)) {
            return TaskAction::ContextMenu(index);
        }

        if response.clicked() {
            return TaskAction::Select(index);
        }

        TaskAction::None
    }

    fn draw_buffering_overlay(&self, painter: &egui::Painter, area: Rect, time: f64) {
        painter.rect_filled(area, 0.0, palette::OVERLAY_BG);

        let center = area.center();
        let radius = 20.0;
        let num_segments = 8;
        let angle_offset = (time * 3.0) % TAU;

        for i in 0..num_segments {
            let angle = angle_offset + (f64::from(i) * TAU / f64::from(num_segments));
            let alpha = (num_segments - i) as f32 / num_segments as f32;
            let color =
                egui::Color32::from_rgba_premultiplied(0x4F, 0xC3, 0xF7, (alpha * 200.0) as u8);

            let start = egui::pos2(
                center.x + radius * (angle as f32).cos(),
                center.y + radius * (angle as f32).sin(),
            );
            let end = egui::pos2(
                center.x + (radius + 6.0) * (angle as f32).cos(),
                center.y + (radius + 6.0) * (angle as f32).sin(),
            );
            painter.line_segment([start, end], egui::Stroke::new(3.0, color));
        }

        painter.text(
            egui::pos2(center.x, center.y + 35.0),
            egui::Align2::CENTER_CENTER,
            "缓冲中...",
            egui::FontId::proportional(14.0),
            palette::TEXT_PRIMARY,
        );
    }

    fn draw_error_overlay(&self, painter: &egui::Painter, area: Rect, msg: &str) {
        painter.rect_filled(
            area,
            0.0,
            egui::Color32::from_rgba_premultiplied(0, 0, 0, 180),
        );
        painter.text(
            area.center(),
            egui::Align2::CENTER_CENTER,
            format!("⚠ {msg}"),
            egui::FontId::proportional(18.0),
            palette::ERROR,
        );
    }

    fn draw_info_overlay(&self, painter: &egui::Painter, area: Rect, info: &str) {
        painter.text(
            egui::pos2(area.min.x + 8.0, area.min.y + 8.0),
            egui::Align2::LEFT_TOP,
            info,
            egui::FontId::proportional(12.0),
            palette::TEXT_SECONDARY,
        );
    }

    fn draw_context_menu(
        &self,
        ui: &mut Ui,
        pos: egui::Pos2,
        items: &[(&str, Vec<ContextMenuAction>)],
    ) -> Option<ContextMenuAction> {
        let mut result = None;
        let ctx = ui.ctx();
        egui::Area::new(ui.next_auto_id())
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(palette::MENU_BG)
                    .stroke(egui::Stroke::new(1.0, palette::MENU_SEPARATOR))
                    .inner_margin(egui::Margin::symmetric(4.0, 2.0))
                    .show(ui, |ui| {
                        ui.style_mut().spacing.item_spacing = egui::vec2(0.0, 0.0);
                        for (group_label, actions) in items {
                            if !group_label.is_empty() {
                                ui.label(
                                    egui::RichText::new(*group_label)
                                        .size(10.0)
                                        .color(palette::TEXT_SECONDARY),
                                );
                            }
                            for action in actions {
                                let label = match action {
                                    ContextMenuAction::Play => "播放",
                                    ContextMenuAction::Pause => "暂停",
                                    ContextMenuAction::Stop => "停止",
                                    ContextMenuAction::Restart => "重新开始",
                                    ContextMenuAction::Remove => "删除",
                                    ContextMenuAction::Properties => "属性",
                                    ContextMenuAction::ToggleFullscreen => "全屏",
                                    ContextMenuAction::AspectRatio4x3 => "画面比例 4:3",
                                    ContextMenuAction::AspectRatio16x9 => "画面比例 16:9",
                                    ContextMenuAction::AspectRatioOriginal => "画面比例 原始",
                                    ContextMenuAction::OpenSettings => "设置",
                                    ContextMenuAction::About => "关于",
                                    ContextMenuAction::PriorityHigh => "优先下载 - 高",
                                    ContextMenuAction::PriorityNormal => "优先下载 - 普通",
                                    ContextMenuAction::PriorityLow => "优先下载 - 低",
                                };
                                if ui.selectable_label(false, label).clicked() {
                                    result = Some(action.clone());
                                }
                            }
                            ui.separator();
                        }
                    });
            });
        result
    }
}
