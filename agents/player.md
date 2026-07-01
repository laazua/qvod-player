# Player Module Specification

## Overview

The Player module provides the end-user interface for the QVOD P2SP streaming system. It operates in two modes: **GUI mode** (powered by `egui`) for desktop users and **CLI mode** for headless or scripted usage. The player consumes media streams from the `qvs-stream` engine, decodes them via `qvs-media` (ffmpeg-next bindings), and renders video/audio to the user.

**Crate:** `qvs-gui` (GUI) and `qvs-cli` (command-line)

**Dependencies:**
- `qvs-stream` — stream engine for playback control
- `qvs-media` — ffmpeg-next demuxing/decoding/rendering
- `qvs-core` — shared types and errors
- `qvs-format` — URI parsing and cache management
- egui / eframe — GUI framework
- cpal — audio output
- clap — CLI argument parsing

---

## 1. Player State Machine

The player is governed by a strict state machine that transitions based on user input and engine events.

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum PlayerState {
    /// Initial state, no media loaded
    Idle,

    /// Engine is resolving info_hash, contacting tracker/DHT, fetching metadata
    Loading {
        progress: f64,       // 0.0..1.0 metadata download progress
        peers_found: u32,    // Number of peers discovered so far
        message: String,     // Status message (e.g. "Connecting to tracker...")
    },

    /// Media is actively playing
    Playing {
        position: Duration,     // Current playback position
        duration: Duration,     // Total media duration (0 if unknown)
        speed: f64,             // Playback speed multiplier (1.0 = normal)
    },

    /// Playback is paused at a specific position
    Paused {
        position: Duration,
    },

    /// An unrecoverable error has occurred
    Error {
        message: String,
        code: ErrorCode,
        actionable: bool,   // Whether user can retry or must load new media
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ErrorCode {
    NetworkError,
    MetadataNotFound,
    NoPeers,
    DecodeError,
    CacheFull,
    UnsupportedFormat,
    EngineTimeout,
    InvalidUri,
}
```

### State Transition Diagram

```
Idle ────load_uri()────> Loading
Loading ──on_metadata_ok()──> Playing
Loading ──on_error()────────> Error
Playing ──pause()──────────> Paused
Playing ──stop()───────────> Idle
Playing ──on_error()──────> Error
Paused ──play()───────────> Playing
Paused ──stop()───────────> Idle
Paused ──on_error()──────> Error
Error ──retry()──────────> Loading
Error ──load_uri()───────> Loading
Error ──clear()──────────> Idle
```

### State Management Implementation

```rust
pub struct Player {
    state: PlayerState,
    engine: Arc<QvodEngine>,
    media: Option<MediaDecoder>,
    audio: Option<AudioOutput>,
    config: PlayerConfig,
    event_rx: mpsc::UnboundedReceiver<PlayerEvent>,
    command_tx: mpsc::UnboundedSender<PlayerCommand>,
}

impl Player {
    pub fn new(config: PlayerConfig, engine: Arc<QvodEngine>) -> Self;

    /// Process one event from the engine or UI (called each frame)
    pub fn tick(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            self.handle_event(event);
        }
    }

    fn handle_event(&mut self, event: PlayerEvent) {
        match event {
            PlayerEvent::MetadataLoaded { duration, keyframes } => {
                self.transition_to(PlayerState::Playing {
                    position: Duration::ZERO,
                    duration,
                    speed: 1.0,
                });
            }
            PlayerEvent::Progress { position, .. } => {
                if let PlayerState::Playing { ref mut pos, .. } = &mut self.state {
                    *pos = position;
                }
            }
            PlayerEvent::EngineError(err) => {
                self.transition_to(PlayerState::Error {
                    message: err.to_string(),
                    code: map_error_code(&err),
                    actionable: err.is_retryable(),
                });
            }
            PlayerEvent::Buffering { progress, peers } => {
                if matches!(self.state, PlayerState::Loading { .. }) {
                    // Update loading progress
                }
            }
            PlayerEvent::StreamEnded => {
                self.transition_to(PlayerState::Idle);
            }
        }
    }

    fn transition_to(&mut self, new_state: PlayerState) {
        let old_state = std::mem::replace(&mut self.state, new_state.clone());
        self.on_state_changed(&old_state, &new_state);
    }

    fn on_state_changed(&mut self, old: &PlayerState, new: &PlayerState) {
        match (old, new) {
            (PlayerState::Playing { .. }, PlayerState::Idle) => {
                self.media.take();
                self.audio.take();
            }
            _ => {}
        }
    }
}
```

---

## 2. GUI Mode (egui)

### 2.1 Main Window Layout

The GUI uses egui with eframe as the windowing backend. The main window is organized into the following sections:

```
┌─────────────────────────────────────────────────────┐
│  [−] [□] [×]  QVOD Player - movie.mp4              │
├─────────────────────────────────────────────────────┤
│  [🔍 URL Input Bar                    ] [▶ Play]    │
├─────────────────────────────────────────────────────┤
│                                                      │
│              ┌──────────────────────────┐            │
│              │                          │            │
│              │    Video Render Area     │            │
│              │   (egui::CentralPanel)   │            │
│              │                          │            │
│              └──────────────────────────┘            │
│                                                      │
│  ◄◄ [⏸ Pause] ►►  ──●───────────────────  00:42    │
│  [🔊 ████████░░]  1.0x  [⛶ Fullscreen]             │
├─────────────────────────────────────────────────────┤
│  Status: Playing  Peers: 12/47  Speed: 2.3 MB/s    │
│  Buffer: 34.2% (2m17s playable)  Health: ████▌     │
└─────────────────────────────────────────────────────┘
```

### 2.2 egui Widget Implementation

```rust
use eframe::egui;

pub struct PlayerUi {
    /// Whether the video area is focused for keyboard input
    video_focused: bool,
    /// Cached texture handle for the current video frame
    video_texture: Option<egui::TextureHandle>,
    /// Dragging state for seek bar
    seeking: bool,
    /// Pending seek position while dragging
    seek_position: Option<Duration>,
}

impl PlayerUi {
    pub fn render(&mut self, ctx: &egui::Context, player: &mut Player) {
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                self.render_url_bar(ui, player);
            });
        });

        egui::CentralPanel::default()
            .frame(egui::Frame::dark_canvas(&ctx.style()))
            .show(ctx, |ui| {
                self.render_video_area(ui, player);
            });

        egui::TopBottomPanel::bottom("controls").show(ctx, |ui| {
            self.render_controls(ui, player);
            self.render_status_bar(ui, player);
        });
    }
}
```

### 2.3 Video Renderer Widget

The video renderer converts decoded frames from ffmpeg-next into egui textures. This is the most performance-critical component.

```rust
use egui::ColorImage;
use ffmpeg_next::frame::Video as FfmpegVideoFrame;

pub struct VideoRenderer {
    /// RGBA pixel buffer for the current frame
    rgba_buffer: Vec<u8>,
    /// Cached egui texture
    texture: Option<egui::TextureHandle>,
    /// Current frame dimensions
    dimensions: (u32, u32),
    /// SWScale context for format conversion (YUV→RGBA)
    swscale: Option<ffmpeg_next::software::scaling::Context>,
    /// Target render size (may differ from native dimensions)
    target_size: Option<(u32, u32)>,
}

