pub mod palette;
pub mod qvod6;

#[derive(Debug, Clone, PartialEq)]
pub enum TaskStatus {
    Downloading,
    Paused,
    Completed,
    Error(String),
}

impl Default for TaskStatus {
    fn default() -> Self {
        Self::Downloading
    }
}

#[derive(Debug, Clone, Default)]
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

#[derive(Debug, Clone)]
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
}

use eframe::egui::{self, Context, Pos2, Rect, Ui};

pub trait SkinEngine: Send + Sync {
    fn name(&self) -> &'static str;
    fn apply_style(&self, ctx: &Context);

    fn draw_title_bar(&self, ui: &mut Ui, title: &str) -> TitleBarAction;
    fn draw_play_button(&self, ui: &mut Ui, playing: bool) -> bool;
    fn draw_stop_button(&self, ui: &mut Ui) -> bool;
    fn draw_time_display(&self, ui: &mut Ui, position_ms: u64, duration_ms: u64);
    fn draw_progress_bar(&self, ui: &mut Ui, progress: f32, buffered: f32) -> Option<f32>;
    fn draw_volume_control(&self, ui: &mut Ui, volume: &mut f32, muted: &mut bool);
    fn draw_fullscreen_button(&self, ui: &mut Ui) -> bool;
    fn draw_tab_bar(&self, ui: &mut Ui, tabs: &[&str], active: &mut usize);
    fn draw_task_entry(
        &self,
        ui: &mut Ui,
        entry: &TaskEntry,
        index: usize,
        selected: bool,
    ) -> TaskAction;
    fn draw_buffering_overlay(&self, painter: &egui::Painter, area: Rect, time: f64);
    fn draw_error_overlay(&self, painter: &egui::Painter, area: Rect, msg: &str);
    fn draw_info_overlay(&self, painter: &egui::Painter, area: Rect, info: &str);
    fn draw_context_menu(
        &self,
        ui: &mut Ui,
        pos: Pos2,
        items: &[(&str, Vec<ContextMenuAction>)],
    ) -> Option<ContextMenuAction>;
}

pub use palette::*;
pub use qvod6::Qvod6Skin;
