use eframe::egui;
use eframe::Frame;

use crate::client::ServerClient;
use crate::player::PlayerPanel;
use crate::playlist::PlaylistManager;
use crate::settings::AppSettings;
use crate::skin::{palette, Qvod6Skin, SkinEngine, TaskEntry, TaskStatus, TitleBarAction};
use crate::status::StatusPanel;
use crate::theme::QvodTheme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppPage {
    Player,
    Settings,
    Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerState {
    Stopped,
    Playing,
    Paused,
    Buffering,
    Error(String),
    Ended,
}

pub struct QvodApp {
    pub settings: AppSettings,
    pub player: PlayerPanel,
    pub playlist: PlaylistManager,
    pub status: StatusPanel,
    pub theme: QvodTheme,
    pub skin: Box<dyn SkinEngine>,
    pub server_client: Option<ServerClient>,
    page: AppPage,
    player_state: PlayerState,
}

impl QvodApp {
    #[must_use]
    pub fn new(startup_uri: Option<String>, cli_server_url: Option<String>) -> Self {
        let settings = AppSettings::load().with_cli_server_url(cli_server_url);
        let server_client = settings
            .server_url
            .as_ref()
            .map(|url| ServerClient::new(url.clone()));
        let player = PlayerPanel::new();
        let mut app = Self {
            settings,
            player,
            playlist: PlaylistManager::new(),
            status: StatusPanel::new(),
            theme: QvodTheme::Dark,
            skin: Box::new(Qvod6Skin::new()),
            server_client,
            page: AppPage::Player,
            player_state: PlayerState::Stopped,
        };

        if let Some(uri) = startup_uri {
            let name = uri.split('|').nth(1).unwrap_or(&uri).to_string();
            app.play_uri(&uri, &name);
        }

        app
    }

    pub fn set_page(&mut self, page: AppPage) {
        self.page = page;
    }

    #[must_use]
    pub fn page(&self) -> AppPage {
        self.page
    }

    pub fn play_uri(&mut self, uri: &str, title: &str) {
        self.playlist.add(TaskEntry {
            uri: uri.into(),
            title: title.into(),
            status: TaskStatus::Downloading,
            ..Default::default()
        });
        self.player_state = PlayerState::Buffering;
    }

    pub fn on_keypress(&mut self, key: egui::Key) {
        match key {
            egui::Key::Space => {
                self.player.controls.toggle_play();
                self.player_state = if self.player.controls.playing {
                    PlayerState::Playing
                } else {
                    PlayerState::Paused
                };
            }
            egui::Key::ArrowLeft => {
                self.player.controls.seek_backward(10000);
            }
            egui::Key::ArrowRight => {
                self.player.controls.seek_forward(10000);
            }
            egui::Key::Escape => {
                self.player_state = PlayerState::Stopped;
            }
            _ => {}
        }
    }

    #[must_use]
    pub fn player_state(&self) -> PlayerState {
        self.player_state.clone()
    }
}

impl eframe::App for QvodApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        self.theme.apply(ctx);

        // ── Frameless window resize (drag edges/corners) ───────────
        // Since the window uses custom decorations (no native frame),
        // we must detect mouse-at-edge and initiate OS resize manually.
        {
            const RESIZE_MARGIN: f32 = 8.0;
            let screen = ctx.screen_rect();
            if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                let near_left = pos.x <= screen.min.x + RESIZE_MARGIN;
                let near_right = pos.x >= screen.max.x - RESIZE_MARGIN;
                let near_top = pos.y <= screen.min.y + RESIZE_MARGIN;
                let near_bottom = pos.y >= screen.max.y - RESIZE_MARGIN;

                // Map the 4 booleans to a ResizeDirection; corners win.
                let dir: Option<egui::viewport::ResizeDirection> =
                    match (near_left, near_right, near_top, near_bottom) {
                        (true, _, true, _) => Some(egui::viewport::ResizeDirection::NorthWest),
                        (true, _, _, true) => Some(egui::viewport::ResizeDirection::SouthWest),
                        (_, true, true, _) => Some(egui::viewport::ResizeDirection::NorthEast),
                        (_, true, _, true) => Some(egui::viewport::ResizeDirection::SouthEast),
                        (true, _, _, _) => Some(egui::viewport::ResizeDirection::West),
                        (_, true, _, _) => Some(egui::viewport::ResizeDirection::East),
                        (_, _, true, _) => Some(egui::viewport::ResizeDirection::North),
                        (_, _, _, true) => Some(egui::viewport::ResizeDirection::South),
                        _ => None,
                    };

                if let Some(direction) = dir {
                    // Set the appropriate OS resize cursor.
                    let cursor = match direction {
                        egui::viewport::ResizeDirection::North
                        | egui::viewport::ResizeDirection::South => {
                            egui::CursorIcon::ResizeVertical
                        }
                        egui::viewport::ResizeDirection::East
                        | egui::viewport::ResizeDirection::West => {
                            egui::CursorIcon::ResizeHorizontal
                        }
                        egui::viewport::ResizeDirection::NorthEast
                        | egui::viewport::ResizeDirection::SouthWest => {
                            egui::CursorIcon::ResizeNeSw
                        }
                        egui::viewport::ResizeDirection::NorthWest
                        | egui::viewport::ResizeDirection::SouthEast => {
                            egui::CursorIcon::ResizeNwSe
                        }
                    };
                    ctx.set_cursor_icon(cursor);

                    // Start native OS resize on primary click.
                    if ctx.input(|i| i.pointer.primary_clicked()) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
                    }
                }
            }
        }

        let title_bar_action = egui::TopBottomPanel::top("title_bar")
            .frame(egui::Frame::none().fill(egui::Color32::TRANSPARENT))
            .show(ctx, |ui| self.skin.draw_title_bar(ui, "QVOD Player"))
            .inner;
        match title_bar_action {
            TitleBarAction::Close => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            TitleBarAction::Maximize => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            }
            TitleBarAction::Minimize => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
            TitleBarAction::Drag => {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            TitleBarAction::None => {}
        }

        let (space, right, left, escape) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Space),
                i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if space {
            self.on_keypress(egui::Key::Space);
        }
        if right {
            self.on_keypress(egui::Key::ArrowRight);
        }
        if left {
            self.on_keypress(egui::Key::ArrowLeft);
        }
        if escape {
            self.on_keypress(egui::Key::Escape);
        }

        // ── Zoom keyboard shortcuts (Ctrl + = / - / 0) ──────────────
        let zoom_in = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Plus));
        let zoom_out = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Minus));
        let zoom_reset = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Num0));

        if zoom_in {
            self.player.zoom_in();
        }
        if zoom_out {
            self.player.zoom_out();
        }
        if zoom_reset {
            self.player.reset_zoom();
        }

        // ── Fullscreen keyboard shortcut ────────────────────────────
        if ctx.input(|i| i.key_pressed(egui::Key::F11)) {
            self.player.controls.fullscreen = !self.player.controls.fullscreen;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(
                self.player.controls.fullscreen,
            ));
        }

        egui::TopBottomPanel::top("menu_bar")
            .frame(egui::Frame::none().fill(palette::CONTROL_BAR_BG))
            .show(ctx, |ui| {
                egui::menu::bar(ui, |ui| {
                    ui.menu_button("文件", |ui| {
                        if ui.button("打开文件...").clicked() {
                            ui.close_menu();
                        }
                        if ui.button("打开 URL...").clicked() {
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("退出").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("播放", |ui| {
                        if ui.button("播放").clicked() {
                            ui.close_menu();
                        }
                        if ui.button("暂停").clicked() {
                            ui.close_menu();
                        }
                        if ui.button("停止").clicked() {
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("全屏 (F11)").clicked() {
                            self.player.controls.fullscreen = !self.player.controls.fullscreen;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(
                                self.player.controls.fullscreen,
                            ));
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("控制", |ui| {
                        ui.menu_button("画面比例", |ui| {
                            for &ar in crate::player::AspectRatio::variants() {
                                let label = if ar == self.player.aspect_ratio {
                                    format!("✓ {}", ar.label())
                                } else {
                                    ar.label().to_string()
                                };
                                if ui.button(label).clicked() {
                                    self.player.aspect_ratio = ar;
                                    ui.close_menu();
                                }
                            }
                        });
                        ui.menu_button("缩放", |ui| {
                            if ui.button("放大 (Ctrl+=)").clicked() {
                                self.player.zoom_in();
                                ui.close_menu();
                            }
                            if ui.button("缩小 (Ctrl+-)").clicked() {
                                self.player.zoom_out();
                                ui.close_menu();
                            }
                            if ui.button("适应窗口 (Ctrl+0)").clicked() {
                                self.player.reset_zoom();
                                ui.close_menu();
                            }
                        });
                    });
                    ui.menu_button("设置", |ui| {
                        if ui.button("偏好设置...").clicked() {
                            self.set_page(AppPage::Settings);
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("帮助", |ui| {
                        if ui.button("关于 QVOD").clicked() {
                            ui.close_menu();
                        }
                    });
                });
            });

        egui::TopBottomPanel::top("nav_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.page, AppPage::Player, "▶ Player");
                ui.selectable_value(&mut self.page, AppPage::Settings, "⚙ Settings");
                ui.selectable_value(&mut self.page, AppPage::Status, "📊 Status");
            });
        });

        egui::SidePanel::right("task_list")
            .resizable(true)
            .default_width(300.0)
            .min_width(280.0)
            .max_width(400.0)
            .frame(egui::Frame::none().fill(palette::SIDEBAR_BG))
            .show(ctx, |ui| {
                self.playlist.ui(ui, &*self.skin);
            });

        let video_area: egui::Rect = match self.page {
            AppPage::Player => {
                egui::CentralPanel::default()
                    .show(ctx, |ui| self.player.ui(ui, &self.player_state))
                    .inner
            }
            AppPage::Settings => {
                egui::CentralPanel::default().show(ctx, |ui| self.settings.ui(ui, &mut self.theme));
                egui::Rect::NOTHING
            }
            AppPage::Status => {
                egui::CentralPanel::default().show(ctx, |ui| self.status.ui(ui));
                egui::Rect::NOTHING
            }
        };

        egui::TopBottomPanel::bottom("controls")
            .min_height(48.0)
            .frame(egui::Frame::none().fill(palette::CONTROL_BAR_BG))
            .show(ctx, |ui| {
                self.player.controls.ui(ui, &*self.skin);
            });

        if self.player.controls.playing && self.player_state == PlayerState::Paused {
            self.player_state = PlayerState::Playing;
        }
        if !self.player.controls.playing && self.player_state == PlayerState::Playing {
            self.player_state = PlayerState::Paused;
        }

        if self.page == AppPage::Player {
            let time = ctx.input(|i| i.time);
            self.player
                .overlay
                .draw(ctx, &self.player_state, &*self.skin, video_area, time);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_creation() {
        let app = QvodApp::new(None, None);
        assert_eq!(app.page(), AppPage::Player);
        assert_eq!(app.player_state(), PlayerState::Stopped);
    }

    #[test]
    fn test_page_navigation() {
        let mut app = QvodApp::new(None, None);
        app.set_page(AppPage::Settings);
        assert_eq!(app.page(), AppPage::Settings);
    }

    #[test]
    fn test_play_uri() {
        let mut app = QvodApp::new(None, None);
        app.play_uri("qvod://hash|test.mp4|1024|mp4|", "Test Video");
        assert_eq!(app.player_state(), PlayerState::Buffering);
        assert_eq!(app.playlist.len(), 1);
    }

    #[test]
    fn test_keyboard_shortcuts() {
        let mut app = QvodApp::new(None, None);
        app.on_keypress(egui::Key::Space);
        assert_eq!(app.player_state(), PlayerState::Playing);
        app.on_keypress(egui::Key::Space);
        assert_eq!(app.player_state(), PlayerState::Paused);
    }
}
