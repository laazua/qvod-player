# QVOD GUI 1:1 Restoration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Transform the functional egui player GUI into a pixel-accurate recreation of QvodPlayer 6.x with left-video + right-playlist layout, custom window chrome, and an extensible SkinEngine trait.

**Architecture:** New `skin/` module with `SkinEngine` trait, `Qvod6Skin` implementation, and `Palette` constants. The existing `theme.rs` becomes a thin proxy. Controls, playlist, overlay, and player panels delegate all drawing to the active skin.

**Tech Stack:** Rust, egui, eframe (no new dependencies)

**Spec:** `docs/superpowers/specs/2026-07-03-qvod-gui-1x1-restoration-design.md`

## Global Constraints

- All skin draw methods must be pure functions w.r.t. the skin state (no mutations)
- The SkinEngine trait must NOT depend on egui types beyond `egui::Ui`, `egui::Color32`, `egui::Rect`, `egui::Painter`, `egui::Context`
- All new code must compile with `cargo clippy -- -D warnings` and `cargo fmt`
- Window must use `viewort.decorations = false` for custom chrome
- Minimum window size: 960x600
- Default window size: 1200x800

---

### Task 1: Skin Module Scaffolding

**Files:**
- Create: `crates/qvs-gui/src/skin/mod.rs`
- Create: `crates/qvs-gui/src/skin/palette.rs`
- Create: `crates/qvs-gui/src/skin/qvod6.rs`
- Modify: None yet (module will be wired later)

**Interfaces:**
- Consumes: Nothing yet
- Produces: `skin::palette` (color constants), `skin::SkinEngine` (trait), `skin::Qvod6Skin` (empty impl), `skin::TaskEntry`, `skin::TaskStatus`, `skin::TitleBarAction`, `skin::TaskAction`, `skin::ContextMenuAction`

- [ ] **Step 1: Create `palette.rs` with all color constants**

Write `crates/qvs-gui/src/skin/palette.rs`:

```rust
use eframe::egui::Color32;

pub const BG_GRADIENT_TOP: Color32 = Color32::from_rgb(0x1A, 0x1A, 0x2E);
pub const BG_GRADIENT_BOTTOM: Color32 = Color32::from_rgb(0x16, 0x21, 0x3E);
pub const VIDEO_BG: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
pub const CONTROL_BAR_BG: Color32 = Color32::from_rgb(0x0F, 0x0F, 0x23);
pub const SIDEBAR_BG: Color32 = Color32::from_rgb(0x1E, 0x1E, 0x30);
pub const TITLE_BAR_BG: Color32 = Color32::from_rgb(0x12, 0x12, 0x2A);
pub const TITLE_BAR_SEPARATOR: Color32 = Color32::from_rgb(0x2A, 0x2A, 0x44);

pub const BTN_DEFAULT: Color32 = Color32::from_rgb(0xE8, 0xE8, 0xE8);
pub const BTN_HOVER: Color32 = Color32::from_rgb(0x4F, 0xC3, 0xF7);
pub const BTN_ACTIVE: Color32 = Color32::from_rgb(0x02, 0x88, 0xD1);

pub const PROGRESS_BG: Color32 = Color32::from_rgb(0x4A, 0x4A, 0x6A);
pub const PROGRESS_FILL: Color32 = Color32::from_rgb(0x00, 0xBC, 0xD4);
pub const PROGRESS_BUFFERED: Color32 = Color32::from_rgb(0x3A, 0x3A, 0x5A);

pub const VOLUME_SLIDER_BG: Color32 = Color32::from_rgb(0x4A, 0x4A, 0x6A);
pub const VOLUME_SLIDER_FILL: Color32 = Color32::from_rgb(0x00, 0xBC, 0xD4);

pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xE0, 0xE0, 0xE0);
pub const TEXT_HIGHLIGHT: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(0x90, 0x90, 0x90);

pub const ERROR: Color32 = Color32::from_rgb(0xFF, 0x52, 0x52);
pub const SUCCESS: Color32 = Color32::from_rgb(0x69, 0xF0, 0xAE);
pub const WARNING: Color32 = Color32::from_rgb(0xFF, 0xC1, 0x07);

pub const TAB_ACTIVE_BG: Color32 = Color32::from_rgb(0x2A, 0x2A, 0x44);
pub const TAB_INACTIVE_BG: Color32 = Color32::from_rgb(0x16, 0x16, 0x28);

pub const LIST_ENTRY_HOVER: Color32 = Color32::from_rgb(0x2A, 0x2A, 0x44);
pub const LIST_ENTRY_SELECTED: Color32 = Color32::from_rgb(0x33, 0x33, 0x55);

pub const MENU_BG: Color32 = Color32::from_rgb(0x22, 0x22, 0x38);
pub const MENU_HOVER: Color32 = Color32::from_rgb(0x3A, 0x3A, 0x5A);
pub const MENU_SEPARATOR: Color32 = Color32::from_rgb(0x3A, 0x3A, 0x4A);

pub const OVERLAY_BG: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 160);
```

