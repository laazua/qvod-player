use std::time::{Duration, Instant};

use eframe::egui;

#[derive(Debug, Clone)]
pub struct NetworkStatus {
    pub download_speed: f64,
    pub upload_speed: f64,
    pub connected_peers: usize,
    pub buffer_progress: f64,
    pub download_progress: f64,
    pub dht_table_size: usize,
    pub active_connections: Vec<PeerConnectionInfo>,
    /// If connected to a remote server, the server URL.
    pub server_url: Option<String>,
    pub server_connected: bool,
}

impl Default for NetworkStatus {
    fn default() -> Self {
        Self {
            download_speed: 0.0,
            upload_speed: 0.0,
            connected_peers: 0,
            buffer_progress: 0.0,
            download_progress: 0.0,
            dht_table_size: 0,
            active_connections: Vec::new(),
            server_url: None,
            server_connected: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PeerConnectionInfo {
    pub peer_id: String,
    pub addr: String,
    pub speed_down: f64,
    pub speed_up: f64,
    pub rtt: Duration,
}

pub struct StatusPanel {
    status: NetworkStatus,
    last_update: Instant,
    update_interval: Duration,
}

impl StatusPanel {
    #[must_use]
    pub fn new() -> Self {
        Self {
            status: NetworkStatus::default(),
            last_update: Instant::now(),
            update_interval: Duration::from_secs(1),
        }
    }

    pub fn update(&mut self, new_status: NetworkStatus) {
        self.status = new_status;
        self.last_update = Instant::now();
    }

    #[must_use]
    pub fn needs_update(&self) -> bool {
        self.last_update.elapsed() >= self.update_interval
    }

    #[must_use]
    pub fn status(&self) -> &NetworkStatus {
        &self.status
    }

    pub fn reset(&mut self) {
        self.status = NetworkStatus::default();
        self.last_update = Instant::now();
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Network Status");
        ui.separator();

        egui::Grid::new("status_grid")
            .num_columns(2)
            .spacing([16.0, 6.0])
            .show(ui, |ui| {
                // Server mode indicator
                if let Some(ref server_url) = self.status.server_url {
                    ui.label("Server Mode:");
                    let color = if self.status.server_connected {
                        egui::Color32::GREEN
                    } else {
                        egui::Color32::RED
                    };
                    ui.colored_label(
                        color,
                        format!(
                            "{} {}",
                            server_url,
                            if self.status.server_connected {
                                "✓"
                            } else {
                                "✗"
                            }
                        ),
                    );
                    ui.end_row();
                } else {
                    ui.label("Mode:");
                    ui.label("Local Engine");
                    ui.end_row();
                }

                ui.label("Download Speed:");
                ui.colored_label(
                    if self.status.download_speed > 1_000_000.0 {
                        egui::Color32::GREEN
                    } else {
                        egui::Color32::WHITE
                    },
                    format!("{:.1} KB/s", self.status.download_speed / 1024.0),
                );
                ui.end_row();

                ui.label("Upload Speed:");
                ui.colored_label(
                    if self.status.upload_speed > 100_000.0 {
                        egui::Color32::GREEN
                    } else {
                        egui::Color32::WHITE
                    },
                    format!("{:.1} KB/s", self.status.upload_speed / 1024.0),
                );
                ui.end_row();

                ui.label("Connected Peers:");
                ui.label(format!("{}", self.status.connected_peers));
                ui.end_row();

                ui.label("Buffer:");
                ui.label(format!("{:.1}%", self.status.buffer_progress * 100.0));
                ui.end_row();

                ui.label("Download Progress:");
                ui.label(format!("{:.1}%", self.status.download_progress * 100.0));
                ui.end_row();

                ui.label("DHT Table Size:");
                ui.label(format!("{}", self.status.dht_table_size));
                ui.end_row();

                ui.label("Active Connections:");
                ui.label(format!("{}", self.status.active_connections.len()));
                ui.end_row();
            });
    }
}

impl Default for StatusPanel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_status() {
        let panel = StatusPanel::new();
        assert_eq!(panel.status().download_speed, 0.0);
    }

    #[test]
    fn test_update_status() {
        let mut panel = StatusPanel::new();
        let ns = NetworkStatus {
            download_speed: 1_000_000.0,
            connected_peers: 5,
            ..Default::default()
        };
        panel.update(ns);
        assert_eq!(panel.status().download_speed, 1_000_000.0);
        assert_eq!(panel.status().connected_peers, 5);
    }

    #[test]
    fn test_reset() {
        let mut panel = StatusPanel::new();
        panel.update(NetworkStatus {
            download_speed: 500_000.0,
            ..Default::default()
        });
        panel.reset();
        assert_eq!(panel.status().download_speed, 0.0);
    }
}