impl VideoRenderer {
    /// Accept a decoded video frame and convert to RGBA
    pub fn push_frame(&mut self, frame: &FfmpegVideoFrame) {
        let (w, h) = (frame.width(), frame.height());
        let target = self.target_size.unwrap_or((w, h));

        // Lazily initialize SWScale context
        if self.swscale.is_none() || self.dimensions != (w, h) {
            self.swscale = Some(
                ffmpeg_next::software::scaling::Context::get(
                    ffmpeg_next::format::Pixel::YUV420P,
                    w, h,
                    ffmpeg_next::format::Pixel::RGBA,
                    target.0, target.1,
                    ffmpeg_next::software::scaling::Flags::BILINEAR,
                )
                .expect("Failed to create SWScale context"),
            );
            self.rgba_buffer = vec![0u8; (target.0 * target.1 * 4) as usize];
            self.dimensions = (w, h);
        }

        // Convert YUV → RGBA
        let mut rgb_frame = ffmpeg_next::frame::Video::new(
            ffmpeg_next::format::Pixel::RGBA,
            target.0, target.1,
        );
        self.swscale.as_ref().unwrap().run(frame, &mut rgb_frame)
            .expect("SWScale conversion failed");

        // Copy pixel data
        let stride = rgb_frame.stride(0);
        let bytes = rgb_frame.data(0);
        for y in 0..target.1 {
            let src_start = (y as usize) * stride;
            let dst_start = (y as usize) * (target.0 as usize) * 4;
            self.rgba_buffer[dst_start..dst_start + (target.0 as usize) * 4]
                .copy_from_slice(&bytes[src_start..src_start + (target.0 as usize) * 4]);
        }
    }

    /// Draw the current frame into an egui UI
    pub fn render(&mut self, ui: &mut egui::Ui) -> egui::Response {
        let available = ui.available_size();
        let aspect_ratio = self.dimensions.0 as f32 / self.dimensions.1 as f32;
        let render_size = if available.x / available.y > aspect_ratio {
            egui::Vec2::new(available.y * aspect_ratio, available.y)
        } else {
            egui::Vec2::new(available.x, available.x / aspect_ratio)
        };

        if let Some(rgba) = self.rgba_buffer.as_slice() {
            let color_image = ColorImage::from_rgba_unmultiplied(
                [self.target_size.unwrap_or(self.dimensions).0 as usize,
                 self.target_size.unwrap_or(self.dimensions).1 as usize],
                rgba,
            );

            let texture: &egui::TextureHandle = self.texture.get_or_insert_with(|| {
                ui.ctx().load_texture(
                    "video_frame",
                    color_image.clone(),
                    egui::TextureOptions::LINEAR,
                )
            });

            // Update texture if frame changed
            texture.set(color_image, egui::TextureOptions::LINEAR);

            ui.add(
                egui::Image::new(texture)
                    .fit_to_exact_size(render_size)
                    .sense(egui::Sense::click()),
            )
        } else {
            // No frame yet - show placeholder
            ui.allocate_space(render_size)
        }
    }
}
```

### 2.4 Playback Controls

```rust
pub struct PlaybackControls {
    /// Volume level (0.0 = mute, 1.0 = max)
    volume: f32,
    /// Whether volume slider is being dragged
    volume_dragging: bool,
    /// Playback speed (0.5x, 1.0x, 1.5x, 2.0x)
    speed: f64,
    /// Whether the controls overlay is visible (auto-hides after inactivity)
    overlay_visible: bool,
    /// Timer for auto-hide
    idle_timer: f64,
    /// Available speeds in the speed selector
    available_speeds: [f64; 8],
}

impl Default for PlaybackControls {
    fn default() -> Self {
        Self {
            volume: 0.8,
            volume_dragging: false,
            speed: 1.0,
            overlay_visible: true,
            idle_timer: 0.0,
            available_speeds: [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0],
        }
    }
}

impl PlaybackControls {
    pub fn render(&mut self, ui: &mut egui::Ui, player: &mut Player, ctx: &egui::Context) {
        // Calculate control bar height
        let bar_height = 48.0;

        egui::Frame::new()
            .fill(egui::Color32::from_black_alpha(180))
            .show(ui, |ui| {
                ui.set_min_height(bar_height);
                ui.set_max_height(bar_height);

                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        // --- Previous Track ---
                        if ui.button("⏮").clicked() {
                            player.previous_track();
                        }

                        // --- Play/Pause Toggle ---
                        let play_label = match player.state() {
                            PlayerState::Playing { .. } => "⏸",
                            PlayerState::Paused { .. } | PlayerState::Idle => "▶",
                            PlayerState::Loading { .. } => "⏳",
                            PlayerState::Error { .. } => "↻",
                        };
                        if ui.button(play_label).clicked() {
                            match player.state() {
                                PlayerState::Playing { .. } => player.pause(),
                                PlayerState::Paused { .. } | PlayerState::Idle => player.play(),
                                PlayerState::Loading { .. } => {} // no-op
                                PlayerState::Error { .. } => player.retry(),
                            }
                        }

                        // --- Next Track ---
                        if ui.button("⏭").clicked() {
                            player.next_track();
                        }

                        ui.separator();

                        // --- Seek Bar ---
                        if let (Some(pos), Some(dur)) = (player.position(), player.duration()) {
                            let pos_secs = pos.as_secs_f64();
                            let dur_secs = dur.as_secs_f64().max(1.0);

                            // Time label (current)
                            ui.label(format!("{:02}:{:02}",
                                (pos_secs / 60.0) as u64,
                                (pos_secs % 60.0) as u64));

                            // Progress bar
                            let mut progress = (pos_secs / dur_secs) as f32;
                            let seek_bar = egui::Slider::new(&mut progress, 0.0..=1.0)
                                .show_value(false)
                                .custom_formatter(|v, _| {
                                    let s = v * dur_secs;
                                    format!("{:02}:{:02}", (s / 60.0) as u64, (s % 60.0) as u64)
                                });
                            let resp = ui.add(seek_bar);

                            if resp.drag_started() {
                                self.overlay_visible = true;
                                player.pause();
                            }
                            if resp.drag_released() {
                                let new_pos = Duration::from_secs_f64(progress as f64 * dur_secs);
                                player.seek(new_pos);
                                player.play();
                            }

                            // Time label (total)
                            ui.label(format!("{:02}:{:02}",
                                (dur_secs / 60.0) as u64,
                                (dur_secs % 60.0) as u64));
                        }
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // --- Fullscreen ---
                        if ui.button("⛶").clicked() {
                            // Toggle fullscreen via eframe
                        }

                        // --- Speed Selector ---
                        egui::ComboBox::from_id_salt("speed_selector")
                            .selected_text(format!("{:.2}x", self.speed))
                            .show_ui(ui, |ui| {
                                for &s in &self.available_speeds {
                                    if ui.selectable_label(
                                        (self.speed - s).abs() < f64::EPSILON,
                                        format!("{:.2}x", s),
                                    ).clicked() {
                                        self.speed = s;
                                        player.set_speed(s);
                                    }
                                }
                            });

                        // --- Volume ---
                        ui.add(egui::Slider::new(&mut self.volume, 0.0..=1.0)
                            .text("🔊")
                            .show_value(false)
                            .fixed_width(80.0));
                        player.set_volume(self.volume);
                    });
                });
            });
    }
}
```

### 2.5 URL Input Bar

```rust
pub struct UrlInputBar {
    /// Current URL text in the input field
    url_text: String,
    /// History of previously entered URLs
    history: Vec<String>,
    /// Whether the history dropdown is open
    show_history: bool,
}

