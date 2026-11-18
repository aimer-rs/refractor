//! The colors of the syntax: which style a tree-sitter capture name gets.
//!
//! A highlights query names what it captures in dotted paths, from general to particular:
//! `keyword`, `keyword.control`, `function.method`, `type.builtin`. A palette gives a style to a
//! few names, and a capture takes the style of the longest prefix of its name that has one, so
//! `function.method.call` is a `function` unless the palette says otherwise.

use aimer::Color;
use aimer::input::input::SpanStyle;
use aimer::style::FontStyle;

/// The style of each kind of token.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SyntaxPalette {
    pub keyword: Color,
    pub function: Color,
    pub types: Color,
    pub string: Color,
    pub number: Color,
    pub comment: Color,
    pub constant: Color,
    pub property: Color,
    pub operator: Color,
    pub punctuation: Color,
    pub attribute: Color,
    pub variable: Color,
    /// The text of a doc comment that is rendered as markdown: lighter than a comment, which it is read
    /// as prose and not set aside.
    pub doc: Color,
}

impl SyntaxPalette {
    /// Colors for a dark background, in the family of the One Dark theme.
    pub const fn dark() -> Self {
        Self {
            keyword: Color::Rgb(198, 120, 221),
            function: Color::Rgb(97, 175, 239),
            types: Color::Rgb(229, 192, 123),
            string: Color::Rgb(152, 195, 121),
            number: Color::Rgb(209, 154, 102),
            comment: Color::Rgb(106, 115, 130),
            constant: Color::Rgb(209, 154, 102),
            property: Color::Rgb(224, 108, 117),
            operator: Color::Rgb(86, 182, 194),
            punctuation: Color::Rgb(171, 178, 191),
            attribute: Color::Rgb(229, 192, 123),
            variable: Color::Rgb(220, 223, 228),
            doc: Color::Rgb(192, 198, 210),
        }
    }

    /// The style of a capture called `name`, or `None` when it is to be left as the plain text is:
    /// internal captures (`_`-prefixed) and names this palette has nothing to say about.
    pub fn style_for(&self, name: &str) -> Option<SpanStyle> {
        if name.starts_with('_') {
            return None;
        }
        // The longest prefix of the dotted path that is known: `keyword.control.rust` is a `keyword`.
        let mut path = name;
        loop {
            if let Some(style) = self.exact(path) {
                return Some(style);
            }
            match path.rfind('.') {
                Some(dot) => path = &path[..dot],
                None => return None,
            }
        }
    }

    fn exact(&self, name: &str) -> Option<SpanStyle> {
        let color = |color: Color| Some(SpanStyle::new().color(color));
        match name {
            "keyword" | "include" | "conditional" | "repeat" | "storage" => color(self.keyword),
            "function" | "method" | "macro" => color(self.function),
            "constructor" | "type" | "namespace" | "module" => color(self.types),
            "string" | "character" | "escape" => color(self.string),
            "number" | "float" | "boolean" => color(self.number),
            "constant" => color(self.constant),
            "comment" => Some(
                SpanStyle::new()
                    .color(self.comment)
                    .font_style(FontStyle::Italic),
            ),
            "property" | "field" => color(self.property),
            "operator" => color(self.operator),
            "punctuation" | "delimiter" => color(self.punctuation),
            "attribute" | "label" | "lifetime" => color(self.attribute),
            "variable" | "parameter" => color(self.variable),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn color_of(name: &str) -> Option<Color> {
        SyntaxPalette::dark()
            .style_for(name)
            .and_then(|style| style.color)
    }

    #[test]
    fn a_capture_takes_the_style_of_the_longest_prefix_of_its_name_that_has_one() {
        let palette = SyntaxPalette::dark();
        assert_eq!(color_of("keyword"), Some(palette.keyword));
        assert_eq!(color_of("keyword.control.repeat"), Some(palette.keyword));
        assert_eq!(color_of("function.method.call"), Some(palette.function));
        assert_eq!(color_of("type.builtin"), Some(palette.types));
        assert_eq!(
            color_of("string.escape"),
            Some(palette.string),
            "an escape is a string unless said otherwise"
        );
    }

    #[test]
    fn internal_and_unknown_captures_are_left_plain() {
        assert_eq!(color_of("_internal"), None);
        assert_eq!(color_of("spell"), None);
        assert_eq!(color_of("nonsense.thing"), None);
    }

    #[test]
    fn comments_are_italic_and_the_rest_are_not() {
        let palette = SyntaxPalette::dark();
        assert_eq!(
            palette
                .style_for("comment.documentation")
                .and_then(|style| style.font_style),
            Some(FontStyle::Italic)
        );
        assert_eq!(
            palette
                .style_for("keyword")
                .and_then(|style| style.font_style),
            None
        );
    }
}