- [ ] **Step 2: Define shared types in `skin/mod.rs`**

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum TaskStatus {
    Downloading,
    Paused,
    Completed,
    Error(String),
}

#[derive(Debug, Clone)]
pub struct TaskEntry {
    pub title: String,
    pub uri: String,
    pub status: TaskStatus,
    pub progress: f64,
    pub downloaded: u64,
    pub total: u64,
    pub speed_down: f64,
    pub speed_up: f64,
}

#[derive(Debug)]
pub enum TitleBarAction {
    None,
    Minimize,
    Maximize,
    Close,
    Drag,
}

#[derive(Debug)]
pub enum TaskAction {
    None,
    Select(usize),
    Play(usize),
    Remove(usize),
    ContextMenu(usize),
}

#[derive(Debug)]
pub enum ContextMenuAction {
    Play,
    Pause,
    Stop,
    Restart,
    Remove,
    Properties,
    ToggleFullscreen,
    AspectRatio4x3,
    AspectRatio16x9,
    AspectRatioOriginal,
    OpenSettings,
    About,
    PriorityHigh,
    PriorityNormal,
    PriorityLow,
    SpeedLimit(u32),
}
```

- [ ] **Step 3: Define `SkinEngine` trait in `skin/mod.rs`**

```rust
use eframe::egui::{self, Context, Rect, Ui};

pub trait SkinEngine: Send + Sync {
    fn name(&self) -> &str;
    fn apply_style(&self, ctx: &Context);

    fn draw_title_bar(&self, ui: &mut Ui, title: &str) -> TitleBarAction;
    fn draw_play_button(&self, ui: &mut Ui, playing: bool) -> bool;
    fn draw_stop_button(&self, ui: &mut Ui) -> bool;
    fn draw_time_display(&self, ui: &mut Ui, position_ms: u64, duration_ms: u64);
    fn draw_progress_bar(&self, ui: &mut Ui, progress: f32, buffered: f32) -> Option<f32>;
    fn draw_volume_control(&self, ui: &mut Ui, volume: &mut f32, muted: &mut bool);
    fn draw_fullscreen_button(&self, ui: &mut Ui) -> bool;
    fn draw_tab_bar(&self, ui: &mut Ui, tabs: &[&str], active: &mut usize);
    fn draw_task_entry(&self, ui: &mut Ui, entry: &TaskEntry, index: usize, selected: bool) -> TaskAction;
    fn draw_buffering_overlay(&self, painter: &egui::Painter, area: Rect, time: f64);
    fn draw_error_overlay(&self, painter: &egui::Painter, area: Rect, msg: &str);
    fn draw_info_overlay(&self, painter: &egui::Painter, area: Rect, info: &str);
    fn draw_context_menu(&self, ui: &mut Ui, items: &[(&str, Vec<ContextMenuAction>)]) -> Option<ContextMenuAction>;
}
```

Then add `pub mod palette; pub mod qvod6;` and `pub use ...` re-exports.

- [ ] **Step 4: Create stub `Qvod6Skin` in `skin/qvod6.rs`**

```rust
use eframe::egui::{self, Context, Rect, Ui};
use super::{TaskEntry, TitleBarAction, TaskAction, ContextMenuAction, SkinEngine};

pub struct Qvod6Skin;

impl Qvod6Skin {
    pub fn new() -> Self {
        Self
    }
}