impl UrlInputBar {
    pub fn render(&mut self, ui: &mut egui::Ui, player: &mut Player) {
        ui.horizontal(|ui| {
            // URL text input
            let resp = ui.add_sized(
                egui::vec2(ui.available_width() - 80.0, 0.0),
                egui::TextEdit::singleline(&mut self.url_text)
                    .hint_text("Enter qvod:// or http(s):// URL...")
                    .desired_width(f32::INFINITY),
            );

            // Submit on Enter
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.submit_url(player);
            }

            // Play button
            if ui.button("▶ Play").clicked() {
                self.submit_url(player);
            }

            // History dropdown
            if ui.button("▼").clicked() {
                self.show_history = !self.show_history;
            }

            // Dropdown list
            if self.show_history && !self.history.is_empty() {
                egui::Window::new("history")
                    .fixed_pos(ui.next_widget_position())
                    .show(ui.ctx(), |ui| {
                        for url in &self.history {
                            if ui.selectable_label(false, url).clicked() {
                                self.url_text = url.clone();
                                self.show_history = false;
                                self.submit_url(player);
                            }
                        }
                    });
            }
        });
    }

    fn submit_url(&mut self, player: &mut Player) {
        let url = self.url_text.trim().to_string();
        if url.is_empty() {
            return;
        }

        // Add to history (deduplicate, limit to 50 entries)
        self.history.retain(|h| h != &url);
        self.history.push(url.clone());
        if self.history.len() > 50 {
            self.history.remove(0);
        }

        // Determine URI type and load
        if url.starts_with("qvod://") {
            match QvodUri::parse(&url) {
                Ok(uri) => player.load_qvod(uri),
                Err(e) => player.show_error(format!("Invalid qvod:// URI: {}", e)),
            }
        } else if url.starts_with("http://") || url.starts_with("https://") {
            player.load_http(&url);
        } else {
            player.show_error("Unsupported URI scheme. Use qvod:// or http(s)://".into());
        }
    }
}
```

### 2.6 Network Status Panel

```rust
pub struct NetworkStatusPanel {
    /// Whether the panel is expanded
    expanded: bool,
}

impl NetworkStatusPanel {
    pub fn render(&mut self, ui: &mut egui::Ui, engine: &QvodEngine) {
        let stats = engine.stats();

        ui.horizontal(|ui| {
            // Health indicator
            let health = self.calculate_health(&stats);
            let health_color = if health > 0.7 {
                egui::Color32::GREEN
            } else if health > 0.3 {
                egui::Color32::YELLOW
            } else {
                egui::Color32::RED
            };
            ui.label(egui::RichText::new("●").color(health_color));

            // Compact status
            ui.label(format!(
                "Peers: {}/{} | Speed: {}/s | Buffer: {:.1}% | {}",
                stats.connected_peers,
                stats.total_peers,
                format_bytes_per_sec(stats.download_speed),
                stats.buffer_percent * 100.0,
                match stats.buffer_playable {
                    d if d > Duration::from_secs(60) => "Stable",
                    d if d > Duration::from_secs(10) => "Buffering",
                    _ => "Critical",
                }
            ));

            // Expand button
            if ui.button("📊").clicked() {
                self.expanded = !self.expanded;
            }
        });

        if self.expanded {
            egui::Frame::new()
                .fill(egui::Color32::from_black_alpha(200))
                .show(ui, |ui| {
                    self.render_detailed_panel(ui, &stats);
                });
        }
    }

    fn render_detailed_panel(&mut self, ui: &mut egui::Ui, stats: &EngineStats) {
        egui::Grid::new("network_grid")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                ui.label("Download Speed:");
                ui.label(format!("{}/s", format_bytes_per_sec(stats.download_speed)));
                ui.end_row();

                ui.label("Upload Speed:");
                ui.label(format!("{}/s", format_bytes_per_sec(stats.upload_speed)));
                ui.end_row();

                ui.label("Connected Peers:");
                ui.label(format!("{}", stats.connected_peers));
                ui.end_row();

                ui.label("Interested Peers:");
                ui.label(format!("{}", stats.interested_peers));
                ui.end_row();

                ui.label("Choked Peers:");
                ui.label(format!("{}", stats.choked_peers));
                ui.end_row();

                ui.label("Pieces Complete:");
                ui.label(format!("{}/{} ({:.1}%)",
                    stats.pieces_complete,
                    stats.pieces_total,
                    stats.pieces_complete as f64 / stats.pieces_total.max(1) as f64 * 100.0));
                ui.end_row();

                ui.label("Buffer Filled:");
                ui.label(format!("{:.1} MB / {:.1} MB",
                    stats.buffer_bytes as f64 / 1_048_576.0,
                    stats.buffer_capacity as f64 / 1_048_576.0));
                ui.end_row();

                ui.label("Playable Duration:");
                ui.label(format_duration(stats.buffer_playable));
                ui.end_row();

                ui.label("Avg RTT:");
                ui.label(format!("{} ms", stats.avg_rtt.as_millis()));
                ui.end_row();

                ui.label("Cache Hit Rate:");
                ui.label(format!("{:.1}%", stats.cache_hit_rate * 100.0));
                ui.end_row();

                ui.label("Memory Usage:");
                ui.label(format!("{:.1} MB", stats.memory_usage as f64 / 1_048_576.0));
                ui.end_row();
            });

        // Peer table
        ui.separator();
        ui.label("Active Peers:");
        egui::ScrollArea::vertical()
            .max_height(200.0)
            .show(ui, |ui| {
                egui::Grid::new("peer_grid")
                    .num_columns(6)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label("Peer ID");
                        ui.label("Speed (↓/↑)");
                        ui.label("RTT");
                        ui.label("Progress");
                        ui.label("Flags");
                        ui.label("Quality");
                        ui.end_row();

                        for peer in &stats.peers {
                            ui.label(format!("{}", &peer.peer_id[..8]));
                            ui.label(format!("{}/{}",
                                format_bytes_per_sec(peer.download_speed),
                                format_bytes_per_sec(peer.upload_speed)));
                            ui.label(format!("{}ms", peer.rtt.as_millis()));
                            ui.label(format!("{:.1}%", peer.progress * 100.0));
                            ui.label(format!("{}{}{}",
                                if peer.choked { "C" } else { "U" },
                                if peer.interested { "I" } else { " " },
                                if peer.is_seed { "S" } else { " " }));
                            ui.label(format!("{:.1}", peer.quality_score));
                            ui.end_row();
                        }
                    });
            });
    }

    fn calculate_health(&self, stats: &EngineStats) -> f32 {
        let mut score = 0.0;

        // Buffer health (40% weight)
        if stats.buffer_playable > Duration::from_secs(120) {
            score += 0.4;
        } else {
            score += 0.4 * (stats.buffer_playable.as_secs_f32() / 120.0);
        }

        // Peer count (30% weight)
        let peer_ratio = stats.connected_peers.min(20) as f32 / 20.0;
        score += 0.3 * peer_ratio;

        // Speed (20% weight)
        let speed_ratio = (stats.download_speed / 1_000_000.0).min(1.0); // normalize to 1MB/s
        score += 0.2 * speed_ratio;

        // Error rate (10% weight)
        score += 0.1 * (1.0 - stats.error_rate.min(1.0));

        score.min(1.0)
    }
}

