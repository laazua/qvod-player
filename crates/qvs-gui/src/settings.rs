use std::path::PathBuf;

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::theme::QvodTheme;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub cache_dir: PathBuf,
    pub cache_size_gb: u32,
    pub local_server_port: u16,
    pub server_url: Option<String>,
    pub max_connections: u32,
    pub http_fallback: bool,
    pub tracker_urls: Vec<String>,
    pub dht_seed_nodes: Vec<String>,
    pub language: String,
    pub theme: String,
    pub playlist_history: Vec<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        // Allow baking in a server URL at compile/packaging time via QVS_SERVER_URL env var.
        let baked_server_url = option_env!("QVS_SERVER_URL").map(String::from);
        Self {
            cache_dir: dirs::cache_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join("qvs"),
            cache_size_gb: 10,
            local_server_port: 8621,
            server_url: baked_server_url,
            max_connections: 50,
            http_fallback: true,
            tracker_urls: vec!["http://tracker.qvod.com:8621/announce".into()],
            dht_seed_nodes: vec!["router.bittorrent.com:6881".into()],
            language: "zh-CN".into(),
            theme: "dark".into(),
            playlist_history: Vec::new(),
        }
    }
}

impl AppSettings {
    /// Override server_url from CLI argument (takes precedence over baked-in value).
    pub fn with_cli_server_url(mut self, cli_server_url: Option<String>) -> Self {
        if let Some(url) = cli_server_url {
            self.server_url = Some(url);
        }
        self
    }
}

impl AppSettings {
    #[must_use]
    pub fn load() -> Self {
        let path = Self::config_path();
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(settings) = toml::from_str(&data) {
                return settings;
            }
        }
        let settings = Self::default();
        let _ = settings.save();
        settings
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let data = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, data).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("qvs")
            .join("settings.toml")
    }

    #[must_use]
    pub fn cache_size_bytes(&self) -> u64 {
        u64::from(self.cache_size_gb) * 1024 * 1024 * 1024
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, theme: &mut QvodTheme) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("Settings");
            ui.separator();

            egui::Grid::new("settings_grid")
                .num_columns(2)
                .spacing([16.0, 8.0])
                .show(ui, |ui| {
                    let cache_str = &mut self.cache_dir.to_string_lossy().to_string();

                    ui.label("Cache Directory:");
                    ui.add(egui::TextEdit::singleline(cache_str).desired_width(300.0));
                    if let Ok(pb) = std::path::PathBuf::from(cache_str.clone()).canonicalize() {
                        self.cache_dir = pb;
                    }
                    ui.end_row();

                    ui.label("Cache Size (GB):");
                    ui.add(egui::Slider::new(&mut self.cache_size_gb, 1..=1000));
                    ui.end_row();

                    ui.label("HTTP Port:");
                    ui.add(egui::Slider::new(&mut self.local_server_port, 1024..=65535));
                    ui.end_row();

                    ui.label("Server URL:");
                    let mut server_url_str = self.server_url.clone().unwrap_or_default();
                    let resp = ui.add(egui::TextEdit::singleline(&mut server_url_str).desired_width(300.0).hint_text("http://server:8621 (留空=本地模式)"));
                    if resp.changed() {
                        if server_url_str.is_empty() {
                            self.server_url = None;
                        } else {
                            self.server_url = Some(server_url_str);
                        }
                    }
                    ui.end_row();

                    ui.label("Max Connections:");
                    ui.add(egui::Slider::new(&mut self.max_connections, 10..=200));
                    ui.end_row();

                    ui.label("HTTP Fallback:");
                    ui.checkbox(&mut self.http_fallback, "Enable HTTP source fallback");
                    ui.end_row();

                    let tracker_str = self.tracker_urls.join("\n");
                    let mut tracker_buf = tracker_str.clone();
                    ui.label("Tracker URLs:");
                    ui.add(
                        egui::TextEdit::multiline(&mut tracker_buf)
                            .desired_rows(3)
                            .desired_width(300.0),
                    );
                    self.tracker_urls = tracker_buf.lines().map(String::from).collect();
                    ui.end_row();

                    let dht_str = self.dht_seed_nodes.join("\n");
                    let mut dht_buf = dht_str.clone();
                    ui.label("DHT Seed Nodes:");
                    ui.add(
                        egui::TextEdit::multiline(&mut dht_buf)
                            .desired_rows(3)
                            .desired_width(300.0),
                    );
                    self.dht_seed_nodes = dht_buf.lines().map(String::from).collect();
                    ui.end_row();

                    ui.label("Language:");
                    egui::ComboBox::from_id_source("lang")
                        .selected_text(&self.language)
                        .show_ui(ui, |ui: &mut egui::Ui| {
                            ui.selectable_value(&mut self.language, "en-US".into(), "English");
                            ui.selectable_value(&mut self.language, "zh-CN".into(), "中文");
                        });
                    ui.end_row();

                    ui.label("Theme:");
                    let theme_str = format!("{theme:?}");
                    egui::ComboBox::from_id_source("theme")
                        .selected_text(&theme_str)
                        .show_ui(ui, |ui: &mut egui::Ui| {
                            ui.selectable_value(theme, QvodTheme::Dark, "Dark");
                            ui.selectable_value(theme, QvodTheme::Light, "Light");
                            ui.selectable_value(theme, QvodTheme::System, "System");
                        });
                    ui.end_row();
                });

            ui.separator();
            if ui.button("💾 Save Settings").clicked() {
                let _ = self.save();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings() {
        let settings = AppSettings::default();
        assert_eq!(settings.max_connections, 50);
        assert!(settings.http_fallback);
    }

    #[test]
    fn test_cache_size_calculation() {
        let settings = AppSettings {
            cache_size_gb: 5,
            ..Default::default()
        };
        assert_eq!(settings.cache_size_bytes(), 5 * 1024 * 1024 * 1024);
    }

    #[test]
    fn test_serialize_roundtrip() {
        let settings = AppSettings::default();
        let serialized = toml::to_string_pretty(&settings).unwrap();
        let deserialized: AppSettings = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.max_connections, settings.max_connections);
    }
}
