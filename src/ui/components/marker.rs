use std::cell::RefCell;
use std::collections::HashMap;

use aimer::{Color, StaticSvg, SvgDocument};

thread_local! {
    /// The documents already made, by icon and color: each is parsed once.
    static TINTED: RefCell<HashMap<(usize, [u8; 4]), SvgDocument>> = RefCell::new(HashMap::new());
}

/// A stroked Lucide icon with its `currentColor` replaced by `color`, for the margin of the editor,
/// which draws an icon from its scene and cannot style it as the `Svg` widget can.
///
/// # Panics
///
/// If the embedded icon is not valid SVG, which is a fault in the build and not in any input.
pub fn tinted(icon: &'static StaticSvg, color: Color) -> SvgDocument {
    let (red, green, blue, alpha) = color.to_rgba();
    let key = (icon.source().as_ptr() as usize, [red, green, blue, alpha]);
    TINTED.with(|cache| {
        cache
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| {
                let source = icon
                    .source()
                    .replace("currentColor", &format!("#{red:02x}{green:02x}{blue:02x}"));
                SvgDocument::from_svg(source.as_bytes()).expect("a Lucide icon is valid SVG")
            })
            .clone()
    })
}

#[cfg(test)]
mod tests {
    use aimer::Icons;

    use super::*;

    #[test]
    fn an_icon_is_tinted_once_per_color_and_keeps_its_shape() {
        let color = Color::Rgb(84, 205, 236);
        let first = tinted(Icons::PEN_LINE, color);
        let again = tinted(Icons::PEN_LINE, color);
        assert!(
            std::sync::Arc::ptr_eq(first.scene(), again.scene()),
            "made once for a color"
        );
        let other = tinted(Icons::PEN_LINE, Color::Rgb(255, 0, 0));
        assert!(
            !std::sync::Arc::ptr_eq(first.scene(), other.scene()),
            "and again for another"
        );
        let book = tinted(Icons::BOOK_OPEN, color);
        assert_ne!(first.scene().nodes.len() + book.scene().nodes.len(), 0);
        assert_eq!(first.scene().viewport.width, 24.0, "the icon's own size");
    }
}
