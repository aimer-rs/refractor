//! How markdown looks in the editor, for `.md` files and for the doc comments of Rust: a
//! [`MarkdownTheme`] for the framework's live preview, which does the reading and renders what the
//! cursor is not on.

use std::rc::Rc;

use aimer::input::input::SpanStyle;
use aimer::style::{FontWeight, LayoutSpacing, TextDecoration, TextStyle};
use aimer::{
    AnyWidget, Color, Container, MarkdownLivePreview, MarkdownTheme, MarkdownViewer, Scrollable,
    Widget,
};

use crate::ui::doc_comments::DocLink;
use crate::ui::fonts;
use crate::services::settings::ThemeMode;

/// The size of the text the markdown is set in, which is the size of code.
pub const BODY_SIZE: u32 = 13;
/// Heading sizes for `#` to `######`: a step above the code, not the poster sizes of a note.
const HEADING_SIZES: [u32; 6] = [22, 19, 17, 15, 14, 14];
/// The corner radius in logical pixels of the background of inline code, which makes it read as a chip.
const INLINE_CODE_RADIUS: f32 = 4.0;

/// The colors of the markdown in one mode of the theme.
#[derive(Clone, Copy)]
struct Colors {
    text: Color,
    muted: Color,
    link: Color,
    accent: Color,
    /// The color of a rendered doc comment: a muted green-grey, which reads as prose set apart from the
    /// code and not as a comment.
    doc: Color,
    code_background: Color,
    keyword: Color,
    string: Color,
    number: Color,
    rule: Color,
}

impl Colors {
    const DARK: Self = Self {
        text: Color::Rgba(232, 230, 240, 255),
        muted: Color::Rgba(138, 136, 154, 255),
        link: Color::Rgba(84, 205, 236, 255),
        accent: Color::Rgb(141, 5, 87),
        doc: Color::Rgba(124, 158, 134, 255),
        code_background: Color::Rgba(255, 255, 255, 1),
        keyword: Color::Rgba(255, 123, 114, 255),
        string: Color::Rgba(165, 214, 255, 255),
        number: Color::Rgba(121, 192, 255, 255),
        rule: Color::Rgba(140, 142, 152, 120),
    };

    const LIGHT: Self = Self {
        text: Color::Rgba(36, 34, 48, 255),
        muted: Color::Rgba(110, 108, 125, 255),
        link: Color::Rgba(0, 100, 150, 255),
        accent: Color::Rgb(176, 20, 110),
        doc: Color::Rgba(64, 98, 76, 255),
        code_background: Color::Rgba(0, 0, 0, 1),
        keyword: Color::Rgba(207, 34, 46, 255),
        string: Color::Rgba(10, 48, 105, 255),
        number: Color::Rgba(5, 80, 174, 255),
        rule: Color::Rgba(90, 92, 102, 120),
    };

    const fn of(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self::DARK,
            ThemeMode::Light => Self::LIGHT,
        }
    }
}

/// The color of a rendered doc comment in `mode`.
pub const fn doc_color(mode: ThemeMode) -> Color {
    Colors::of(mode).doc
}

/// A live preview in the editor's look. Make one for a document and clone it: clones share the parse,
/// so moving the caret between lines walks the tree again and does not parse.
pub fn preview(mode: ThemeMode) -> MarkdownLivePreview {
    MarkdownLivePreview::new(&theme(mode))
}

/// The theme: only what differs from the editor's own text style, which stays the body.
fn theme(mode: ThemeMode) -> MarkdownTheme {
    let colors = Colors::of(mode);
    let heading = |size: u32| {
        TextStyle::new()
            .font_size(size)
            .font_weight(FontWeight::Bold)
    };
    let defaults = MarkdownTheme::default();
    MarkdownTheme {
        body: defaults
            .body
            .color(colors.text)
            .font_size(BODY_SIZE),
        headings: HEADING_SIZES.map(heading),
        code_block: defaults.code_block.color(colors.text),
        // A table is its lines and nothing else: the page shows through the cells.
        table_header_background: Color::Transparent,
        table_cell_background: Color::Transparent,
        keyword_color: colors.keyword,
        string_color: colors.string,
        number_color: colors.number,
        blockquote: defaults.blockquote.color(colors.muted),
        inline_code: defaults
            .inline_code
            .font_family(crate::ui::fonts::code_family())
            .background_color(colors.code_background)
            .background_radius(INLINE_CODE_RADIUS),
        link: SpanStyle::new()
            .color(colors.link)
            .text_decoration(TextDecoration::Underline),
        code_background: colors.code_background,
        icon_color: colors.accent,
        comment_color: colors.muted,
        rule_color: colors.rule,
        ..defaults
    }
}

