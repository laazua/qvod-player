//! CJK (Chinese/Japanese/Korean) font configuration for egui.
//!
//! egui's built-in fonts (Hack, Ubuntu-Light) do not contain CJK glyphs.
//! On Windows builds, Chinese text renders as empty boxes without this setup.
//!
//! This module attempts to locate a system CJK font at runtime and adds it as
//! a fallback for both the `Proportional` and `Monospace` font families.

use std::path::Path;

use eframe::egui::{self, FontData, FontDefinitions};

/// Register a CJK-capable system font as fallback in egui's font definitions.
///
/// Call this once during application startup, inside the `eframe::run_native`
/// closure where the [`egui::Context`] is available.
pub fn setup_cjk_fonts(ctx: &egui::Context) {
    let Some(font_bytes) = load_system_cjk_font() else {
        tracing::warn!("No system CJK font found — Chinese text may not render correctly");
        return;
    };

    let mut fonts = FontDefinitions::default();

    fonts
        .font_data
        .insert("cjk".to_owned(), FontData::from_owned(font_bytes));

    // Insert CJK font as the **last** fallback so Latin glyphs still come from
    // the primary fonts (Hack / Ubuntu-Light), and only characters missing from
    // those fonts (CJK, emoji extensions, …) fall through to the CJK face.
    for family in &[egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        if let Some(fonts_for_family) = fonts.families.get_mut(family) {
            fonts_for_family.push("cjk".to_owned());
        }
    }

    ctx.set_fonts(fonts);
    tracing::info!("System CJK font loaded and registered with egui");
}

// ---------------------------------------------------------------------------
// Platform-specific font discovery
// ---------------------------------------------------------------------------

/// Try to locate and read a system CJK font, returning its raw bytes.
#[cfg(target_os = "windows")]
fn load_system_cjk_font() -> Option<Vec<u8>> {
    // Priority order: Microsoft YaHei UI (Win 8+), Microsoft YaHei (Win 7+), SimSun
    let candidates = &[
        r"C:\Windows\Fonts\msyh.ttc",   // Microsoft YaHei regular
        r"C:\Windows\Fonts\msyhbd.ttc", // Microsoft YaHei bold
        r"C:\Windows\Fonts\msyhl.ttc",  // Microsoft YaHei light
        r"C:\Windows\Fonts\simsun.ttc", // SimSun (fallback)
    ];

    for path in candidates {
        if Path::new(path).exists() {
            match std::fs::read(path) {
                Ok(data) => {
                    tracing::debug!("Loaded CJK font from {path}");
                    return Some(data);
                }
                Err(e) => {
                    tracing::warn!("Found {path} but failed to read: {e}");
                }
            }
        }
    }

    None
}

#[cfg(target_os = "macos")]
fn load_system_cjk_font() -> Option<Vec<u8>> {
    // Priority: PingFang SC (macOS 10.11+), STHeiti, Apple LiGothic
    let candidates = &[
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/System/Library/Fonts/Apple LiGothic.ttf",
        // Fallback for older macOS versions
        "/Library/Fonts/Arial Unicode.ttf",
    ];

    for path in candidates {
        if Path::new(path).exists() {
            match std::fs::read(path) {
                Ok(data) => {
                    tracing::debug!("Loaded CJK font from {path}");
                    return Some(data);
                }
                Err(e) => {
                    tracing::warn!("Found {path} but failed to read: {e}");
                }
            }
        }
    }

    None
}

#[cfg(target_os = "linux")]
fn load_system_cjk_font() -> Option<Vec<u8>> {
    // On Linux, CJK font locations vary wildly.  We search well-known paths
    // first, then fall back to a recursive scan of common font directories.
    let well_known = &[
        // Noto Sans CJK (various distro layouts)
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/google-noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto/NotoSansCJK-Regular.ttc",
        // WenQuanYi Micro Hei / Zen Hei
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
        "/usr/share/fonts/wqy-microhei/wqy-microhei.ttc",
        // Droid Sans Fallback (older Android-based Linux)
        "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
        // Source Han Sans (Adobe)
        "/usr/share/fonts/opentype/source-han-sans/SourceHanSansSC-Regular.otf",
        "/usr/share/fonts/truetype/source-han-sans/SourceHanSansSC-Regular.otf",
    ];

    for path in well_known {
        if Path::new(path).exists() {
            match std::fs::read(path) {
                Ok(data) => {
                    tracing::debug!("Loaded CJK font from {path}");
                    return Some(data);
                }
                Err(e) => {
                    tracing::warn!("Found {path} but failed to read: {e}");
                }
            }
        }
    }

    // Recursive fallback scan of standard font directories
    let search_dirs = &["/usr/share/fonts", "/usr/local/share/fonts"];

    for dir in search_dirs {
        if let Ok(dir) = std::fs::read_dir(dir) {
            for entry in dir.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(str::to_lowercase)
                    .unwrap_or_default();

                // Match common CJK font name patterns
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(str::to_ascii_lowercase);

                let is_truetype = ext.as_deref() == Some("ttf");
                let is_opentype = ext.as_deref() == Some("otf");

                let is_cjk =
                    name.contains("noto") && name.contains("cjk") && name.contains("regular")
                        || name.contains("wqy")
                        || name.contains("wenquanyi")
                        || name.contains("droid") && name.contains("fallback")
                        || name.contains("sourcehan")
                        || name.contains("noto")
                            && (name.contains("sc") || name.contains("cn"))
                            && (is_truetype || is_opentype);

                if is_cjk {
                    match std::fs::read(&path) {
                        Ok(data) => {
                            tracing::debug!("Loaded CJK font from {path:?}");
                            return Some(data);
                        }
                        Err(e) => {
                            tracing::warn!("Found {path:?} but failed to read: {e}");
                        }
                    }
                }
            }
        }
    }

    None
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn load_system_cjk_font() -> Option<Vec<u8>> {
    None
}
