use std::sync::mpsc;

use eframe::egui;
use eframe::Frame;

use qvs_local_server::{LocalServer, LocalServerConfig};
use qvs_media::FrameReader;
use qvs_stream::{EngineConfig, QvodEngine};

use crate::client::{ServerClient, StreamStatusResponse};
use crate::player::PlayerPanel;
use crate::playlist::PlaylistManager;
use crate::settings::AppSettings;
use crate::skin::{palette, Qvod6Skin, SkinEngine, TaskEntry, TaskStatus, TitleBarAction};
use crate::status::{NetworkStatus, StatusPanel};
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

    // Server-client mode state
    current_hash: Option<String>,
    pending_play_hash: Option<String>,
    pending_play_url: Option<String>,
    status_rx: mpsc::Receiver<Option<StreamStatusResponse>>,
    status_tx: mpsc::Sender<Option<StreamStatusResponse>>,

    // Error channel: async server operations send errors here for UI display
    error_rx: mpsc::Receiver<String>,
    error_tx: mpsc::Sender<String>,

    // Dialog state
    show_url_dialog: bool,
    url_input: String,

    // Embedded local server (standalone mode)
    _local_server: Option<LocalServer>,

    // Video decoder (ffmpeg-next native or subprocess fallback)
    frame_reader: Option<FrameReader>,
    current_file_path: Option<String>,
    video_texture: Option<egui::TextureHandle>,
}

