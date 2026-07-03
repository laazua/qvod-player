use eframe::egui::{self, Color32, Visuals};

/// Deprecated: being superseded by [`crate::skin::SkinEngine`].
/// Kept for backward compatibility with settings.rs.
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
        let mut style = (*ctx.style()).clone();
        match self {
            QvodTheme::Dark => {
                style.visuals = Visuals::dark();
                style.visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(30, 30, 30);
                style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(50, 50, 50);
                style.visuals.widgets.active.bg_fill = Color32::from_rgb(70, 70, 70);
                style.visuals.override_text_color = Some(Color32::WHITE);
                style.visuals.window_fill = Color32::from_rgb(25, 25, 25);
                style.visuals.panel_fill = Color32::from_rgb(35, 35, 35);
            }
            QvodTheme::Light => {
                style.visuals = Visuals::light();
                style.visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(240, 240, 240);
                style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(220, 220, 220);
                style.visuals.widgets.active.bg_fill = Color32::from_rgb(200, 200, 200);
                style.visuals.override_text_color = Some(Color32::BLACK);
                style.visuals.window_fill = Color32::from_rgb(245, 245, 245);
                style.visuals.panel_fill = Color32::from_rgb(235, 235, 235);
            }
            QvodTheme::System => {
                let is_dark = ctx.style().visuals.dark_mode;
                if is_dark {
                    QvodTheme::Dark.apply(ctx);
                } else {
                    QvodTheme::Light.apply(ctx);
                }
                return;
            }
        }
        ctx.set_style(style);
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