fn format_bytes_per_sec(bytes_per_sec: f64) -> String {
    if bytes_per_sec > 1_048_576.0 {
        format!("{:.1} MB", bytes_per_sec / 1_048_576.0)
    } else if bytes_per_sec > 1024.0 {
        format!("{:.0} KB", bytes_per_sec / 1024.0)
    } else {
        format!("{:.0} B", bytes_per_sec)
    }
}

fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs > 3600 {
        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
    } else if secs > 60 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{}s", secs)
    }
}
```

### 2.7 Playlist/History Management

```rust
#[derive(Debug, Clone)]
pub struct PlaylistEntry {
    pub uri: String,
    pub title: String,
    pub duration: Option<Duration>,
    pub added_at: chrono::DateTime<chrono::Local>,
    pub last_played: Option<chrono::DateTime<chrono::Local>>,
    pub completion: f64,        // 0.0..1.0 (from cache)
    pub metadata: Option<FileMeta>,
}

pub struct PlaylistManager {
    entries: Vec<PlaylistEntry>,
    current_index: Option<usize>,
    play_mode: PlayMode,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayMode {
    Normal,       // Play once, stop at end
    RepeatOne,    // Repeat current track
    RepeatAll,    // Repeat entire playlist
    Shuffle,      // Random order
}

impl PlaylistManager {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            current_index: None,
            play_mode: PlayMode::Normal,
        }
    }

    pub fn add(&mut self, uri: String, title: String) {
        self.entries.push(PlaylistEntry {
            uri,
            title,
            duration: None,
            added_at: chrono::Local::now(),
            last_played: None,
            completion: 0.0,
            metadata: None,
        });
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.entries.len() {
            self.entries.remove(index);
            if let Some(current) = self.current_index {
                if index < current {
                    self.current_index = Some(current - 1);
                } else if index == current {
                    self.current_index = None;
                }
            }
        }
    }

    pub fn next(&mut self) -> Option<&PlaylistEntry> {
        match self.play_mode {
            PlayMode::Normal | PlayMode::RepeatAll => {
                let next = self.current_index.map(|i| (i + 1) % self.entries.len());
                self.current_index = next;
                self.current_index.and_then(|i| self.entries.get(i))
            }
            PlayMode::RepeatOne => {
                self.current_index.and_then(|i| self.entries.get(i))
            }
            PlayMode::Shuffle => {
                use rand::seq::SliceRandom;
                let idx = (0..self.entries.len())
                    .filter(|&i| Some(i) != self.current_index)
                    .collect::<Vec<_>>()
                    .choose(&mut rand::thread_rng())
                    .copied();
                self.current_index = idx;
                idx.and_then(|i| self.entries.get(i))
            }
        }
    }

    pub fn previous(&mut self) -> Option<&PlaylistEntry> {
        match self.play_mode {
            PlayMode::Normal | PlayMode::RepeatAll => {
                let count = self.entries.len();
                let prev = self.current_index.map(|i| (i + count - 1) % count);
                self.current_index = prev;
                self.current_index.and_then(|i| self.entries.get(i))
            }
            _ => self.current_index.and_then(|i| self.entries.get(i)),
        }
    }

    pub fn render_playlist(&self, ui: &mut egui::Ui, player: &mut Player) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, entry) in self.entries.iter().enumerate() {
                let is_current = self.current_index == Some(i);
                let bg_color = if is_current {
                    egui::Color32::from_rgba_premultiplied(40, 80, 160, 80)
                } else {
                    egui::Color32::TRANSPARENT
                };

                egui::Frame::new()
                    .fill(bg_color)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui.button("▶").clicked() && !is_current {
                                player.load_uri(&entry.uri);
                                // self.current_index = Some(i);  // handled by player callback
                            }
                            ui.label(&entry.title);
                            if let Some(dur) = entry.duration {
                                ui.label(format!("[{}]", format_duration(dur)));
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if entry.completion > 0.0 {
                                    ui.label(format!("{:.0}%", entry.completion * 100.0));
                                }
                                if ui.button("✕").clicked() {
                                    // Remove handled via callback
                                }
                            });
                        });
                    });
            }
        });
    }
}
```

### 2.8 Settings Dialog

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerConfig {
    // Window
    pub window_width: u32,
    pub window_height: u32,
    pub fullscreen_on_start: bool,
    pub remember_window_position: bool,

    // Playback
    pub default_volume: f32,
    pub remember_playback_position: bool,
    pub auto_resume: bool,

    // Network
    pub listen_port: u16,
    pub udp_port: u16,
    pub max_connections: u32,
    pub max_upload_slots: u32,
    pub download_rate_limit: u64,       // bytes/sec, 0 = unlimited
    pub upload_rate_limit: u64,         // bytes/sec, 0 = unlimited
    pub enable_dht: bool,
    pub enable_tracker: bool,
    pub enable_http_fallback: bool,

    // Cache
    pub cache_dir: PathBuf,
    pub max_cache_size_gb: u32,
    pub auto_cleanup_cache: bool,
    pub cache_cleanup_threshold_pct: f32,

    // Proxy
    pub proxy_enabled: bool,
    pub proxy_host: String,
    pub proxy_port: u16,
    pub proxy_username: String,
    pub proxy_password: String,

    // Tracker
    pub tracker_urls: Vec<String>,
    pub dht_seed_nodes: Vec<String>,

    // UI
    pub show_network_panel: bool,
    pub auto_hide_controls: bool,
    pub controls_idle_timeout_secs: f32,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            window_width: 1280,
            window_height: 720,
            fullscreen_on_start: false,
            remember_window_position: true,
            default_volume: 0.8,
            remember_playback_position: true,
            auto_resume: true,
            listen_port: 8621,
            udp_port: 8622,
            max_connections: 50,
            max_upload_slots: 5,
            download_rate_limit: 0,
            upload_rate_limit: 0,
            enable_dht: true,
            enable_tracker: true,
            enable_http_fallback: true,
            cache_dir: default_cache_dir(),
            max_cache_size_gb: 4,
            auto_cleanup_cache: true,
            cache_cleanup_threshold_pct: 90.0,
            proxy_enabled: false,
            proxy_host: String::new(),
            proxy_port: 1080,
            proxy_username: String::new(),
            proxy_password: String::new(),
            tracker_urls: vec![
                "http://tracker.qvod.example.com/announce".into(),
            ],
            dht_seed_nodes: vec![
                "dht.qvod.example.com:8622".into(),
            ],
            show_network_panel: true,
            auto_hide_controls: true,
            controls_idle_timeout_secs: 3.0,
        }
    }
}

fn default_cache_dir() -> PathBuf {
    let base = if cfg!(target_os = "linux") {
        dirs::data_dir().unwrap_or_else(|| PathBuf::from("~/.local/share"))
    } else if cfg!(target_os = "macos") {
        dirs::data_dir().unwrap_or_else(|| PathBuf::from("~/Library/Application Support"))
    } else if cfg!(target_os = "windows") {
        dirs::data_dir().unwrap_or_else(|| PathBuf::from(r"C:\Users\Default\AppData\Local"))
    } else {
        PathBuf::from(".qvod")
    };
    base.join("qvod")
}

pub struct SettingsDialog {
    open: bool,
    config: PlayerConfig,
    selected_section: SettingsSection,
    modified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SettingsSection {
    General,
    Playback,
    Network,
    Cache,
    Proxy,
    Tracker,
    About,
}

impl SettingsDialog {
    pub fn new(config: PlayerConfig) -> Self {
        Self {
            open: false,
            config,
            selected_section: SettingsSection::General,
            modified: false,
        }
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    pub fn render(&mut self, ctx: &egui::Context) -> Option<PlayerConfig> {
        let mut result = None;

        if self.open {
            egui::Window::new("Settings")
                .id(egui::Id::new("settings_dialog"))
                .default_size([600.0, 450.0])
                .resizable(true)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        // Left sidebar
                        egui::SidePanel::left("settings_sidebar")
                            .resizable(false)
                            .min_width(120.0)
                            .show_inside(ui, |ui| {
                                egui::ScrollArea::vertical().show(ui, |ui| {
                                    ui.vertical(|ui| {
                                        if ui.selectable_label(
                                            self.selected_section == SettingsSection::General,
                                            "General",
                                        ).clicked() {
                                            self.selected_section = SettingsSection::General;
                                        }
                                        if ui.selectable_label(
                                            self.selected_section == SettingsSection::Playback,
                                            "Playback",
                                        ).clicked() {
                                            self.selected_section = SettingsSection::Playback;
                                        }
                                        if ui.selectable_label(
                                            self.selected_section == SettingsSection::Network,
                                            "Network",
                                        ).clicked() {
                                            self.selected_section = SettingsSection::Network;
                                        }
                                        if ui.selectable_label(
                                            self.selected_section == SettingsSection::Cache,
                                            "Cache",
                                        ).clicked() {
                                            self.selected_section = SettingsSection::Cache;
                                        }
                                        if ui.selectable_label(
                                            self.selected_section == SettingsSection::Proxy,
                                            "Proxy",
                                        ).clicked() {
                                            self.selected_section = SettingsSection::Proxy;
                                        }
                                        if ui.selectable_label(
                                            self.selected_section == SettingsSection::Tracker,
                                            "Tracker / DHT",
                                        ).clicked() {
                                            self.selected_section = SettingsSection::Tracker;
                                        }
                                        if ui.selectable_label(
                                            self.selected_section == SettingsSection::About,
                                            "About",
                                        ).clicked() {
                                            self.selected_section = SettingsSection::About;
                                        }
                                    });
                                });
                            });

                        // Right content
                        egui::CentralPanel::default().show_inside(ui, |ui| {
                            match self.selected_section {
                                SettingsSection::General => self.render_general(ui),
                                SettingsSection::Playback => self.render_playback(ui),
                                SettingsSection::Network => self.render_network(ui),
                                SettingsSection::Cache => self.render_cache(ui),
                                SettingsSection::Proxy => self.render_proxy(ui),
                                SettingsSection::Tracker => self.render_tracker(ui),
                                SettingsSection::About => self.render_about(ui),
                            }
                        });
                    });

                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Cancel").clicked() {
                                self.open = false;
                            }
                            if ui.button("Save").clicked() {
                                result = Some(self.config.clone());
                                self.open = false;
                            }
                        });
                    });
                });
        }

        result
    }

    fn render_general(&mut self, ui: &mut egui::Ui) {
        ui.heading("General Settings");
        ui.separator();

        ui.add(egui::Checkbox::new(&mut self.config.fullscreen_on_start, "Fullscreen on start"));
        ui.add(egui::Checkbox::new(&mut self.config.remember_window_position, "Remember window position"));
        ui.add(egui::Checkbox::new(&mut self.config.show_network_panel, "Show network status panel"));
        ui.add(egui::Checkbox::new(&mut self.config.auto_hide_controls, "Auto-hide playback controls"));

        if self.config.auto_hide_controls {
            ui.add(egui::Slider::new(
                &mut self.config.controls_idle_timeout_secs, 1.0..=10.0
            ).text("Controls idle timeout (sec)"));
        }
    }

    fn render_playback(&mut self, ui: &mut egui::Ui) {
        ui.heading("Playback Settings");
        ui.separator();

        ui.add(egui::Slider::new(&mut self.config.default_volume, 0.0..=1.0).text("Default Volume"));
        ui.add(egui::Checkbox::new(&mut self.config.remember_playback_position, "Remember playback position"));
        ui.add(egui::Checkbox::new(&mut self.config.auto_resume, "Auto-resume from last position"));
    }

    fn render_network(&mut self, ui: &mut egui::Ui) {
        ui.heading("Network Settings");
        ui.separator();

        ui.horizontal(|ui| {
            ui.label("Listen Port:");
            ui.add(egui::Slider::new(&mut self.config.listen_port, 1024..=65535));
        });
        ui.horizontal(|ui| {
            ui.label("UDP Port:");
            ui.add(egui::Slider::new(&mut self.config.udp_port, 1024..=65535));
        });
        ui.add(egui::Slider::new(&mut self.config.max_connections, 10..=500).text("Max Connections"));

        ui.separator();
        ui.label("Rate Limits (0 = unlimited):");
        ui.horizontal(|ui| {
            ui.label("Download:");
            ui.add(egui::Slider::new(&mut self.config.download_rate_limit, 0..=100_000_000)
                .suffix(" B/s"));
        });
        ui.horizontal(|ui| {
            ui.label("Upload:");
            ui.add(egui::Slider::new(&mut self.config.upload_rate_limit, 0..=10_000_000)
                .suffix(" B/s"));
        });

        ui.separator();
        ui.add(egui::Checkbox::new(&mut self.config.enable_dht, "Enable DHT"));
        ui.add(egui::Checkbox::new(&mut self.config.enable_tracker, "Enable Tracker"));
        ui.add(egui::Checkbox::new(&mut self.config.enable_http_fallback, "Enable HTTP Fallback"));
        ui.add(egui::Slider::new(&mut self.config.max_upload_slots, 0..=50).text("Max Upload Slots"));
    }

    fn render_cache(&mut self, ui: &mut egui::Ui) {
        ui.heading("Cache Settings");
        ui.separator();

        ui.horizontal(|ui| {
            ui.label("Cache Directory:");
            if ui.button("📁").clicked() {
                // Open native file dialog
            }
        });
        ui.label(self.config.cache_dir.to_string_lossy().to_string());

        ui.add(egui::Slider::new(&mut self.config.max_cache_size_gb, 1..=100)
            .text("Max Cache Size (GB)"));
        ui.add(egui::Checkbox::new(&mut self.config.auto_cleanup_cache, "Auto-cleanup cache"));
        if self.config.auto_cleanup_cache {
            ui.add(egui::Slider::new(&mut self.config.cache_cleanup_threshold_pct, 50.0..=100.0)
                .text("Cleanup at usage (%)"));
        }
    }

    fn render_proxy(&mut self, ui: &mut egui::Ui) {
        ui.heading("Proxy Settings");
        ui.separator();

        ui.add(egui::Checkbox::new(&mut self.config.proxy_enabled, "Enable Proxy"));
        if self.config.proxy_enabled {
            ui.horizontal(|ui| {
                ui.label("Host:");
                ui.text_edit_singleline(&mut self.config.proxy_host);
            });
            ui.horizontal(|ui| {
                ui.label("Port:");
                ui.add(egui::Slider::new(&mut self.config.proxy_port, 1..=65535));
            });
            ui.horizontal(|ui| {
                ui.label("Username:");
                ui.text_edit_singleline(&mut self.config.proxy_username);
            });
            ui.horizontal(|ui| {
                ui.label("Password:");
                ui.add(egui::TextEdit::singleline(&mut self.config.proxy_password).password(true));
            });
        }
    }

    fn render_tracker(&mut self, ui: &mut egui::Ui) {
        ui.heading("Tracker & DHT Settings");
        ui.separator();

        ui.label("Tracker URLs:");
        let mut to_remove = None;
        for (i, url) in self.config.tracker_urls.clone().iter().enumerate() {
            ui.horizontal(|ui| {
                let mut url_mut = self.config.tracker_urls[i].clone();
                ui.text_edit_singleline(&mut url_mut);
                self.config.tracker_urls[i] = url_mut;
                if ui.button("✕").clicked() {
                    to_remove = Some(i);
                }
            });
        }
        if let Some(i) = to_remove {
            self.config.tracker_urls.remove(i);
        }
        if ui.button("+ Add Tracker URL").clicked() {
            self.config.tracker_urls.push("http://".into());
        }

        ui.separator();
        ui.label("DHT Seed Nodes:");
        let mut to_remove_dht = None;
        for (i, node) in self.config.dht_seed_nodes.clone().iter().enumerate() {
            ui.horizontal(|ui| {
                let mut node_mut = self.config.dht_seed_nodes[i].clone();
                ui.text_edit_singleline(&mut node_mut);
                self.config.dht_seed_nodes[i] = node_mut;
                if ui.button("✕").clicked() {
                    to_remove_dht = Some(i);
                }
            });
        }
        if let Some(i) = to_remove_dht {
            self.config.dht_seed_nodes.remove(i);
        }
        if ui.button("+ Add DHT Seed Node").clicked() {
            self.config.dht_seed_nodes.push("".into());
        }
    }

    fn render_about(&mut self, ui: &mut egui::Ui) {
        ui.heading("About QVOD Player");
        ui.separator();
        ui.label("QVOD (快播) P2SP Streaming System");
        ui.label("Version: 0.1.0");
        ui.label("License: MIT");
        ui.label("");
        ui.label("Built with Rust, egui, FFmpeg");
        ui.label("Cross-platform: Linux, macOS, Windows");
    }
}
```

