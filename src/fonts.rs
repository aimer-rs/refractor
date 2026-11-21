//! The fonts: Inter for the window and JetBrains Mono for code. Both are variable fonts, so
//! one file per style serves every weight, and both are listed under `[fonts]` in `Aimer.toml`
//! rather than embedded in the code.

use aimer::style::TextStyle;
use aimer::{BundleFont, FontFamily, FontStyle, FontWeight};

/// Typed handles for the entries under `[fonts]` in `Aimer.toml`.
#[aimer::assets]
struct Assets;

const CODE: &str = "JetBrains Mono";
const UI: &str = "Inter";

/// The faces of a family: an upright and an italic variable font. Each registers once, as regular
/// weight; the weight a piece of text asks for selects the `wght` instance of that face.
fn faces(family: &'static str) -> [BundleFont; 2] {
    let (upright, italic) = if family == UI {
        (Assets::UI_FONT, Assets::UI_FONT_ITALIC)
    } else {
        (Assets::MONO_FONT, Assets::MONO_FONT_ITALIC)
    };
    [
        upright
            .family(family)
            .weight(FontWeight::Normal)
            .style(FontStyle::Normal),
        italic
            .family(family)
            .weight(FontWeight::Normal)
            .style(FontStyle::Italic),
    ]
}

/// Registers the faces of both families. Call it once, before the app starts; calling it again does
/// nothing. A face that cannot be registered is reported, and text set in its family falls back to the
/// generic fonts ([`ui_family`], [`code_family`]).
pub fn register() {
    for family in [UI, CODE] {
        for face in faces(family) {
            if let Err(error) = face.register() {
                aimer::console::warn!("Could not register {family} ({}): {error:?}", face.path());
            }
        }
    }
}

/// The family code is set in: JetBrains Mono, or the generic monospace font if it is not registered.
pub fn code_family() -> FontFamily {
    aimer::FontRegistry::family(CODE).unwrap_or(FontFamily::MONOSPACE)
}

/// The family the window is set in: Inter, or the generic sans-serif font if it is not registered.
pub fn ui_family() -> FontFamily {
    aimer::FontRegistry::family(UI).unwrap_or(FontFamily::SANS_SERIF)
}

/// A text style for the window's own text, in Inter. Every piece of text that is not code starts
/// from this, since the framework has no way to change the font of all of it at once.
pub fn ui() -> TextStyle {
    TextStyle::new().font_family(ui_family())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aimer::FontRegistry;

    #[test]
    fn registers_both_variable_families_with_one_face_per_style() {
        register();
        // A second call must not register anything twice.
        register();
        for (name, family) in [(UI, ui_family()), (CODE, code_family())] {
            assert_eq!(
                FontRegistry::family(name),
                Some(family),
                "{name} is registered"
            );
            let regular = FontRegistry::resolve(family, FontWeight::Normal, FontStyle::Normal)
                .expect("the upright face is registered");
            let bold = FontRegistry::resolve(family, FontWeight::Bold, FontStyle::Normal)
                .expect("a bold request resolves");
            // One variable face serves every weight, so bold is the same face as regular.
            assert_eq!(
                regular.face_id, bold.face_id,
                "{name} bold shares the variable face"
            );
            let italic = FontRegistry::resolve(family, FontWeight::Normal, FontStyle::Italic)
                .expect("the italic face is registered");
            assert_ne!(
                regular.face_id, italic.face_id,
                "{name} italic is its own face"
            );
        }
    }
}
