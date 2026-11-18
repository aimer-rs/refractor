//! How markdown looks in the editor, for `.md` files and for the doc comments of Rust: a
//! [`MarkdownTheme`] for the framework's live preview, which does the reading and renders what the
//! cursor is not on.

use aimer::input::input::SpanStyle;
use aimer::style::{FontWeight, LayoutSpacing, TextDecoration, TextStyle};
use aimer::{Color, MarkdownLivePreview, MarkdownTheme, MarkdownViewer, Widget};

use crate::fonts;

/// The size of the text the markdown is set in, which is the size of code.
pub const BODY_SIZE: u32 = 13;
/// Heading sizes for `#` to `######`: a step above the code, not the poster sizes of a note.
const HEADING_SIZES: [u32; 6] = [22, 19, 17, 15, 14, 14];

const TEXT: Color = Color::Rgba(232, 230, 240, 255);
const MUTED: Color = Color::Rgba(138, 136, 154, 255);
const LINK: Color = Color::Rgba(84, 205, 236, 255);
const ACCENT: Color = Color::Rgba(255, 92, 150, 255);
/// The color of a rendered doc comment: a muted green-grey, which reads as prose set apart from the code
/// and not as a comment.
pub const DOC: Color = Color::Rgba(124, 158, 134, 255);
const CODE_BACKGROUND: Color = Color::Rgba(255, 255, 255, 22);

/// A live preview in the editor's look. Make one for a document and clone it: clones share the parse,
/// so moving the caret between lines walks the tree again and does not parse.
pub fn preview() -> MarkdownLivePreview {
    MarkdownLivePreview::new(&theme())
}

/// The theme: only what differs from the editor's own text style, which stays the body.
fn theme() -> MarkdownTheme {
    let heading = |size: u32| TextStyle::new().font_size(size).font_weight(FontWeight::Bold);
    let defaults = MarkdownTheme::default();
    MarkdownTheme {
        body: defaults.body.color(TEXT).font_size(BODY_SIZE),
        headings: HEADING_SIZES.map(heading),
        code_block: defaults.code_block.color(TEXT),
        // A table is its lines and nothing else: the page shows through the cells.
        table_header_background: Color::Transparent,
        table_cell_background: Color::Transparent,
        keyword_color: Color::Rgba(255, 123, 114, 255),
        string_color: Color::Rgba(165, 214, 255, 255),
        number_color: Color::Rgba(121, 192, 255, 255),
        blockquote: defaults.blockquote.color(MUTED),
        inline_code: defaults
            .inline_code
            .font_family(crate::fonts::code_family())
            .background_color(CODE_BACKGROUND),
        link: SpanStyle::new().color(LINK).text_decoration(TextDecoration::Underline),
        code_background: CODE_BACKGROUND,
        icon_color: ACCENT,
        comment_color: MUTED,
        rule_color: Color::Rgba(140, 142, 152, 120),
        ..defaults
    }
}

/// The theme of a rendered doc comment: the UI font in the doc color, headings a step larger, code in the
/// code font.
pub fn doc_theme() -> MarkdownTheme {
    let ui = |size: u32| TextStyle::new().font_family(fonts::ui_family()).font_size(size).color(DOC);
    let heading = |size: u32| ui(size).font_weight(FontWeight::Bold);
    let defaults = MarkdownTheme::default();
    MarkdownTheme {
        body: ui(BODY_SIZE),
        headings: [17, 16, 15, 14, 14, 14].map(heading),
        blockquote: ui(BODY_SIZE),
        code_block: defaults.code_block.font_family(fonts::code_family()).font_size(12).color(DOC),
        // Inline code flows with the prose: the code font on a tinted background.
        inline_code: SpanStyle::new().font_family(fonts::code_family()).color(DOC).background_color(DOC.with_alpha(0.14)),
        link: SpanStyle::new().color(LINK).text_decoration(TextDecoration::Underline),
        code_background: CODE_BACKGROUND,
        icon_color: DOC,
        comment_color: MUTED,
        rule_color: DOC.with_alpha(0.35),
        table_header_background: Color::Transparent,
        table_cell_background: Color::Transparent,
        keyword_color: Color::Rgba(255, 123, 114, 255),
        string_color: Color::Rgba(165, 214, 255, 255),
        number_color: Color::Rgba(121, 192, 255, 255),
        block_spacing: 6,
        ..defaults
    }
}

/// A rendered doc comment: the markdown, as the viewer draws it.
pub fn doc_viewer(markdown: &str, theme: &MarkdownTheme) -> impl Widget + use<> {
    MarkdownViewer::new()
        .markdown(markdown.to_owned())
        .theme(*theme)
        .scrollable(false)
        .padding(LayoutSpacing::all(0_u32))
}

#[cfg(test)]
mod tests {
    use aimer::input::input::HighlightContext;

    use super::*;

    fn size_at(text: &str, cursor: usize, at: usize) -> Option<u32> {
        preview()
            .ranges(text, &HighlightContext::new(text, cursor..cursor))
            .iter()
            .find(|range| range.range.contains(&at))
            .and_then(|range| range.style.font_size)
    }

    #[test]
    fn headings_take_the_sizes_of_the_editor_theme() {
        let text = "# One\n### Three\nbody";
        let end = text.len();
        assert_eq!(size_at(text, end, text.find("One").unwrap()), Some(HEADING_SIZES[0]));
        assert_eq!(size_at(text, end, text.find("Three").unwrap()), Some(HEADING_SIZES[2]));
    }

    #[test]
    fn the_line_the_caret_is_on_shows_its_marks_and_the_others_conceal_them() {
        let text = "# One\nbody";
        // The caret on the body: the heading's `# ` is concealed.
        let concealed = preview().ranges(text, &HighlightContext::new(text, 8..8));
        assert!(concealed.iter().any(|range| range.range == (0..2)), "{concealed:?}");
        // The caret on the heading: its source is shown.
        let shown = preview().ranges(text, &HighlightContext::new(text, 2..2));
        assert!(!shown.iter().any(|range| range.range == (0..2)), "{shown:?}");
    }
}