impl SkinEngine for Qvod6Skin {
    fn name(&self) -> &str { "Qvod 6.x" }
    fn apply_style(&self, ctx: &Context) { /* TBD in Task 2 */ }
    fn draw_title_bar(&self, _ui: &mut Ui, _title: &str) -> TitleBarAction { TitleBarAction::None }
    fn draw_play_button(&self, _ui: &mut Ui, _playing: bool) -> bool { false }
    fn draw_stop_button(&self, _ui: &mut Ui) -> bool { false }
    fn draw_time_display(&self, _ui: &mut Ui, _position_ms: u64, _duration_ms: u64) {}
    fn draw_progress_bar(&self, _ui: &mut Ui, _progress: f32, _buffered: f32) -> Option<f32> { None }
    fn draw_volume_control(&self, _ui: &mut Ui, _volume: &mut f32, _muted: &mut bool) {}
    fn draw_fullscreen_button(&self, _ui: &mut Ui) -> bool { false }
    fn draw_tab_bar(&self, _ui: &mut Ui, _tabs: &[&str], _active: &mut usize) {}
    fn draw_task_entry(&self, _ui: &mut Ui, _entry: &TaskEntry, _index: usize, _selected: bool) -> TaskAction { TaskAction::None }
    fn draw_buffering_overlay(&self, _painter: &egui::Painter, _area: Rect, _time: f64) {}
    fn draw_error_overlay(&self, _painter: &egui::Painter, _area: Rect, _msg: &str) {}
    fn draw_info_overlay(&self, _painter: &egui::Painter, _area: Rect, _info: &str) {}
    fn draw_context_menu(&self, _ui: &mut Ui, _items: &[(&str, Vec<ContextMenuAction>)]) -> Option<ContextMenuAction> { None }
}
```

- [ ] **Step 5: Compile and test**

```bash
cargo build --package qvs-gui
cargo test --package qvs-gui
cargo clippy --package qvs-gui -- -D warnings
```

---

### Task 2: Implement Qvod6Skin Drawing (Title Bar + Window Chrome)

**Files:**
- Modify: `crates/qvs-gui/src/skin/qvod6.rs`
- Modify: `crates/qvs-gui/src/main.rs` (decorations=false)
- Modify: `crates/qvs-gui/src/app.rs` (integrate skin, remove native title)

**Interfaces:**
- Consumes: `skin::palette`, `skin::SkinEngine`
- Produces: Working `draw_title_bar` implementation

- [ ] **Step 1: Implement `apply_style`**

```rust
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
```

- [ ] **Step 2: Implement `draw_title_bar`**

```rust
fn draw_title_bar(&self, ui: &mut Ui, title: &str) -> TitleBarAction {
    let height = 32.0;
    let rect = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::click_and_drag()).0;
    let painter = ui.painter_at(rect);

    // Background gradient
    painter.rect_filled(rect, 0.0, palette::TITLE_BAR_BG);
    // Bottom separator line
    painter.line_segment(
        [egui::pos2(rect.min.x, rect.max.y), egui::pos2(rect.max.x, rect.max.y)],
        egui::Stroke::new(1.0, palette::TITLE_BAR_SEPARATOR),
    );

    // App icon + title text
    painter.text(
        egui::pos2(rect.min.x + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        format!("[Q] {title}"),
        egui::FontId::proportional(13.0),
        palette::TEXT_PRIMARY,
    );

    // Window buttons (minimize, maximize, close)
    let btn_size = egui::vec2(34.0, 24.0);
    let btn_y = rect.center().y - btn_size.y / 2.0;

    let close_rect = egui::Rect::from_min_size(egui::pos2(rect.max.x - 34.0, btn_y), btn_size);
    let max_rect = egui::Rect::from_min_size(egui::pos2(rect.max.x - 68.0, btn_y), btn_size);
    let min_rect = egui::Rect::from_min_size(egui::pos2(rect.max.x - 102.0, btn_y), btn_size);

    let close_response = ui.allocate_rect_at(close_rect, egui::Sense::click());
    let max_response = ui.allocate_rect_at(max_rect, egui::Sense::click());
    let min_response = ui.allocate_rect_at(min_rect, egui::Sense::click());

    // Close button (red on hover)
    let close_color = if close_response.hovered() { palette::ERROR } else { palette::BTN_DEFAULT };
    painter.text(close_rect.center(), egui::Align2::CENTER_CENTER, "✕", egui::FontId::proportional(14.0), close_color);

    // Maximize button
    let max_color = if max_response.hovered() { palette::BTN_HOVER } else { palette::BTN_DEFAULT };
    painter.text(max_rect.center(), egui::Align2::CENTER_CENTER, "□", egui::FontId::proportional(14.0), max_color);

    // Minimize button
    let min_color = if min_response.hovered() { palette::BTN_HOVER } else { palette::BTN_DEFAULT };
    painter.text(min_rect.center(), egui::Align2::CENTER_CENTER, "─", egui::FontId::proportional(14.0), min_color);

    if close_response.clicked() { return TitleBarAction::Close; }
    if max_response.clicked() { return TitleBarAction::Maximize; }
    if min_response.clicked() { return TitleBarAction::Minimize; }
    if rect.contains(ui.input(|i| i.pointer.hover_pos().unwrap_or(egui::pos2(-1.0, -1.0)))) && ui.input(|i| i.pointer.any_down()) {
        return TitleBarAction::Drag;
    }

    TitleBarAction::None
}
```

- [ ] **Step 3: Update `main.rs` to disable native decorations**

```rust
let options = eframe::NativeOptions {
    viewport: egui::ViewportBuilder::default()
        .with_inner_size([1200.0, 800.0])
        .with_min_inner_size(960.0, 600.0)
        .with_decorations(false),
    ..Default::default()
};
```

- [ ] **Step 4: Wire skin into `app.rs`**

Add field: `skin: Box<dyn SkinEngine>` to `QvodApp`.
Initialize in `new()`: `skin: Box::new(Qvod6Skin::new())`.
In `update()`, call `self.skin.draw_title_bar(ui, "QVOD Player")` first thing.
Handle `TitleBarAction::Close` → `std::process::exit(0)`.
Handle `TitleBarAction::Drag` → `ctx.send_viewport_cmd(egui::ViewportCommand::BeginResizeOrDrag(egui::Dir2::RIGHT))`.

Wait, actually egui's drag needs to be done differently. For egui with `decorations: false`, you handle drag by:

```rust
if response.dragged() {
    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
}
```

And for close:
```rust
if action == TitleBarAction::Close {
    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
}
```

- [ ] **Step 5: Compile and test**

```bash
cargo build --package qvs-gui
cargo clippy --package qvs-gui -- -D warnings
```

---

### Task 3: Implement Control Bar Drawing

**Files:**
- Modify: `crates/qvs-gui/src/skin/qvod6.rs`
- Modify: `crates/qvs-gui/src/controls.rs`
- Modify: `crates/qvs-gui/src/app.rs`

**Interfaces:**
- Consumes: `SkinEngine::draw_play_button`, `draw_stop_button`, `draw_time_display`, `draw_progress_bar`, `draw_volume_control`, `draw_fullscreen_button`
- Produces: Fully custom-drawn control bar

- [ ] **Step 1: Implement all control bar draw methods in Qvod6Skin**

**`draw_play_button`:** Custom circular button, green ▶ when stopped, yellow ⏸ when playing. Hover changes to brighter shade. Button size 36x36.

```rust
fn draw_play_button(&self, ui: &mut Ui, playing: bool) -> bool {
    let size = egui::vec2(36.0, 36.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let painter = ui.painter_at(rect);

    let bg = if response.hovered() { palette::BTN_HOVER } else { palette::BTN_DEFAULT };
    painter.circle_filled(rect.center(), 16.0, bg);

    let text = if playing { "⏸" } else { "▶" };
    painter.text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(16.0), palette::BG_GRADIENT_TOP);

    response.clicked()
}
```

**`draw_stop_button`:** Square ■ button, same size.

```rust
fn draw_stop_button(&self, ui: &mut Ui) -> bool {
    let size = egui::vec2(36.0, 36.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let painter = ui.painter_at(rect);
    let color = if response.hovered() { palette::BTN_HOVER } else { palette::BTN_DEFAULT };
    painter.text(rect.center(), egui::Align2::CENTER_CENTER, "■", egui::FontId::proportional(18.0), color);
    response.clicked()
}
```

**`draw_time_display`:** `MM:SS / MM:SS` format.

```rust
fn draw_time_display(&self, ui: &mut Ui, position_ms: u64, duration_ms: u64) {
    let pos = format!("{:02}:{:02}", position_ms / 60000, (position_ms / 1000) % 60);
    let dur = format!("{:02}:{:02}", duration_ms / 60000, (duration_ms / 1000) % 60);
    ui.label(egui::RichText::new(format!("{pos} / {dur}")).color(palette::TEXT_PRIMARY).size(12.0));
}
```

**`draw_progress_bar`:** Custom drawn with two layers (buffered + played), draggable thumb. Returns `Some(new_progress)` if dragged, `None` otherwise.

```rust
fn draw_progress_bar(&self, ui: &mut Ui, progress: f32, buffered: f32) -> Option<f32> {
    let height = 8.0;
    let width = ui.available_width().max(100.0);
    let size = egui::vec2(width, height);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    // Background track
    painter.rect_filled(rect, 4.0, palette::PROGRESS_BG);

    // Buffered portion
    if buffered > 0.0 {
        let buf_rect = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(rect.width() * buffered.min(1.0), height),
        );
        painter.rect_filled(buf_rect, 4.0, palette::PROGRESS_BUFFERED);
    }

    // Played portion
    let fill_rect = egui::Rect::from_min_size(
        rect.min,
        egui::vec2(rect.width() * progress.min(1.0), height),
    );
    painter.rect_filled(fill_rect, 4.0, palette::PROGRESS_FILL);

    // Thumb (draggable circle)
    let thumb_x = rect.min.x + rect.width() * progress;
    let thumb_center = egui::pos2(thumb_x, rect.center().y);
    painter.circle_filled(thumb_center, 5.0, palette::BTN_HOVER);

    // Handle drag
    if let Some(mouse_pos) = response.interact_pointer_pos() {
        let rel_x = (mouse_pos.x - rect.min.x) / rect.width();
        let new_progress = rel_x.clamp(0.0, 1.0);
        return Some(new_progress);
    }

    None
}
```

**`draw_volume_control`:** Speaker icon + slider inline.

```rust
fn draw_volume_control(&self, ui: &mut Ui, volume: &mut f32, muted: &mut bool) {
    ui.horizontal(|ui| {
        let icon = if *muted { "🔇" } else if *volume < 0.33 { "🔈" } else if *volume < 0.66 { "🔉" } else { "🔊" };
        if ui.add(egui::Button::new(icon).min_size(egui::vec2(28.0, 28.0))).clicked() {
            *muted = !*muted;
        }

        let slider_width = 80.0;
        let slider_height = 6.0;
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(slider_width, slider_height),
            egui::Sense::click_and_drag(),
        );
        let painter = ui.painter_at(rect);

        painter.rect_filled(rect, 3.0, palette::VOLUME_SLIDER_BG);
        let effective_vol = if *muted { 0.0 } else { *volume };
        let fill_rect = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * effective_vol, slider_height));
        painter.rect_filled(fill_rect, 3.0, palette::VOLUME_SLIDER_FILL);

        let thumb_center = egui::pos2(rect.min.x + rect.width() * effective_vol, rect.center().y);
        painter.circle_filled(thumb_center, 4.0, palette::BTN_HOVER);

        if let Some(mouse_pos) = response.interact_pointer_pos() {
            let rel = ((mouse_pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
            *volume = rel;
            *muted = false;
        }
    });
}
```

**`draw_fullscreen_button`:** □ icon.

```rust
fn draw_fullscreen_button(&self, ui: &mut Ui) -> bool {
    let btn = egui::Button::new(egui::RichText::new("□").color(palette::BTN_DEFAULT))
        .min_size(egui::vec2(28.0, 28.0));
    ui.add(btn).clicked()
}
```

- [ ] **Step 2: Rewrite `PlayerControls::ui()` to use skin engine**

Add `skin: &dyn SkinEngine` parameter to `ui()`:

```rust
pub fn ui(&mut self, ui: &mut egui::Ui, skin: &dyn SkinEngine) {
    ui.horizontal(|ui| {
        ui.style_mut().spacing.item_spacing.x = 4.0;

        if skin.draw_play_button(ui, self.playing) {
            self.toggle_play();
        }
        if skin.draw_stop_button(ui) {
            self.reset();
        }

        ui.separator();

        skin.draw_time_display(ui, self.position_ms, self.duration_ms);

        if let Some(new_progress) = skin.draw_progress_bar(ui, self.progress(), self.buffered_progress()) {
            self.seek_to((new_progress * self.duration_ms as f32) as u64);
        }

        ui.separator();

        skin.draw_volume_control(ui, &mut self.volume, &mut self.muted);

        if skin.draw_fullscreen_button(ui) {
            // Toggle fullscreen
        }
    });
}
```

Add helper: `fn buffered_progress(&self) -> f32` returning `self.duration_ms > 0` then `self.buffered_seconds as f32 / (self.duration_ms as f32 / 1000.0)` else 0.0.

- [ ] **Step 3: Update `app.rs` to pass skin to controls**

In `update()`, change:
```rust
egui::TopBottomPanel::bottom("controls").show(ctx, |ui| {
    self.player.controls.ui(ui);
});
```
to:
```rust
egui::TopBottomPanel::bottom("controls")
    .min_height(48.0)
    .frame(egui::Frame::none().fill(palette::CONTROL_BAR_BG))
    .show(ctx, |ui| {
        self.player.controls.ui(ui, &*self.skin);
    });
