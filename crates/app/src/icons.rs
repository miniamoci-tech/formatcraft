//! Lucide icons (ISC licence, see ATTRIBUTION.toml), drawn from the bundled
//! icon font. Codepoints come from lucide-static 1.52.0 `font/info.json`.

use eframe::egui;

static LUCIDE_TTF: &[u8] = include_bytes!("../../../assets/fonts/lucide.ttf");

pub const PLAY: &str = "\u{e13c}";
pub const STOP: &str = "\u{e167}";
pub const ERASER: &str = "\u{e28f}";
pub const FOLDER: &str = "\u{e0d7}";
pub const FOLDER_OPEN: &str = "\u{e247}";
pub const FOLDER_PLUS: &str = "\u{e0d9}";
pub const SETTINGS: &str = "\u{e154}";
pub const INFO: &str = "\u{e0f9}";
pub const FILM: &str = "\u{e0d0}";
pub const MUSIC: &str = "\u{e122}";
pub const IMAGE: &str = "\u{e0f6}";
pub const FILE_TEXT: &str = "\u{e0cc}";
pub const WRENCH: &str = "\u{e1b1}";
pub const CLAPPERBOARD: &str = "\u{e29b}";
pub const PLUS: &str = "\u{e13d}";
pub const ARROW_UP: &str = "\u{e04a}";
pub const ARROW_DOWN: &str = "\u{e042}";
pub const X: &str = "\u{e1b2}";
pub const RETRY: &str = "\u{e148}";
pub const COPY: &str = "\u{e09e}";
pub const CHECK: &str = "\u{e06c}";
pub const ALERT: &str = "\u{e077}";
pub const SCISSORS: &str = "\u{e14e}";
pub const COMBINE: &str = "\u{e44c}";
pub const MERGE: &str = "\u{e0e4}";
pub const RECORD: &str = "\u{e345}";
pub const SHRINK: &str = "\u{e220}";
pub const SPLIT: &str = "\u{e440}";
pub const WARNING: &str = "\u{e193}";
pub const LAYERS: &str = "\u{e529}";
pub const SLIDERS: &str = "\u{e29a}";
pub const AUDIO_LINES: &str = "\u{e55a}";
pub const UPLOAD: &str = "\u{e19e}";
pub const CIRCLE_STOP: &str = "\u{e083}";

/// Adds Lucide as a fallback to both font families.
pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "lucide".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(LUCIDE_TTF)),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        if let Some(list) = fonts.families.get_mut(&family) {
            list.push("lucide".to_owned());
        }
    }
    ctx.set_fonts(fonts);
}
