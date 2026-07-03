use std::sync::OnceLock;

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
        // Lazy-loaded title bar icon from the embedded qvod.ico
        static TITLE_ICON: OnceLock<egui::TextureHandle> = OnceLock::new();

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
        let tex = TITLE_ICON.get_or_init(|| {
            let image = crate::icon::color_image_small().clone();
            ui.ctx()
                .load_texture("title_icon", image, egui::TextureOptions::NEAREST)
        });

        // Draw title-bar icon at its native 16×16
        let icon_size = 16.0;
        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(rect.min.x + 12.0, rect.center().y),
            egui::vec2(icon_size, icon_size),
        );
        egui::Image::from_texture(tex).paint_at(ui, icon_rect);

        // Title text after the icon
        painter.text(
            egui::pos2(rect.min.x + 24.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            title,
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
        if close_response.hovered() {
            painter.rect_filled(
                close_rect,
                0.0,
                egui::Color32::from_rgba_premultiplied(0xE8, 0x11, 0x23, 200),
            );
        }
        // Draw close X
        let cx = close_rect.center().x;
        let cy = close_rect.center().y;
        let half = 5.5;
        let close_line_color = if close_response.hovered() {
            egui::Color32::WHITE
        } else {
            close_color
        };
        painter.line_segment(
            [
                egui::pos2(cx - half, cy - half),
                egui::pos2(cx + half, cy + half),
            ],
            egui::Stroke::new(1.5, close_line_color),
        );
        painter.line_segment(
            [
                egui::pos2(cx + half, cy - half),
                egui::pos2(cx - half, cy + half),
            ],
            egui::Stroke::new(1.5, close_line_color),
        );

        let max_color = if max_response.hovered() {
            palette::BTN_HOVER
        } else {
            palette::BTN_DEFAULT
        };
        if max_response.hovered() {
            painter.rect_filled(
                max_rect,
                0.0,
                egui::Color32::from_rgba_premultiplied(0xFF, 0xFF, 0xFF, 20),
            );
        }
        // Draw maximize square
        let cx = max_rect.center().x;
        let cy = max_rect.center().y;
        let half = 5.5;
        let square =
            egui::Rect::from_center_size(egui::pos2(cx, cy), egui::vec2(half * 2.0, half * 2.0));
        painter.rect_stroke(square, 1.0, egui::Stroke::new(1.5, max_color));

        let min_color = if min_response.hovered() {
            palette::BTN_HOVER
        } else {
            palette::BTN_DEFAULT
        };
        if min_response.hovered() {
            painter.rect_filled(
                min_rect,
                0.0,
                egui::Color32::from_rgba_premultiplied(0xFF, 0xFF, 0xFF, 20),
            );
        }
        // Draw minimize line
        let cx = min_rect.center().x;
        let cy = min_rect.center().y;
        painter.line_segment(
            [egui::pos2(cx - 6.5, cy), egui::pos2(cx + 6.5, cy)],
            egui::Stroke::new(1.5, min_color),
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

        let cx = rect.center().x;
        let cy = rect.center().y;
        let icon_color = palette::CONTROL_BAR_BG;

        if playing {
            // Pause: two rounded vertical bars
            let bar_w = 3.0;
            let bar_h = 9.0;
            let gap = 2.5;
            painter.rect_filled(
                egui::Rect::from_center_size(
                    egui::pos2(cx - gap / 2.0 - bar_w / 2.0, cy),
                    egui::vec2(bar_w, bar_h),
                ),
                1.0,
                icon_color,
            );
            painter.rect_filled(
                egui::Rect::from_center_size(
                    egui::pos2(cx + gap / 2.0 + bar_w / 2.0, cy),
                    egui::vec2(bar_w, bar_h),
                ),
                1.0,
                icon_color,
            );
        } else {
            // Play: right-pointing triangle
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(cx - 3.5, cy - 5.5),
                    egui::pos2(cx - 3.5, cy + 5.5),
                    egui::pos2(cx + 5.5, cy),
                ],
                icon_color,
                egui::Stroke::new(0.0, egui::Color32::TRANSPARENT),
            ));
        }

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
        let cx = rect.center().x;
        let cy = rect.center().y;
        painter.rect_filled(
            egui::Rect::from_center_size(egui::pos2(cx, cy), egui::vec2(8.0, 8.0)),
            1.5,
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
            // Drawn speaker icon (avoids emoji font-dependency on Windows).
            let (icon_rect, icon_response) =
                ui.allocate_exact_size(egui::vec2(28.0, 28.0), Sense::click());
            {
                let painter = ui.painter_at(icon_rect);
                let cx = icon_rect.center().x;
                let cy = icon_rect.center().y;
                let c = if *muted {
                    palette::TEXT_SECONDARY
                } else {
                    palette::BTN_HOVER
                };

                // Speaker body (small rounded rect)
                let speaker =
                    egui::Rect::from_center_size(egui::pos2(cx - 3.0, cy), egui::vec2(4.0, 7.0));
                painter.rect_filled(speaker, 1.0, c);

                // Speaker cone (triangle)
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        egui::pos2(cx - 1.0, cy - 3.5),
                        egui::pos2(cx - 1.0, cy + 3.5),
                        egui::pos2(cx + 4.0, cy),
                    ],
                    c,
                    egui::Stroke::new(0.0, egui::Color32::TRANSPARENT),
                ));

                // Sound-wave arcs (only when unmuted)
                if !*muted && *volume > 0.0 {
                    let alpha_base = (40.0 + *volume * 180.0) as u8;
                    for i in 0..3 {
                        let r = 5.0 + i as f32 * 2.5;
                        let alpha = alpha_base.saturating_sub(i as u8 * 60);
                        painter.circle_stroke(
                            egui::pos2(cx + 4.0, cy),
                            r,
                            egui::Stroke::new(
                                1.2,
                                egui::Color32::from_rgba_premultiplied(c.r(), c.g(), c.b(), alpha),
                            ),
                        );
                    }
                }
            }
            if icon_response.clicked() {
                *muted = !*muted;
            }

            // Slider track
            let slider_w = 80.0;
            let slider_h = 6.0;
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(slider_w, slider_h), Sense::click_and_drag());
            let painter = ui.painter_at(rect);

            // Background track
            painter.rect_filled(rect, 3.0, palette::VOLUME_SLIDER_BG);

            // Filled portion
            let effective = if *muted { 0.0 } else { *volume };
            if effective > 0.0 {
                let fill = egui::Rect::from_min_size(
                    rect.min,
                    egui::vec2(rect.width() * effective, slider_h),
                );
                painter.rect_filled(fill, 3.0, palette::VOLUME_SLIDER_FILL);
            }

            // Thumb knob with ring
            let thumb_x = rect.min.x + rect.width() * effective;
            let thumb_center = egui::pos2(thumb_x, rect.center().y);
            painter.circle_filled(thumb_center, 5.0, palette::BTN_HOVER);
            let ring_color = if *muted {
                palette::TEXT_SECONDARY
            } else {
                palette::BTN_HOVER
            };
            painter.circle_stroke(thumb_center, 5.0, egui::Stroke::new(1.5, ring_color));

            // Interaction
            if let Some(mouse_pos) = response.interact_pointer_pos() {
                let rel = ((mouse_pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
                *volume = rel;
                *muted = false;
            }
        });
    }

    fn draw_fullscreen_button(&self, ui: &mut Ui) -> bool {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), Sense::click());
        let painter = ui.painter_at(rect);
        let color = if response.hovered() {
            palette::BTN_HOVER
        } else {
            palette::BTN_DEFAULT
        };
        let cx = rect.center().x;
        let cy = rect.center().y;
        // Fullscreen: small square with outward corner brackets
        let s = egui::Rect::from_center_size(egui::pos2(cx, cy), egui::vec2(9.0, 9.0));
        painter.rect_stroke(s, 0.0, egui::Stroke::new(1.5, color));
        // Top-left corner extension
        painter.line_segment(
            [
                egui::pos2(s.min.x - 2.0, s.min.y),
                egui::pos2(s.min.x - 2.0, s.min.y - 2.0),
            ],
            egui::Stroke::new(1.5, color),
        );
        painter.line_segment(
            [
                egui::pos2(s.min.x, s.min.y - 2.0),
                egui::pos2(s.min.x - 2.0, s.min.y - 2.0),
            ],
            egui::Stroke::new(1.5, color),
        );
        response.clicked()
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