/// The theme of a rendered doc comment: the UI font in the doc color, headings a step larger, code in the
/// code font.
pub fn doc_theme(mode: ThemeMode) -> MarkdownTheme {
    let colors = Colors::of(mode);
    let doc = colors.doc;
    let ui = |size: u32| {
        TextStyle::new()
            .font_family(fonts::ui_family())
            .font_size(size)
            .color(doc)
    };
    let heading = |size: u32| ui(size).font_weight(FontWeight::Bold);
    let defaults = MarkdownTheme::default();
    MarkdownTheme {
        body: ui(BODY_SIZE),
        headings: [17, 16, 15, 14, 14, 14].map(heading),
        blockquote: ui(BODY_SIZE),
        code_block: defaults
            .code_block
            .font_family(fonts::code_family())
            .font_size(12)
            .color(doc),
        // Inline code flows with the prose: the code font on a tinted background.
        // No color of its own: it is the color of the text it is in, and the color of a link when it is the name in one.
        inline_code: SpanStyle::new()
            .font_family(fonts::code_family())
            .background_color(doc.with_alpha(0.14))
            .background_radius(INLINE_CODE_RADIUS),
        link: SpanStyle::new()
            .color(colors.link)
            .text_decoration(TextDecoration::Underline),
        code_background: colors.code_background,
        icon_color: doc,
        comment_color: colors.muted,
        rule_color: doc.with_alpha(0.35),
        table_header_background: Color::Transparent,
        table_cell_background: Color::Transparent,
        keyword_color: colors.keyword,
        string_color: colors.string,
        number_color: colors.number,
        block_spacing: 6,
        ..defaults
    }
}

/// The theme of the card of documentation for a name: that of a rendered doc comment, but for the header of a code block, which says
/// the language and has a button that copies it, and has no place in a card that is mostly a signature.
pub fn hover_theme(mode: ThemeMode) -> MarkdownTheme {
    MarkdownTheme {
        code_header: false,
        ..doc_theme(mode)
    }
}

/// The markdown of a card of documentation, as the viewer draws it: scrolled when `scrollable`, which is when what is in the card
/// is more than it shows.
pub fn hover_viewer(markdown: &str, theme: &MarkdownTheme, scrollable: bool) -> AnyWidget {
    let viewer = MarkdownViewer::new()
        .markdown(markdown.to_owned())
        .theme(*theme)
        .scrollable(false)
        .padding(LayoutSpacing::all(0_u32));
    if scrollable {
        // Scrolled by an area of its own, as the list of files is, round the document that does not scroll.
        Scrollable::new()
            .child(Container::new().child(viewer))
            .boxed()
    } else {
        viewer.boxed()
    }
}

/// A rendered doc comment: the markdown, as the viewer draws it. The links whose target is [`DocLink::TARGET_PREFIX`] and the
/// number of one of `links` are what the comment calls ``[`Name`]``: `click` is called with the one that is clicked, and every other
/// link is the viewer's.
pub fn doc_viewer(
    markdown: &str,
    theme: &MarkdownTheme,
    links: Rc<[DocLink]>,
    click: Rc<dyn Fn(&DocLink)>,
) -> impl Widget + use<> {
    MarkdownViewer::new()
        .markdown(markdown.to_owned())
        .theme(*theme)
        .scrollable(false)
        .padding(LayoutSpacing::all(0_u32))
        .on_link_with_prefix(DocLink::TARGET_PREFIX, move |rest| {
            if let Some(link) = DocLink::index_of(rest).and_then(|index| links.get(index)) {
                click(link);
            }
        })
}

#[cfg(test)]
mod tests {
    use aimer::input::input::HighlightContext;

    use super::*;

    fn size_at(text: &str, cursor: usize, at: usize) -> Option<u32> {
        preview(ThemeMode::Dark)
            .ranges(text, &HighlightContext::new(text, cursor..cursor))
            .iter()
            .find(|range| range.range.contains(&at))
            .and_then(|range| range.style.font_size)
    }

    #[test]
    fn inline_code_has_rounded_corners_in_the_editor_theme_and_in_a_doc_comment() {
        assert_eq!(
            theme(ThemeMode::Dark)
                .inline_code
                .background_radius,
            Some(INLINE_CODE_RADIUS)
        );
        assert_eq!(
            doc_theme(ThemeMode::Dark)
                .inline_code
                .background_radius,
            Some(INLINE_CODE_RADIUS)
        );
        assert_eq!(
            hover_theme(ThemeMode::Light)
                .inline_code
                .background_radius,
            Some(INLINE_CODE_RADIUS)
        );
    }

    #[test]
    fn inline_code_in_a_doc_comment_takes_the_color_of_where_it_is_so_that_a_link_around_it_colors_it()
     {
        let theme = doc_theme(ThemeMode::Dark);
        assert_eq!(theme.inline_code.color, None);
        assert_eq!(theme.body.color, Colors::of(ThemeMode::Dark).doc);
    }

    #[test]
    fn headings_take_the_sizes_of_the_editor_theme() {
        let text = "# One\n### Three\nbody";
        let end = text.len();
        assert_eq!(
            size_at(text, end, text.find("One").unwrap()),
            Some(HEADING_SIZES[0])
        );
        assert_eq!(
            size_at(text, end, text.find("Three").unwrap()),
            Some(HEADING_SIZES[2])
        );
    }

    #[test]
    fn the_line_the_caret_is_on_shows_its_marks_and_the_others_conceal_them() {
        let text = "# One\nbody";
        // The caret on the body: the heading's `# ` is concealed.
        let concealed = preview(ThemeMode::Dark).ranges(text, &HighlightContext::new(text, 8..8));
        assert!(
            concealed
                .iter()
                .any(|range| range.range == (0..2)),
            "{concealed:?}"
        );
        // The caret on the heading: its source is shown.
        let shown = preview(ThemeMode::Dark).ranges(text, &HighlightContext::new(text, 2..2));
        assert!(
            !shown.iter().any(|range| range.range == (0..2)),
            "{shown:?}"
        );
    }
}
