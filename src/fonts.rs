//! The fonts: Google Sans for the window and JetBrains Mono for code, both embedded so the editor
//! looks the same on every machine.

use aimer::style::TextStyle;
use aimer::{FontFamily, FontRegistration, FontRegistry, FontStyle, FontWeight};

const CODE: &str = "JetBrains Mono";
const UI: &str = "Google Sans";

/// The faces of a family: the bytes, the weight and the style each is registered as.
type Faces = [(&'static [u8], FontWeight, FontStyle)];

const CODE_FACES: &Faces = &[
    (
        include_bytes!("../assets/JetBrainsMono-2.304/fonts/ttf/JetBrainsMono-Regular.ttf"),
        FontWeight::Normal,
        FontStyle::Normal,
    ),
    (
        include_bytes!("../assets/JetBrainsMono-2.304/fonts/ttf/JetBrainsMono-Bold.ttf"),
        FontWeight::Bold,
        FontStyle::Normal,
    ),
    (
        include_bytes!("../assets/JetBrainsMono-2.304/fonts/ttf/JetBrainsMono-Italic.ttf"),
        FontWeight::Normal,
        FontStyle::Italic,
    ),
    (
        include_bytes!("../assets/JetBrainsMono-2.304/fonts/ttf/JetBrainsMono-BoldItalic.ttf"),
        FontWeight::Bold,
        FontStyle::Italic,
    ),
];

const UI_FACES: &Faces = &[
    (
        include_bytes!("../assets/Google_Sans/static/GoogleSans-Regular.ttf"),
        FontWeight::Normal,
        FontStyle::Normal,
    ),
    (
        include_bytes!("../assets/Google_Sans/static/GoogleSans-Medium.ttf"),
        FontWeight::Value(500),
        FontStyle::Normal,
    ),
    (
        include_bytes!("../assets/Google_Sans/static/GoogleSans-Bold.ttf"),
        FontWeight::Bold,
        FontStyle::Normal,
    ),
];

/// Registers the faces of both families. Call it once, before the app starts; calling it again does
/// nothing. A face that cannot be registered is reported, and text set in its family falls back to the
/// generic fonts ([`ui_family`], [`code_family`]).
pub fn register() {
    register_family(UI, UI_FACES);
    register_family(CODE, CODE_FACES);
}

fn register_family(family: &str, faces: &Faces) {
    if FontRegistry::family(family).is_some() {
        return;
    }
    for (bytes, weight, style) in faces {
        if let Err(error) = FontRegistry::register(FontRegistration {
            family,
            bytes,
            weight: *weight,
            style: *style,
        }) {
            aimer::console::warn!("Could not register {family}: {error:?}");
        }
    }
}

/// The family code is set in: JetBrains Mono, or the generic monospace font if it is not registered.
pub fn code_family() -> FontFamily {
    FontRegistry::family(CODE).unwrap_or(FontFamily::MONOSPACE)
}

/// The family the window is set in: Google Sans, or the generic sans-serif font if it is not registered.
pub fn ui_family() -> FontFamily {
    FontRegistry::family(UI).unwrap_or(FontFamily::SANS_SERIF)
}

/// A text style for the window's own text, in Google Sans. Every piece of text that is not code starts
/// from this, since the framework has no way to change the font of all of it at once.
pub fn ui() -> TextStyle {
    TextStyle::new().font_family(ui_family())
}
