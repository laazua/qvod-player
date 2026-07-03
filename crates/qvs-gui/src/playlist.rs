use eframe::egui;

use crate::skin::{ContextMenuAction, SkinEngine, TaskAction, TaskEntry};

#[derive(Debug)]
pub struct PlaylistManager {
    entries: Vec<TaskEntry>,
    selected: Option<usize>,
    tab_active: usize,
    history: Vec<String>,
    show_context_menu: Option<(usize, egui::Pos2)>,
}

impl PlaylistManager {
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            selected: None,
            tab_active: 0,
            history: Vec::new(),
            show_context_menu: None,
        }
    }

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
                    TaskAction::ContextMenu(idx) => {
                        let cursor_pos = ui.input(|i| i.pointer.interact_pos()).unwrap_or_default();
                        self.show_context_menu = Some((idx, cursor_pos));
                    }
                    _ => {}
                }
            }
        });

        if let Some((idx, pos)) = self.show_context_menu {
            let items = [
                (
                    "",
                    vec![
                        ContextMenuAction::Play,
                        ContextMenuAction::Pause,
                        ContextMenuAction::Stop,
                        ContextMenuAction::Restart,
                        ContextMenuAction::Remove,
                    ],
                ),
                ("", vec![ContextMenuAction::Properties]),
            ];
            let result = skin.draw_context_menu(ui, pos, &items);
            match result {
                Some(ContextMenuAction::Play) => {
                    self.show_context_menu = None;
                }
                Some(ContextMenuAction::Remove) => {
                    self.show_context_menu = None;
                    self.entries.remove(idx);
                    self.selected = None;
                }
                Some(ContextMenuAction::Properties) => {
                    self.show_context_menu = None;
                }
                Some(_) => {
                    self.show_context_menu = None;
                }
                None => {
                    if ui.input(|i| i.pointer.any_click()) {
                        self.show_context_menu = None;
                    }
                }
            }
        }
    }

    pub fn add(&mut self, entry: TaskEntry) {
        self.entries.push(entry);
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.entries.len() {
            self.entries.remove(index);
            if let Some(selected) = self.selected {
                if index < selected {
                    self.selected = Some(selected - 1);
                } else if index == selected {
                    self.selected = None;
                }
            }
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.selected = None;
    }

    pub fn play(&mut self, index: usize) -> Option<&TaskEntry> {
        if index < self.entries.len() {
            self.selected = Some(index);
            let entry = &self.entries[index];
            self.history.push(entry.uri.clone());
            if self.history.len() > 100 {
                self.history.remove(0);
            }
            Some(entry)
        } else {
            None
        }
    }

    #[must_use]
    pub fn current(&self) -> Option<&TaskEntry> {
        self.selected.map(|i| &self.entries[i])
    }

    #[must_use]
    pub fn entries(&self) -> &[TaskEntry] {
        &self.entries
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn move_up(&mut self, index: usize) {
        if index > 0 && index < self.entries.len() {
            self.entries.swap(index, index - 1);
            if let Some(selected) = self.selected {
                if selected == index {
                    self.selected = Some(index - 1);
                } else if selected == index - 1 {
                    self.selected = Some(index);
                }
            }
        }
    }

    pub fn move_down(&mut self, index: usize) {
        if index + 1 < self.entries.len() {
            self.entries.swap(index, index + 1);
            if let Some(selected) = self.selected {
                if selected == index {
                    self.selected = Some(index + 1);
                } else if selected == index + 1 {
                    self.selected = Some(index);
                }
            }
        }
    }

    #[must_use]
    pub fn history(&self) -> &[String] {
        &self.history
    }

    pub fn set_history(&mut self, history: Vec<String>) {
        self.history = history;
    }
}

impl Default for PlaylistManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skin::TaskStatus;

    fn make_entry(title: &str, uri: &str) -> TaskEntry {
        TaskEntry {
            title: title.into(),
            uri: uri.into(),
            status: TaskStatus::Downloading,
            progress: 0.0,
            downloaded: 0,
            total: 0,
            speed_down: 0.0,
            speed_up: 0.0,
        }
    }

    #[test]
    fn test_add_and_play() {
        let mut pm = PlaylistManager::new();
        pm.add(make_entry("Test", "qvod://hash|test.mp4|1024|mp4|"));
        assert_eq!(pm.len(), 1);
        let entry = pm.play(0);
        assert!(entry.is_some());
        assert_eq!(pm.current().unwrap().title, "Test");
    }

    #[test]
    fn test_remove() {
        let mut pm = PlaylistManager::new();
        pm.add(make_entry("A", "uri1"));
        pm.add(make_entry("B", "uri2"));
        pm.remove(1);
        assert_eq!(pm.len(), 1);
    }

    #[test]
    fn test_clear() {
        let mut pm = PlaylistManager::new();
        pm.add(make_entry("A", "uri"));
        pm.clear();
        assert!(pm.is_empty());
    }

    #[test]
    fn test_move_up() {
        let mut pm = PlaylistManager::new();
        pm.add(make_entry("A", "uri1"));
        pm.add(make_entry("B", "uri2"));
        pm.move_up(1);
        assert_eq!(pm.entries()[0].title, "B");
    }

    #[test]
    fn test_move_down() {
        let mut pm = PlaylistManager::new();
        pm.add(make_entry("A", "uri1"));
        pm.add(make_entry("B", "uri2"));
        pm.move_down(0);
        assert_eq!(pm.entries()[0].title, "B");
    }

    #[test]
    fn test_history() {
        let mut pm = PlaylistManager::new();
        pm.add(make_entry("A", "uri1"));
        pm.add(make_entry("B", "uri2"));
        pm.play(0);
        pm.play(1);
        assert_eq!(pm.history().len(), 2);
    }
}