---

## 3. CLI Mode

The CLI provides a headless interface for scripting, remote control, and minimal interaction.

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "qvs", version, about = "QVOD P2SP Streaming Player")]
pub struct CliArgs {
    #[command(subcommand)]
    pub command: CliCommand,
}

#[derive(Subcommand)]
pub enum CliCommand {
    /// Play a media URL
    Play {
        /// Media URL (qvod:// or http(s)://)
        url: String,

        /// Start position (seconds from start)
        #[arg(short, long, default_value = "0")]
        seek: f64,

        /// Volume (0.0 to 1.0)
        #[arg(short, long, default_value = "0.8")]
        volume: f32,

        /// Playback speed
        #[arg(short, long, default_value = "1.0")]
        speed: f64,

        /// Headless mode (no GUI, download only)
        #[arg(short, long)]
        headless: bool,
    },

    /// Show current playback status
    Status {
        /// Continuously update status (like top)
        #[arg(short, long)]
        watch: bool,
    },

    /// List active and cached torrents
    List,

    /// Manage local cache
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },

    /// Control a running player instance
    Control {
        #[command(subcommand)]
        action: ControlAction,
    },
}

#[derive(Subcommand)]
pub enum CacheAction {
    /// Show cache usage info
    Info,
    /// Clean cache to free space
    Clean {
        /// Target size in GB after cleanup
        #[arg(short, long)]
        target_gb: Option<u32>,
    },
    /// Remove a specific cache entry
    Remove {
        info_hash: String,
    },
    /// List all cached entries
    List,
}

