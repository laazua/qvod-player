use eframe::egui::{self, Context, Rect, Ui};

use super::{ContextMenuAction, SkinEngine, TaskAction, TaskEntry, TitleBarAction};

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

    fn apply_style(&self, _ctx: &Context) {}

    fn draw_title_bar(&self, _ui: &mut Ui, _title: &str) -> TitleBarAction {
        TitleBarAction::None
    }

    fn draw_play_button(&self, _ui: &mut Ui, _playing: bool) -> bool {
        false
    }

    fn draw_stop_button(&self, _ui: &mut Ui) -> bool {
        false
    }

    fn draw_time_display(&self, _ui: &mut Ui, _position_ms: u64, _duration_ms: u64) {}

    fn draw_progress_bar(&self, _ui: &mut Ui, _progress: f32, _buffered: f32) -> Option<f32> {
        None
    }

    fn draw_volume_control(&self, _ui: &mut Ui, _volume: &mut f32, _muted: &mut bool) {}

    fn draw_fullscreen_button(&self, _ui: &mut Ui) -> bool {
        false
    }

    fn draw_tab_bar(&self, _ui: &mut Ui, _tabs: &[&str], _active: &mut usize) {}

    fn draw_task_entry(&self, _ui: &mut Ui, _entry: &TaskEntry, _index: usize, _selected: bool) -> TaskAction {
        TaskAction::None
    }

    fn draw_buffering_overlay(&self, _painter: &egui::Painter, _area: Rect, _time: f64) {}

    fn draw_error_overlay(&self, _painter: &egui::Painter, _area: Rect, _msg: &str) {}

    fn draw_info_overlay(&self, _painter: &egui::Painter, _area: Rect, _info: &str) {}

    fn draw_context_menu(&self, _ui: &mut Ui, _items: &[(&str, Vec<ContextMenuAction>)]) -> Option<ContextMenuAction> {
        None
    }
}
