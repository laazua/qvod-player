use eframe::egui;

use crate::skin::{Qvod6Skin, SkinEngine};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QvodTheme {
    Dark,
    Light,
    System,
}

impl Default for QvodTheme {
    fn default() -> Self {
        Self::Dark
    }
}

impl QvodTheme {
    pub fn apply(&self, ctx: &egui::Context) {
        Qvod6Skin::new().apply_style(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_theme() {
        assert_eq!(QvodTheme::default(), QvodTheme::Dark);
    }
}