#[derive(Subcommand)]
pub enum ControlAction {
    Pause,
    Resume,
    Stop,
    Seek { position_secs: f64 },
    Volume { level: f32 },
    Mute,
}
```

### CLI Status Output

```
$ qvs status
┌────────────────────────────────────────────────────┐
│ QVOD Player Status                                 │
├────────────────────────────────────────────────────┤
│ State:      Playing                                │
│ URI:        qvod://A1B2...|movie.mp4|734003200|mp4 │
│ Position:   00:42 / 01:23:45 (0.5%)                │
│ Speed:      2.3 MB/s (down) / 120 KB/s (up)        │
│ Peers:      12 connected / 47 total                │
│ Buffer:     34.2% (2m17s playable)                 │
│ Cache:      156.2 MB / 4.0 GB (3.8%)               │
│ Health:     ████████░░  Good                        │
└────────────────────────────────────────────────────┘
```

### IPC Control

CLI communicates with a running GUI instance via Unix domain socket (Linux/macOS) or named pipe (Windows):

```rust
pub struct IpcClient {
    socket_path: PathBuf,
}

impl IpcClient {
    pub fn connect() -> Result<Self> {
        let runtime_dir = if cfg!(target_os = "linux") {
            std::env::var("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/tmp"))
        } else if cfg!(target_os = "macos") {
            PathBuf::from("/tmp")
        } else {
            PathBuf::from(r"\\.\pipe\")
        };

        let socket_path = runtime_dir.join("qvod-player.sock");
        Ok(Self { socket_path })
    }

    pub fn send_command(&self, cmd: IpcCommand) -> Result<IpcResponse> {
        use std::os::unix::net::UnixStream;
        let stream = UnixStream::connect(&self.socket_path)?;
        let writer = std::io::BufWriter::new(&stream);
        serde_json::to_writer(writer, &cmd)?;
        let reader = std::io::BufReader::new(&stream);
        let resp: IpcResponse = serde_json::from_reader(reader)?;
        Ok(resp)
    }
}
```

---

## 4. Media Decode Integration (qvs-media)

```rust
use ffmpeg_next as ffmpeg;

pub struct MediaDecoder {
    /// Format context (demuxer)
    fmt_ctx: ffmpeg::format::context::Input,
    /// Video stream index
    video_stream_index: usize,
    /// Audio stream index
    audio_stream_index: Option<usize>,
    /// Video decoder
    video_decoder: ffmpeg::decoder::Video,
    /// Audio decoder
    audio_decoder: Option<ffmpeg::decoder::Audio>,
    /// Current playback position
    position: Duration,
    /// Total duration
    duration: Duration,
}

impl MediaDecoder {
    pub fn new(path: &str) -> Result<Self, MediaError> {
        ffmpeg::init()?;

        let fmt_ctx = ffmpeg::format::input(&path)?;
        let duration = Duration::from_micros(fmt_ctx.duration() as u64);

        let video_stream = fmt_ctx.streams()
            .best(ffmpeg::media::Type::Video)
            .ok_or(MediaError::NoVideoStream)?;
        let video_stream_index = video_stream.index();

        let video_decoder = ffmpeg::codec::Context::from_parameters(video_stream.parameters())?
            .decoder()
            .video()?;

        let audio_stream_index = fmt_ctx.streams()
            .best(ffmpeg::media::Type::Audio)
            .map(|s| s.index());
        let audio_decoder = audio_stream_index.and_then(|idx| {
            let stream = fmt_ctx.stream(idx).ok()?;
            ffmpeg::codec::Context::from_parameters(stream.parameters()).ok()
                .and_then(|c| c.decoder().audio().ok())
        });

        Ok(Self {
            fmt_ctx,
            video_stream_index,
            audio_stream_index,
            video_decoder,
            audio_decoder,
            position: Duration::ZERO,
            duration,
        })
    }

