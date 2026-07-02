use eframe::egui;
use eframe::Frame;

use crate::player::PlayerPanel;
use crate::playlist::PlaylistManager;
use crate::settings::AppSettings;
use crate::status::StatusPanel;
use crate::theme::QvodTheme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppPage {
    Player,
    Playlist,
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
    page: AppPage,
    player_state: PlayerState,
}

impl QvodApp {
    #[must_use]
    pub fn new(startup_uri: Option<String>) -> Self {
        let settings = AppSettings::load();
        let player = PlayerPanel::new();
        let mut app = Self {
            settings,
            player,
            playlist: PlaylistManager::new(),
            status: StatusPanel::new(),
            theme: QvodTheme::Dark,
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
        self.playlist.add(crate::playlist::PlaylistEntry::new(
            uri.into(),
            title.into(),
        ));
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

        egui::TopBottomPanel::top("nav_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.page, AppPage::Player, "▶ Player");
                ui.selectable_value(&mut self.page, AppPage::Playlist, "☰ Playlist");
                ui.selectable_value(&mut self.page, AppPage::Settings, "⚙ Settings");
                ui.selectable_value(&mut self.page, AppPage::Status, "📊 Status");
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.page {
            AppPage::Player => self.player.ui(ui, &self.player_state),
            AppPage::Playlist => self.playlist.ui(ui),
            AppPage::Settings => self.settings.ui(ui, &mut self.theme),
            AppPage::Status => self.status.ui(ui),
        });

        egui::TopBottomPanel::bottom("controls").show(ctx, |ui| {
            self.player.controls.ui(ui);
        });

        if self.player.controls.playing && self.player_state == PlayerState::Paused {
            self.player_state = PlayerState::Playing;
        }
        if !self.player.controls.playing && self.player_state == PlayerState::Playing {
            self.player_state = PlayerState::Paused;
        }

        self.player.overlay.draw(ctx, &self.player_state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_creation() {
        let app = QvodApp::new(None);
        assert_eq!(app.page(), AppPage::Player);
        assert_eq!(app.player_state(), PlayerState::Stopped);
    }

    #[test]
    fn test_page_navigation() {
        let mut app = QvodApp::new(None);
        app.set_page(AppPage::Settings);
        assert_eq!(app.page(), AppPage::Settings);
    }

    #[test]
    fn test_play_uri() {
        let mut app = QvodApp::new(None);
        app.play_uri("qvod://hash|test.mp4|1024|mp4|", "Test Video");
        assert_eq!(app.player_state(), PlayerState::Buffering);
        assert_eq!(app.playlist.len(), 1);
    }

    #[test]
    fn test_keyboard_shortcuts() {
        let mut app = QvodApp::new(None);
        app.on_keypress(egui::Key::Space);
        assert_eq!(app.player_state(), PlayerState::Playing);
        app.on_keypress(egui::Key::Space);
        assert_eq!(app.player_state(), PlayerState::Paused);
    }
}
