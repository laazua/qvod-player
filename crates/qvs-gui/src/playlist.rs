use std::time::Instant;

use eframe::egui;

const MAX_HISTORY: usize = 100;

#[derive(Debug, Clone)]
pub struct PlaylistEntry {
    pub uri: String,
    pub title: String,
    pub duration_ms: u64,
    pub added_at: Instant,
}

impl PlaylistEntry {
    #[must_use]
    pub fn new(uri: String, title: String) -> Self {
        Self {
            uri,
            title,
            duration_ms: 0,
            added_at: Instant::now(),
        }
    }
}

#[derive(Debug)]
pub struct PlaylistManager {
    entries: Vec<PlaylistEntry>,
    current_index: Option<usize>,
    history: Vec<String>,
}

impl PlaylistManager {
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            current_index: None,
            history: Vec::with_capacity(MAX_HISTORY),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("➕ Add").clicked() {
                // TODO: Show file picker or URI input
            }
            if ui.button("🗑 Clear").clicked() {
                self.entries.clear();
                self.current_index = None;
            }
            if let Some(idx) = self.current_index {
                if ui.button("✕ Remove").clicked() {
                    self.entries.remove(idx);
                    self.current_index = None;
                }
                if ui.button("▶ Play").clicked() {
                    // Play selected entry
                }
            }
        });

        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut to_remove: Option<usize> = None;
            for (idx, entry) in self.entries.iter().enumerate() {
                let selected = self.current_index == Some(idx);
                let response = ui.selectable_label(selected, &entry.title);
                if response.clicked() {
                    self.current_index = Some(idx);
                }
                response.context_menu(|ui| {
                    if ui.button("Play").clicked() {
                        self.current_index = Some(idx);
                        ui.close_menu();
                    }
                    if ui.button("Copy URI").clicked() {
                        ui.close_menu();
                    }
                    if ui.button("Remove").clicked() {
                        to_remove = Some(idx);
                        ui.close_menu();
                    }
                });
            }
            if let Some(idx) = to_remove {
                self.entries.remove(idx);
                if self.current_index == Some(idx) {
                    self.current_index = None;
                } else if let Some(cur) = self.current_index {
                    if idx < cur {
                        self.current_index = Some(cur - 1);
                    }
                }
            }
        });
    }

    pub fn add(&mut self, entry: PlaylistEntry) {
        self.entries.push(entry);
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

    pub fn clear(&mut self) {
        self.entries.clear();
        self.current_index = None;
    }

    pub fn play(&mut self, index: usize) -> Option<&PlaylistEntry> {
        if index < self.entries.len() {
            self.current_index = Some(index);
            let entry = &self.entries[index];
            self.history.push(entry.uri.clone());
            if self.history.len() > MAX_HISTORY {
                self.history.remove(0);
            }
            Some(entry)
        } else {
            None
        }
    }

    #[must_use]
    pub fn current(&self) -> Option<&PlaylistEntry> {
        self.current_index.map(|i| &self.entries[i])
    }

    #[must_use]
    pub fn entries(&self) -> &[PlaylistEntry] {
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
            if let Some(current) = self.current_index {
                if current == index {
                    self.current_index = Some(index - 1);
                } else if current == index - 1 {
                    self.current_index = Some(index);
                }
            }
        }
    }

    pub fn move_down(&mut self, index: usize) {
        if index + 1 < self.entries.len() {
            self.entries.swap(index, index + 1);
            if let Some(current) = self.current_index {
                if current == index {
                    self.current_index = Some(index + 1);
                } else if current == index + 1 {
                    self.current_index = Some(index);
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

    #[test]
    fn test_add_and_play() {
        let mut pm = PlaylistManager::new();
        pm.add(PlaylistEntry::new(
            "qvod://hash|test.mp4|1024|mp4|".into(),
            "Test".into(),
        ));
        assert_eq!(pm.len(), 1);
        let entry = pm.play(0);
        assert!(entry.is_some());
        assert_eq!(pm.current().unwrap().title, "Test");
    }

    #[test]
    fn test_remove() {
        let mut pm = PlaylistManager::new();
        pm.add(PlaylistEntry::new("uri1".into(), "A".into()));
        pm.add(PlaylistEntry::new("uri2".into(), "B".into()));
        pm.remove(1);
        assert_eq!(pm.len(), 1);
    }

    #[test]
    fn test_clear() {
        let mut pm = PlaylistManager::new();
        pm.add(PlaylistEntry::new("uri".into(), "A".into()));
        pm.clear();
        assert!(pm.is_empty());
    }

    #[test]
    fn test_move_up() {
        let mut pm = PlaylistManager::new();
        pm.add(PlaylistEntry::new("uri1".into(), "A".into()));
        pm.add(PlaylistEntry::new("uri2".into(), "B".into()));
        pm.move_up(1);
        assert_eq!(pm.entries()[0].title, "B");
    }

    #[test]
    fn test_move_down() {
        let mut pm = PlaylistManager::new();
        pm.add(PlaylistEntry::new("uri1".into(), "A".into()));
        pm.add(PlaylistEntry::new("uri2".into(), "B".into()));
        pm.move_down(0);
        assert_eq!(pm.entries()[0].title, "B");
    }

    #[test]
    fn test_history() {
        let mut pm = PlaylistManager::new();
        pm.add(PlaylistEntry::new("uri1".into(), "A".into()));
        pm.add(PlaylistEntry::new("uri2".into(), "B".into()));
        pm.play(0);
        pm.play(1);
        assert_eq!(pm.history().len(), 2);
    }
}