```

- [ ] **Step 4: Compile and test**

```bash
cargo build --package qvs-gui
cargo test --package qvs-gui
cargo clippy --package qvs-gui -- -D warnings
```

---

### Task 4: Implement Right-Side Playlist (Dual Tab)

**Files:**
- Modify: `crates/qvs-gui/src/playlist.rs`
- Modify: `crates/qvs-gui/src/skin/qvod6.rs` (add draw_tab_bar, draw_task_entry)
- Modify: `crates/qvs-gui/src/app.rs`

**Interfaces:**
- Consumes: `SkinEngine::draw_tab_bar`, `draw_task_entry`
- Produces: Right-side panel with 正在播放/网络任务 tabs, each with scrollable task list

- [ ] **Step 1: Implement `draw_tab_bar` in Qvod6Skin**

```rust
fn draw_tab_bar(&self, ui: &mut Ui, tabs: &[&str], active: &mut usize) {
    ui.horizontal(|ui| {
        ui.style_mut().spacing.item_spacing.x = 0.0;
        for (i, tab) in tabs.iter().enumerate() {
            let bg = if i == *active { palette::TAB_ACTIVE_BG } else { palette::TAB_INACTIVE_BG };
            let response = ui.add(
                egui::Frame::none()
                    .fill(bg)
                    .inner_margin(egui::Margin::symmetric(12.0, 4.0))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new(*tab).color(palette::TEXT_PRIMARY).size(12.0));
                    })
                    .response
            );
            if response.clicked() {
                *active = i;
            }
        }
    });
}
```

- [ ] **Step 2: Implement `draw_task_entry` in Qvod6Skin**

```rust
fn draw_task_entry(&self, ui: &mut Ui, entry: &TaskEntry, _index: usize, selected: bool) -> TaskAction {
    let height = 52.0;
    let width = ui.available_width();
    let rect = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click()).0;
    let painter = ui.painter_at(rect);

    // Background
    let bg = if selected { palette::LIST_ENTRY_SELECTED } else if rect.contains(ui.input(|i| i.pointer.hover_pos().unwrap_or(egui::pos2(-1.0, -1.0)))) {
        palette::LIST_ENTRY_HOVER
    } else {
        egui::Color32::TRANSPARENT
    };
    painter.rect_filled(rect, 0.0, bg);

    // Status icon
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

    // Title
    painter.text(
        egui::pos2(rect.min.x + 28.0, rect.min.y + 8.0),
        egui::Align2::LEFT_TOP,
        &entry.title,
        egui::FontId::proportional(12.0),
        palette::TEXT_PRIMARY,
    );

    // Info line (size + speed)
    let info = match entry.status {
        TaskStatus::Downloading => {
            format!("{:.1}/{:.1}MB ↓{:.0}KB/s",
                entry.downloaded as f64 / 1048576.0,
                entry.total as f64 / 1048576.0,
                entry.speed_down / 1024.0)
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

    // Progress bar
    let bar_rect = egui::Rect::from_min_size(
        egui::pos2(rect.min.x + 8.0, rect.max.y - 8.0),
        egui::vec2(rect.width() - 16.0, 4.0),
    );
    painter.rect_filled(bar_rect, 2.0, palette::PROGRESS_BG);
    if entry.progress > 0.0 {
        let fill = egui::Rect::from_min_size(bar_rect.min, egui::vec2(bar_rect.width() * entry.progress as f32, 4.0));
        painter.rect_filled(fill, 2.0, palette::PROGRESS_FILL);
    }

    if selected && ui.input(|i| i.pointer.button_clicked(egui::PointerButton::Secondary)) {
        return TaskAction::ContextMenu(_index);
    }

    if rect.clicked() {
        return TaskAction::Select(_index);
    }

    TaskAction::None
}
```

- [ ] **Step 3: Rewrite `PlaylistManager`**

Add fields:
- `tab_active: usize` (0=正在播放, 1=网络任务)
- `entries: Vec<TaskEntry>`
- `selected: Option<usize>`

```rust
pub struct PlaylistManager {
    entries: Vec<TaskEntry>,
    selected: Option<usize>,
    tab_active: usize,
    history: Vec<String>,
}
```

Rewrite `ui()`:
```rust
pub fn ui(&mut self, ui: &mut egui::Ui, skin: &dyn SkinEngine) {
    let tabs = &["正在播放", "网络任务"];
    skin.draw_tab_bar(ui, tabs, &mut self.tab_active);

    ui.separator();

    egui::ScrollArea::vertical().show(ui, |ui| {
        let mut action: Option<TaskAction> = None;
        for (i, entry) in self.entries.iter().enumerate() {
            let result = skin.draw_task_entry(ui, entry, i, self.selected == Some(i));
            if !matches!(result, TaskAction::None) {
                action = Some(result);
            }
        }
        if let Some(act) = action {
            match act {
                TaskAction::Select(idx) => self.selected = Some(idx),
                TaskAction::ContextMenu(idx) => { /* show context menu */ }
                _ => {}
            }
        }
    });
}
```

- [ ] **Step 4: Wire right panel in `app.rs`**

Replace `AppPage::Playlist` handling with a right-side panel that's always visible:

```rust
egui::SidePanel::right("task_list")
    .resizable(true)
    .default_width(300.0)
    .min_width(280.0)
    .max_width(400.0)
    .frame(egui::Frame::none().fill(palette::SIDEBAR_BG))
    .show(ctx, |ui| {
        self.playlist.ui(ui, &*self.skin);
    });
```

Remove the old `AppPage::Playlist` tab and the `CentralPanel` sidebar, leaving only the video player in the central area.

- [ ] **Step 5: Compile and test**

```bash
cargo build --package qvs-gui
cargo clippy --package qvs-gui -- -D warnings
```

---

### Task 5: Overlays and Context Menus

**Files:**
- Modify: `crates/qvs-gui/src/skin/qvod6.rs`
- Modify: `crates/qvs-gui/src/overlay.rs`
- Modify: `crates/qvs-gui/src/app.rs`

**Interfaces:**
- Consumes: `SkinEngine::draw_buffering_overlay`, `draw_error_overlay`, `draw_info_overlay`, `draw_context_menu`
- Produces: Visual overlays + working context menus

- [ ] **Step 1: Implement overlay draw methods in Qvod6Skin**

**`draw_buffering_overlay`:** Draw a rotating arc animation. The `time` argument is seconds since start (for animation).

```rust
fn draw_buffering_overlay(&self, painter: &egui::Painter, area: Rect, time: f64) {
    // Semi-transparent background
    painter.rect_filled(area, 0.0, palette::OVERLAY_BG);

    // Rotating arcs (8 segments with varying alpha)
    let center = area.center();
    let radius = 20.0;
    let num_segments = 8;
    let angle_offset = (time * 3.0) % (std::f64::consts::TAU);

    for i in 0..num_segments {
        let angle = angle_offset + (i as f64 * std::f64::consts::TAU / num_segments as f64);
        let alpha = ((num_segments - i) as f32 / num_segments as f32);
        let color = egui::Color32::from_rgba_premultiplied(0x4F, 0xC3, 0xF7, (alpha * 200.0) as u8);

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

    // Text
    painter.text(
        egui::pos2(center.x, center.y + 35.0),
        egui::Align2::CENTER_CENTER,
        "缓冲中...",
        egui::FontId::proportional(14.0),
        palette::TEXT_PRIMARY,
    );
}
```

**`draw_error_overlay`:**
```rust
fn draw_error_overlay(&self, painter: &egui::Painter, area: Rect, msg: &str) {
    painter.rect_filled(area, 0.0, egui::Color32::from_rgba_premultiplied(0, 0, 0, 180));
    painter.text(
        area.center(),
        egui::Align2::CENTER_CENTER,
        format!("⚠ {msg}"),
        egui::FontId::proportional(18.0),
        palette::ERROR,
    );
}
```

**`draw_info_overlay`:**
```rust
fn draw_info_overlay(&self, painter: &egui::Painter, area: Rect, info: &str) {
    painter.text(
        egui::pos2(area.min.x + 8.0, area.min.y + 8.0),
        egui::Align2::LEFT_TOP,
        info,
        egui::FontId::proportional(12.0),
        palette::TEXT_SECONDARY,
    );
}
```

- [ ] **Step 2: Rewrite `overlay.rs` to delegate to skin**

```rust
pub fn draw(&mut self, ctx: &egui::Context, state: &PlayerState, skin: &dyn SkinEngine, video_area: Rect, time: f64) {
    match state {
        PlayerState::Buffering => {
            skin.draw_buffering_overlay(&ctx.request_discard().painter_at(video_area), video_area, time);
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        PlayerState::Error(msg) => {
            skin.draw_error_overlay(&ctx.painter_at(video_area), video_area, msg);
        }
        PlayerState::Paused => {
            skin.draw_info_overlay(&ctx.painter_at(video_area), video_area, "⏸ 已暂停");
        }
        _ => {}
    }
}
```

- [ ] **Step 3: Implement `draw_context_menu` in Qvod6Skin**

```rust
fn draw_context_menu(&self, ui: &mut Ui, items: &[(&str, Vec<ContextMenuAction>)]) -> Option<ContextMenuAction> {
    let mut result = None;
    egui::menu::menu_custom(ui, |ui| {
        for (group_label, actions) in items {
            if !group_label.is_empty() {
                ui.label(egui::RichText::new(*group_label).size(10.0).color(palette::TEXT_SECONDARY));
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
                    ContextMenuAction::SpeedLimit(v) => return,
                };
                if ui.add(egui::SelectableLabel::new(false, label)).clicked() {
                    result = Some(action.clone());
                    ui.close_menu();
                }
            }
            ui.separator();
        }
    });
    result
}
```

- [ ] **Step 4: Wire context menus into player and playlist**

In playlist, on right-click:
```rust
if let TaskAction::ContextMenu(idx) = action {
    let items = [("", vec![
        ContextMenuAction::Play, ContextMenuAction::Pause, ContextMenuAction::Stop,
        ContextMenuAction::Restart, ContextMenuAction::Remove,
    ]), ("", vec![ContextMenuAction::Properties])];
    let result = skin.draw_context_menu(ui, &items);
    // Handle result
}
```

- [ ] **Step 5: Add `video_area` tracking to `app.rs`**

Store the video display `Rect` from `player.ui()` output. Pass it to overlay.

- [ ] **Step 6: Compile and test**

```bash
cargo build --package qvs-gui
cargo clippy --package qvs-gui -- -D warnings
```

---

### Task 6: Final Integration and Cleanup

**Files:**
- Modify: `crates/qvs-gui/src/app.rs`
- Modify: `crates/qvs-gui/src/lib.rs`
- Remove: `crates/qvs-gui/src/theme.rs` (or slim to proxy)
- Modify: `crates/qvs-gui/src/player.rs`

- [ ] **Step 1: Slim `theme.rs` to a thin proxy**

```rust
use crate::skin::Qvod6Skin;
use eframe::egui;

pub fn apply_skin(ctx: &egui::Context) {
    let skin = Qvod6Skin::new();
    skin.apply_style(ctx);
}
```

Or just deprecate it and remove the `#[allow]` attributes that no longer apply.

- [ ] **Step 2: Update `lib.rs` to export `skin` module**

```rust
pub mod skin;
```

- [ ] **Step 3: Clean up test expectations in all modules**

Update any tests in `controls.rs`, `playlist.rs`, `overlay.rs`, `app.rs` that may have broken due to API changes (e.g., `PlayerControls::ui()` now takes a `&dyn SkinEngine` parameter).

- [ ] **Step 4: Final build verification**

```bash
cargo build --package qvs-gui
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check
```

- [ ] **Step 5: Commit**

```bash
git add crates/qvs-gui/src/skin/ crates/qvs-gui/src/app.rs crates/qvs-gui/src/controls.rs crates/qvs-gui/src/playlist.rs crates/qvs-gui/src/overlay.rs crates/qvs-gui/src/main.rs crates/qvs-gui/src/lib.rs docs/superpowers/specs/2026-07-03-qvod-gui-1x1-restoration-design.md docs/superpowers/plans/2026-07-03-qvod-gui-1x1-restoration.md
git commit -m "feat(gui): 1:1 Qvod 6.x GUI restoration with SkinEngine"
```