impl QvodApp {
    #[must_use]
    pub fn new(startup_uri: Option<String>, cli_server_url: Option<String>) -> Self {
        tracing::info!(
            "QvodApp::new: startup_uri={:?}, cli_server_url={:?}",
            startup_uri,
            cli_server_url
        );
        let settings = AppSettings::load().with_cli_server_url(cli_server_url);
        let player = PlayerPanel::new();
        let (tx, rx) = mpsc::channel();
        let (error_tx, error_rx) = mpsc::channel();

        // Auto-start a local server when no remote server URL is configured
        let (server_client, local_server) = if let Some(url) = &settings.server_url {
            tracing::info!("QvodApp::new: using remote server: {}", url);
            (Some(ServerClient::new(url.clone())), None)
        } else {
            // Spawn a local engine + server in the background
            tracing::info!("QvodApp::new: starting local embedded server");
            let rt = tokio::runtime::Handle::try_current().expect("tokio runtime must be running");
            let (client, server) = rt.block_on(async { start_local_server(&settings).await });
            (client, Some(server))
        };

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
            current_hash: None,
            pending_play_hash: None,
            pending_play_url: None,
            status_rx: rx,
            status_tx: tx,
            error_rx,
            error_tx,
            show_url_dialog: false,
            url_input: String::new(),
            _local_server: local_server,
            frame_reader: None,
            current_file_path: None,
            video_texture: None,
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

    /// Extract the info_hash from a qvod:// URI, or return the URL itself for http(s)://.
    fn extract_hash(uri: &str) -> Option<String> {
        if uri.starts_with("http://") || uri.starts_with("https://") {
            return Some(uri.to_string());
        }
        uri.strip_prefix("qvod://")
            .and_then(|s| s.split('|').next())
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
    }

    pub fn play_uri(&mut self, uri: &str, title: &str) {
        tracing::info!("play_uri: uri={}, title={}", uri, title);
        let is_file = is_local_file_path(uri);
        let is_url =
            uri.starts_with("http://") || uri.starts_with("https://") || uri.starts_with("qvod://");

        let file_uri = if is_file {
            let normalized = if cfg!(windows) {
                uri.replace('\\', "/")
            } else {
                uri.to_string()
            };
            format!("file://{normalized}")
        } else {
            uri.to_string()
        };

        self.frame_reader = None;
        self.player.clear_video();
        self.current_file_path = None;

        // Start video decoder for local files and http(s) URLs.
        // In remote-server mode this avoids sending file:// paths to the
        // server where the file doesn't exist.  In standalone mode it
        // provides local rendering alongside the embedded engine.
        if is_file || is_url {
            let source = if is_file {
                if cfg!(windows) {
                    uri.replace("file://", "").replace("\\\\", "\\")
                } else {
                    uri.to_string()
                }
            } else {
                uri.to_string()
            };
            match FrameReader::open(&source) {
                Ok(reader) => {
                    tracing::info!(
                        "Started native decoder for {} ({}x{}, {:.1}fps)",
                        source,
                        reader.width(),
                        reader.height(),
                        reader.fps()
                    );
                    self.player
                        .set_video_dimensions(reader.width(), reader.height());
                    self.current_file_path = Some(source);
                    self.frame_reader = Some(reader);
                }
                Err(e) => {
                    tracing::warn!(
                        "Could not start native decoder (playback via server streaming): {e}"
                    );
                }
            }
        }

        let hash = if is_file {
            Some(file_uri.clone())
        } else {
            Self::extract_hash(uri)
        };
        self.current_hash = hash;

        self.playlist.add(TaskEntry {
            uri: file_uri.clone(),
            title: title.into(),
            status: TaskStatus::Downloading,
            ..Default::default()
        });
        self.player_state = PlayerState::Buffering;

        // Decide how to play: local decoder vs server.
        if self.server_client.is_some() {
            self.player.controls.playing = true;
            let is_remote = self.settings.server_url.is_some();
            if is_remote && self.frame_reader.is_some() {
                // Remote server mode with local decoder → play locally,
                // skip the server (file doesn't exist on the remote machine).
                tracing::info!("play_uri: playing locally via ffmpeg decoder");
                self.player_state = PlayerState::Playing;
            } else if is_remote && is_file {
                // Remote server mode + local file + decoder failed.
                // The server is on a different machine and cannot access
                // this file directly.
                let err_msg = "无法播放本地文件：解码失败。".into();
                tracing::error!("play_uri: {err_msg}");
                self.player_state = PlayerState::Error(err_msg);
                self.player.controls.playing = false;
            } else if is_file {
                tracing::info!("play_uri: sending file URL to server");
                self.pending_play_url = Some(file_uri);
            } else {
                tracing::info!("play_uri: sending hash to server: {:?}", self.current_hash);
                self.pending_play_hash = self.current_hash.clone();
            }
        }
    }

    pub fn on_keypress(&mut self, key: egui::Key) {
        tracing::info!("on_keypress: {:?}", key);
        match key {
            egui::Key::Space => {
                self.player.controls.toggle_play();
                self.player_state = if self.player.controls.playing {
                    tracing::info!("Keyboard: play");
                    PlayerState::Playing
                } else {
                    tracing::info!("Keyboard: pause");
                    PlayerState::Paused
                };
            }
            egui::Key::ArrowLeft => {
                tracing::info!("Keyboard: seek backward 10s");
                self.player.controls.seek_backward(10000);
            }
            egui::Key::ArrowRight => {
                tracing::info!("Keyboard: seek forward 10s");
                self.player.controls.seek_forward(10000);
            }
            egui::Key::Escape => {
                tracing::info!("Keyboard: stop");
                self.player_state = PlayerState::Stopped;
                self.player.controls.stop_pressed = true;
                self.player.controls.reset();
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

        // ── In server mode: send any pending play command ────────────
        if let Some(hash) = self.pending_play_hash.take() {
            if let Some(ref client) = self.server_client {
                let client = client.clone();
                let err_tx = self.error_tx.clone();
                tokio::spawn(async move {
                    if let Err(e) = client.play(&hash).await {
                        tracing::error!("server play failed: {e}");
                        let _ = err_tx.send(format!("播放失败: {e}"));
                    }
                });
            }
        }
        if let Some(url) = self.pending_play_url.take() {
            if let Some(ref client) = self.server_client {
                let client = client.clone();
                let err_tx = self.error_tx.clone();
                tokio::spawn(async move {
                    if let Err(e) = client.play_uri(&url).await {
                        tracing::error!("server play_uri failed: {e}");
                        let _ = err_tx.send(format!("播放失败: {e}"));
                    }
                });
            }
        }

        // Capture controls state BEFORE any UI handlers run this frame,
        // so we can detect user-initiated changes after controls.ui().
        let pre_playing = self.player.controls.playing;
        let pre_position = self.player.controls.position_ms;

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
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter(
                                    "媒体文件",
                                    &[
                                        "mp4", "avi", "mkv", "rmvb", "wmv", "flv", "mov", "ts",
                                        "webm", "m4v", "3gp",
                                    ],
                                )
                                .add_filter("QVOD 种子", &["qvs"])
                                .add_filter("所有文件", &["*"])
                                .pick_file()
                            {
                                let path_str = path.to_string_lossy().to_string();
                                let name = path
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or(&path_str)
                                    .to_string();
                                self.play_uri(&path_str, &name);
                            }
                        }
                        if ui.button("打开 URL...").clicked() {
                            self.show_url_dialog = true;
                            self.url_input.clear();
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

        // ── URL input dialog ──────────────────────────────────────────
        if self.show_url_dialog {
            let mut submitted = false;
            let mut closed = false;
            egui::Window::new("打开 URL")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .fixed_size([480.0, 140.0])
                .show(ctx, |ui| {
                    ui.label("请输入 qvod:// 或 http(s):// 链接：");
                    let resp = ui.add_sized(
                        [460.0, 28.0],
                        egui::TextEdit::singleline(&mut self.url_input).hint_text(
                            "qvod://<hash>|<name>|<size>|<fmt>| 或 https://example.com/video.mp4",
                        ),
                    );
                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.horizontal(|ui| {
                        if ui.button("确定").clicked() || enter {
                            submitted = true;
                        }
                        if ui.button("取消").clicked() {
                            closed = true;
                        }
                    });
                    resp.request_focus();
                });
            if submitted {
                let trimmed = self.url_input.trim().to_string();
                if !trimmed.is_empty() {
                    let name = trimmed.split('|').nth(1).unwrap_or(&trimmed).to_string();
                    self.play_uri(&trimmed, &name);
                }
                self.show_url_dialog = false;
            }
            if closed {
                self.show_url_dialog = false;
            }
        }

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

        // ── Sync controls state to remote server ─────────────────────
        if let Some(ref client) = self.server_client {
            let hash = self.current_hash.clone().unwrap_or_default();
            if !hash.is_empty() {
                // Stop (highest priority — resets everything)
                if self.player.controls.stop_pressed {
                    self.player.controls.stop_pressed = false;
                    let client = client.clone();
                    let h = hash.clone();
                    let err_tx = self.error_tx.clone();
                    tokio::spawn(async move {
                        if let Err(e) = client.stop(&h).await {
                            tracing::error!("server stop failed: {e}");
                            let _ = err_tx.send(format!("停止失败: {e}"));
                        }
                    });
                } else {
                    // Play / Pause toggle
                    if pre_playing && !self.player.controls.playing {
                        let client = client.clone();
                        let err_tx = self.error_tx.clone();
                        tokio::spawn(async move {
                            if let Err(e) = client.pause().await {
                                tracing::error!("server pause failed: {e}");
                                let _ = err_tx.send(format!("暂停失败: {e}"));
                            }
                        });
                    } else if !pre_playing && self.player.controls.playing {
                        let client = client.clone();
                        let err_tx = self.error_tx.clone();
                        tokio::spawn(async move {
                            if let Err(e) = client.resume().await {
                                tracing::error!("server resume failed: {e}");
                                let _ = err_tx.send(format!("恢复播放失败: {e}"));
                            }
                        });
                    }

                    // Seek (position changed by user via progress bar or arrow keys)
                    if self.player.controls.position_ms != pre_position {
                        let client = client.clone();
                        let pos = self.player.controls.position_ms;
                        let err_tx = self.error_tx.clone();
                        tokio::spawn(async move {
                            if let Err(e) = client.seek(&hash, pos).await {
                                tracing::error!("server seek failed: {e}");
                                let _ = err_tx.send(format!("拖拽失败: {e}"));
                            }
                        });
                    }
                }
            }
        }

        // ── Process errors from async server operations ──────────────
        while let Ok(err_msg) = self.error_rx.try_recv() {
            tracing::error!("Server operation error, showing popup: {err_msg}");
            self.player_state = PlayerState::Error(err_msg);
            self.player.controls.playing = false;
        }

        // ── Process status responses from server ─────────────────────
        while let Ok(status) = self.status_rx.try_recv() {
            match status {
                Some(s) => {
                    tracing::debug!("Status update: state={}, pos={}ms, dur={}ms, buffered={:.1}s, progress={:.1}%",
                        s.state, s.position_ms, s.duration_ms, s.buffered_seconds, s.download_progress * 100.0);
                    self.player.controls.position_ms = s.position_ms;
                    self.player.controls.duration_ms = s.duration_ms;
                    self.player.controls.buffered_seconds = s.buffered_seconds;

                    // Drive state transitions from server stream state
                    match s.state.as_str() {
                        "Playing" => {
                            if self.player_state == PlayerState::Buffering {
                                tracing::info!("Status: buffering -> playing");
                                self.player_state = PlayerState::Playing;
                                self.player.controls.playing = true;
                            }
                        }
                        "Ended" => {
                            if self.player_state != PlayerState::Stopped {
                                tracing::info!("Status: stream ended");
                                self.player_state = PlayerState::Ended;
                                self.player.controls.playing = false;
                            }
                        }
                        "Paused" => {
                            if self.player_state == PlayerState::Playing {
                                tracing::info!("Status: stream paused by server");
                                self.player_state = PlayerState::Paused;
                                self.player.controls.playing = false;
                            }
                        }
                        "Error" => {
                            if self.player_state != PlayerState::Stopped {
                                let err_msg = "服务器流错误".into();
                                tracing::error!("Status: stream error from server");
                                self.player_state = PlayerState::Error(err_msg);
                                self.player.controls.playing = false;
                            }
                        }
                        _ => {}
                    }

                    self.status.update(NetworkStatus {
                        buffer_progress: if s.duration_ms > 0 {
                            (s.buffered_seconds / (s.duration_ms as f64 / 1000.0)).min(1.0)
                        } else {
                            s.download_progress
                        },
                        download_progress: s.download_progress,
                        connected_peers: s.peer_count,
                        server_url: self.settings.server_url.clone(),
                        server_connected: true,
                        ..Default::default()
                    });
                }
                None => {
                    tracing::warn!("Status: server returned no status");
                    self.status.update(NetworkStatus {
                        server_url: self.settings.server_url.clone(),
                        server_connected: false,
                        ..Default::default()
                    });
                }
            }
        }

        // ── Periodically poll server status ──────────────────────────
        if let Some(ref client) = self.server_client {
            if let Some(ref hash) = self.current_hash {
                if self.status.needs_update() {
                    let client = client.clone();
                    let h = hash.clone();
                    let tx = self.status_tx.clone();
                    tokio::spawn(async move {
                        match client.get_status(&h).await {
                            Ok(status) => {
                                let _ = tx.send(Some(status));
                            }
                            Err(e) => {
                                tracing::warn!("Status poll failed: {e}");
                                let _ = tx.send(None);
                            }
                        }
                    });
                }
            }
        }

        if self.player.controls.playing && self.player_state == PlayerState::Paused {
            self.player_state = PlayerState::Playing;
        }
        if !self.player.controls.playing && self.player_state == PlayerState::Playing {
            self.player_state = PlayerState::Paused;
        }

        // ── Video frame rendering via native decoder ──────────
        if let Some(ref mut reader) = self.frame_reader {
            match self.player_state {
                PlayerState::Playing => {
                    match reader.try_read_frame() {
                        Ok(Some(frame_data)) => {
                            let w = reader.width() as usize;
                            let h = reader.height() as usize;
                            if w > 0 && h > 0 && frame_data.len() >= w * h * 3 {
                                let color_image =
                                    egui::ColorImage::from_rgb([w, h], &frame_data[..w * h * 3]);
                                let new_texture = ctx.load_texture(
                                    "video_frame",
                                    color_image,
                                    egui::TextureOptions::default(),
                                );
                                self.player.set_video_texture(new_texture.id());
                                self.video_texture = Some(new_texture);
                            }
                        }
                        Ok(None) => {
                            // No frame available yet — keep current display
                        }
                        Err(_) => {
                            // Decoder ended or error
                            self.frame_reader = None;
                            self.video_texture = None;
                            self.player.clear_video();
                            if self.player_state == PlayerState::Playing {
                                self.player_state = PlayerState::Ended;
                                self.player.controls.playing = false;
                            }
                        }
                    }
                }
                PlayerState::Paused | PlayerState::Buffering => {
                    // Keep current frame displayed
                }
                PlayerState::Stopped | PlayerState::Ended | PlayerState::Error(_) => {
                    self.frame_reader = None;
                    self.video_texture = None;
                    self.player.clear_video();
                }
            }
        }

        // Periodically request repaint while playing to update video frames
        if self.player_state == PlayerState::Playing && self.frame_reader.is_some() {
            ctx.request_repaint();
        }

        if self.page == AppPage::Player {
            let time = ctx.input(|i| i.time);
            self.player
                .overlay
                .draw(ctx, &self.player_state, &*self.skin, video_area, time);
        }
    }
}

/// Start an embedded QVOD engine + local HTTP server on localhost.
/// Returns a (ServerClient, LocalServer) tuple.
async fn start_local_server(settings: &AppSettings) -> (Option<ServerClient>, LocalServer) {
    tracing::info!(
        "start_local_server: port={}, cache_dir={:?}",
        settings.local_server_port,
        settings.cache_dir
    );

    let engine_config = EngineConfig {
        cache_dir: settings.cache_dir.clone(),
        tracker_enabled: false,
        dht_enabled: false,
        cache_enabled: false,
        ..Default::default()
    };

    tracing::info!("start_local_server: creating QvodEngine");
    let engine = QvodEngine::new(engine_config).await;

    let server_config = LocalServerConfig::new(settings.local_server_port)
        .with_bind_address(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));

    tracing::info!("start_local_server: starting LocalServer");
    match LocalServer::new(&server_config, engine).await {
        Ok(server) => {
            let port: u16 = server.port();
            let client = ServerClient::new(format!("http://127.0.0.1:{port}"));
            tracing::info!("start_local_server: embedded server started on port {port}");
            (Some(client), server)
        }
        Err(e) => {
            tracing::error!("start_local_server: failed: {e}, trying fallback port");
            let fallback_config = LocalServerConfig::new(0)
                .with_bind_address(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
            let fallback_engine = QvodEngine::new(EngineConfig::default()).await;
            let fallback_server = LocalServer::new(&fallback_config, fallback_engine)
                .await
                .unwrap_or_else(|e| panic!("start_local_server: fallback also failed: {e}"));
            tracing::info!("start_local_server: fallback server started");
            (None, fallback_server)
        }
    }
}

/// Check if a string looks like a local file path (not a URI scheme).
fn is_local_file_path(s: &str) -> bool {
    if s.starts_with("http://")
        || s.starts_with("https://")
        || s.starts_with("qvod://")
        || s.starts_with("qvod:")
        || s.starts_with("file://")
    {
        return false;
    }
    // Absolute paths
    s.starts_with('/')
        || s.starts_with("\\\\")
        || s.as_bytes().first().is_some_and(|b| {
            b.is_ascii_alphabetic()
                && s.len() > 2
                && s.as_bytes()[1] == b':'
                && (s.as_bytes().get(2) == Some(&b'\\') || s.as_bytes().get(2) == Some(&b'/'))
        })
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