    pub fn decode_frame(&mut self) -> Result<Option<DecodedFrame>, MediaError> {
        for (stream, packet) in self.fmt_ctx.packets() {
            if stream.index() == self.video_stream_index {
                self.video_decoder.send_packet(&packet)?;
                let mut decoded = ffmpeg::frame::Video::empty();
                if self.video_decoder.receive_frame(&mut decoded).is_ok() {
                    self.position = Duration::from_micros(
                        packet.dts().map_or(0, |dts| dts as u64)
                    );
                    return Ok(Some(DecodedFrame::Video(decoded)));
                }
            } else if let Some(audio_idx) = self.audio_stream_index {
                if stream.index() == audio_idx {
                    if let Some(ref mut decoder) = self.audio_decoder {
                        decoder.send_packet(&packet)?;
                        let mut decoded = ffmpeg::frame::Audio::empty();
                        if decoder.receive_frame(&mut decoded).is_ok() {
                            return Ok(Some(DecodedFrame::Audio(decoded)));
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    pub fn seek(&mut self, timestamp: Duration) -> Result<(), MediaError> {
        let ts = timestamp.as_micros() as i64;
        self.fmt_ctx.seek(ts, ..ts)?;
        self.video_decoder.flush();
        if let Some(ref mut decoder) = self.audio_decoder {
            decoder.flush();
        }
        Ok(())
    }

    pub fn position(&self) -> Duration { self.position }
    pub fn duration(&self) -> Duration { self.duration }
}

pub enum DecodedFrame {
    Video(ffmpeg::frame::Video),
    Audio(ffmpeg::frame::Audio),
}

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("FFmpeg initialization failed: {0}")]
    InitFailed(String),
    #[error("No video stream found")]
    NoVideoStream,
    #[error("Decode error: {0}")]
    Decode(String),
}
```

---

## 5. Audio Output (cpal)

```rust
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub struct AudioOutput {
    stream: cpal::Stream,
    sample_rate: u32,
    channels: u16,
    volume: std::sync::Arc<std::sync::atomic::AtomicF32>,
}

impl AudioOutput {
    pub fn new(
        sample_rate: u32,
        channels: u16,
        audio_rx: mpsc::Receiver<Vec<f32>>,
    ) -> Result<Self, AudioError> {
        let host = cpal::default_host();
        let device = host.default_output_device()
            .ok_or(AudioError::NoOutputDevice)?;
        let config = device.default_output_config()?;

        let volume = std::sync::Arc::new(std::sync::atomic::AtomicF32::new(0.8));
        let volume_clone = volume.clone();

        let stream = device.build_output_stream(
            &config.into(),
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let vol = volume_clone.load(std::sync::atomic::Ordering::Relaxed);
                // Fill audio buffer from the channel
                if let Ok(samples) = audio_rx.try_recv() {
                    for (out, &inp) in data.iter_mut().zip(samples.iter()) {
                        *out = inp * vol;
                    }
                } else {
                    // Fill with silence
                    for sample in data.iter_mut() {
                        *sample = 0.0;
                    }
                }
            },
            |err| eprintln!("Audio stream error: {}", err),
            None,
        )?;

        stream.play()?;

        Ok(Self {
            stream,
            sample_rate,
            channels,
            volume,
        })
    }

    pub fn set_volume(&self, vol: f32) {
        self.volume.store(vol.clamp(0.0, 1.0), std::sync::atomic::Ordering::Relaxed);
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("No audio output device available")]
    NoOutputDevice,
    #[error("Audio stream error: {0}")]
    Stream(String),
}
```

---

## 6. Window Management & Event Loop

```rust
use eframe::egui;

pub struct QvodApp {
    player: Player,
    ui: PlayerUi,
    controls: PlaybackControls,
    url_bar: UrlInputBar,
    network_panel: NetworkStatusPanel,
    playlist: PlaylistManager,
    settings: SettingsDialog,
    config: PlayerConfig,
    video_renderer: VideoRenderer,
    media_decoder: Option<MediaDecoder>,
    audio_output: Option<AudioOutput>,
    load_request: Option<(String, LoadTarget)>,
}

enum LoadTarget {
    Qvod(QvodUri),
    Http(String),
}

impl eframe::App for QvodApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle deferred load requests
        if let Some((_url, target)) = self.load_request.take() {
            match target {
                LoadTarget::Qvod(uri) => self.player.load_qvod(uri),
                LoadTarget::Http(url) => self.player.load_http(&url),
            }
        }

        // Main layout
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            self.url_bar.render(ui, &mut self.player);
        });

        egui::CentralPanel::default()
            .frame(egui::Frame::dark_canvas(&ctx.style()))
            .show(ctx, |ui| {
                self.video_renderer.render(ui);
            });

        egui::TopBottomPanel::bottom("controls").show(ctx, |ui| {
            self.controls.render(ui, &mut self.player, ctx);
            if self.config.show_network_panel {
                self.network_panel.render(ui, &self.player.engine());
            }
        });

        // Settings dialog
        if let Some(new_config) = self.settings.render(ctx) {
            self.config = new_config;
            self.player.apply_config(&self.config);
        }

        // Keyboard shortcuts
        ctx.input(|i| {
            if i.key_pressed(egui::Key::Space) {
                match self.player.state() {
                    PlayerState::Playing { .. } => self.player.pause(),
                    PlayerState::Paused { .. } => self.player.play(),
                    _ => {}
                }
            }
            if i.key_pressed(egui::Key::F) {
                // Toggle fullscreen
            }
            if i.key_pressed(egui::Key::Escape) {
                // Exit fullscreen
            }
            if i.key_pressed(egui::Key::M) {
                self.controls.volume = if self.controls.volume > 0.0 { 0.0 } else { 0.8 };
                self.player.set_volume(self.controls.volume);
            }
            if i.key_pressed(egui::Key::S) {
                self.settings.toggle();
            }
        });

        // Request repaint continuously while playing
        if matches!(self.player.state(), PlayerState::Playing { .. }) {
            ctx.request_repaint();
        }
    }
}
```

---

## 7. Player Commands (Public API)

```rust
impl Player {
    /// Load a qvod:// URI for playback
    pub fn load_qvod(&mut self, uri: QvodUri) {
        self.transition_to(PlayerState::Loading {
            progress: 0.0,
            peers_found: 0,
            message: "Resolving info_hash...".into(),
        });
        let cmd = PlayerCommand::LoadQvod(uri);
        self.command_tx.send(cmd).ok();
    }

    /// Load an HTTP(S) URL for playback
    pub fn load_http(&mut self, url: &str) {
        self.transition_to(PlayerState::Loading {
            progress: 0.0,
            peers_found: 0,
            message: "Connecting to HTTP source...".into(),
        });
        let cmd = PlayerCommand::LoadHttp(url.to_string());
        self.command_tx.send(cmd).ok();
    }

    /// Start or resume playback
    pub fn play(&mut self) {
        let cmd = PlayerCommand::Play;
        self.command_tx.send(cmd).ok();
    }

    /// Pause playback
    pub fn pause(&mut self) {
        let cmd = PlayerCommand::Pause;
        self.command_tx.send(cmd).ok();
    }

    /// Stop playback and return to idle
    pub fn stop(&mut self) {
        let cmd = PlayerCommand::Stop;
        self.command_tx.send(cmd).ok();
    }

    /// Seek to a specific position
    pub fn seek(&mut self, position: Duration) {
        let cmd = PlayerCommand::Seek(position);
        self.command_tx.send(cmd).ok();
    }

    /// Set volume (0.0..1.0)
    pub fn set_volume(&mut self, volume: f32) {
        let cmd = PlayerCommand::SetVolume(volume);
        self.command_tx.send(cmd).ok();
    }

    /// Set playback speed multiplier
    pub fn set_speed(&mut self, speed: f64) {
        let cmd = PlayerCommand::SetSpeed(speed);
        self.command_tx.send(cmd).ok();
    }

    /// Jump to next playlist entry
    pub fn next_track(&mut self) {
        if let Some(entry) = self.playlist.next() {
            self.load_uri(&entry.uri);
        }
    }

    /// Jump to previous playlist entry
    pub fn previous_track(&mut self) {
        if let Some(entry) = self.playlist.previous() {
            self.load_uri(&entry.uri);
        }
    }

    /// Retry after an error
    pub fn retry(&mut self) {
        if let PlayerState::Error { .. } = &self.state {
            let cmd = PlayerCommand::Retry;
            self.command_tx.send(cmd).ok();
        }
    }

    /// Show an error message (for URI parse failures, etc.)
    pub fn show_error(&mut self, message: String) {
        self.transition_to(PlayerState::Error {
            message,
            code: ErrorCode::InvalidUri,
            actionable: true,
        });
    }

    // -- Getters --

    pub fn state(&self) -> &PlayerState { &self.state }
    pub fn position(&self) -> Option<Duration> {
        match &self.state {
            PlayerState::Playing { position, .. } => Some(*position),
            PlayerState::Paused { position } => Some(*position),
            _ => None,
        }
    }
    pub fn duration(&self) -> Option<Duration> {
        match &self.state {
            PlayerState::Playing { duration, .. } => Some(*duration),
            _ => None,
        }
    }
    pub fn engine(&self) -> &QvodEngine { &self.engine }
    pub fn config(&self) -> &PlayerConfig { &self.config }
    pub fn apply_config(&mut self, config: &PlayerConfig) {
        self.config = config.clone();
        self.command_tx.send(PlayerCommand::UpdateConfig(config.clone())).ok();
    }
}
```

---

## 8. Playback Progress Reporting

The streaming engine reports progress via a channel that the player reads each frame:

```rust
/// Events emitted by the streaming engine to the player
pub enum EngineEvent {
    /// Playback position advanced
    PositionUpdate {
        position_bytes: u64,
        position_time: Duration,
        duration: Duration,
    },
    /// Download progress
    DownloadProgress {
        bytes_downloaded: u64,
        bytes_total: u64,
        speed: f64,
    },
    /// Peers changed
    PeersChanged {
        connected: u32,
        total: u32,
    },
    /// Buffer status changed
    BufferStatus {
        playable_duration: Duration,
        buffer_fill_pct: f64,
    },
    /// An error occurred (may be retryable)
    Error { error: QvodError, retryable: bool },
    /// Stream has ended
    StreamEnded,
    /// Metadata has been loaded
    MetadataLoaded {
        duration: Duration,
        keyframe_count: usize,
    },
}

impl Player {
    fn handle_engine_event(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::PositionUpdate { position_time, duration, .. } => {
                if let PlayerState::Playing { ref mut position, ref mut dur, .. } = &mut self.state {
                    *position = position_time;
                    *duration = duration;
                }
            }
            EngineEvent::BufferStatus { playable_duration, buffer_fill_pct } => {
                // Update UI buffer indicators
            }
            EngineEvent::PeersChanged { connected, total } => {
                if let PlayerState::Loading { ref mut peers_found, .. } = &mut self.state {
                    *peers_found = connected;
                }
            }
            EngineEvent::Error { error, retryable } => {
                self.transition_to(PlayerState::Error {
                    message: error.to_string(),
                    code: map_error_code(&error),
                    actionable: retryable,
                });
            }
            EngineEvent::StreamEnded => {
                self.transition_to(PlayerState::Idle);
            }
            _ => {}
        }
    }
}
```

---

## 9. Error Display

Errors are presented to the user with contextual information and suggested actions:

```rust
pub fn render_error_dialog(error: &PlayerState, ui: &mut egui::Ui) {
    if let PlayerState::Error { message, code, actionable } = error {
        egui::Frame::new()
            .fill(egui::Color32::from_rgb(60, 20, 20))
            .rounding(8.0)
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("⚠ Error").size(18.0).color(egui::Color32::RED));
                    ui.add_space(8.0);
                    ui.label(message);
                    ui.add_space(4.0);
                    ui.label(format!("Code: {:?}", code));
                    ui.add_space(12.0);

                    if *actionable {
                        ui.horizontal(|ui| {
                            if ui.button("Retry").clicked() {
                                // retry handled by caller
                            }
                            if ui.button("Load New URL").clicked() {
                                // clear handled by caller
                            }
                        });
                    } else {
                        if ui.button("Clear").clicked() {
                            // clear handled by caller
                        }
                    }

                    // Contextual help
                    ui.add_space(8.0);
                    let help_text = match code {
                        ErrorCode::NetworkError => "Check your internet connection and firewall settings.",
                        ErrorCode::MetadataNotFound => "The source may not contain valid metadata. Try a different link.",
                        ErrorCode::NoPeers => "No other peers found for this content. Try again later or use HTTP fallback.",
                        ErrorCode::DecodeError => "The video codec may not be supported. Install additional codecs.",
                        ErrorCode::CacheFull => "Free up disk space or increase cache size in Settings.",
                        ErrorCode::UnsupportedFormat => "This video format is not supported.",
                        ErrorCode::EngineTimeout => "The engine took too long to respond. This may be a network issue.",
                        ErrorCode::InvalidUri => "The URL format is invalid. Use qvod:// or http(s):// links.",
                    };
                    ui.label(egui::RichText::new(help_text).italics().size(12.0));
                });
            });
    }
}
```

---

## Summary

| Component | File | Description |
|-----------|------|-------------|
| `Player` | `player.rs` | Main player struct, state machine, public API |
| `PlayerUi` | `ui.rs` | egui widget composition |
| `VideoRenderer` | `renderer.rs` | FFmpeg→egui texture conversion |
| `PlaybackControls` | `controls.rs` | Play/pause, seek, volume, speed |
| `UrlInputBar` | `toolbar.rs` | URL input with history |
| `NetworkStatusPanel` | `status.rs` | Real-time network stats display |
| `PlaylistManager` | `playlist.rs` | Playlist/history CRUD |
| `SettingsDialog` | `settings.rs` | Full settings UI |
| `MediaDecoder` | `qvs-media/src/decoder.rs` | FFmpeg demuxing/decoding |
| `AudioOutput` | `audio.rs` | cpal audio playback |
| `QvodApp` | `main.rs` | eframe app entry point |
| `IpcClient` | `ipc.rs` | CLI↔GUI IPC |
