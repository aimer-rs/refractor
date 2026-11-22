//! The colors of the syntax: which style a tree-sitter capture name gets.
//!
//! A highlights query names what it captures in dotted paths, from general to particular:
//! `keyword`, `keyword.control`, `function.method`, `type.builtin`. A palette gives a style to a
//! few names, and a capture takes the style of the longest prefix of its name that has one, so
//! `function.method.call` is a `function` unless the palette says otherwise.

use crate::settings::ThemeMode;
use aimer::Color;
use aimer::FontWeight;
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
    /// A parameter of a function, which is told from the variables in its body.
    pub parameter: Color,
    /// The text of a doc comment that is rendered as markdown: lighter than a comment, which it is read
    /// as prose and not set aside.
    pub doc: Color,
    /// The colors of brackets, by how deep they are: each is a level, and after the last the first is used again.
    pub rainbow: [Color; Self::RAINBOW_LEVELS],
}

impl SyntaxPalette {
    /// How many colors the brackets go through before they begin again.
    pub const RAINBOW_LEVELS: usize = 6;

    /// The color of a bracket `depth` brackets deep.
    pub fn rainbow(&self, depth: usize) -> Color {
        self.rainbow[depth % Self::RAINBOW_LEVELS]
    }

    /// Colors for a dark background, in the family of the One Dark theme.
    pub const fn dark() -> Self {
        Self {
            keyword: Color::Rgb(248, 95, 205),
            function: Color::Rgb(130, 170, 255),
            types: Color::Rgb(255, 203, 107),
            string: Color::Rgb(152, 195, 121),
            number: Color::Rgb(209, 154, 102),
            comment: Color::Rgb(106, 115, 130),
            constant: Color::Rgb(255, 156, 172),
            property: Color::Rgb(224, 108, 117),
            operator: Color::Rgb(86, 182, 194),
            punctuation: Color::Rgb(171, 178, 191),
            attribute: Color::Rgb(229, 192, 123),
            variable: Color::Rgb(229, 251, 205),
            parameter: Color::Rgb(169, 220, 118),
            doc: Color::Rgb(192, 198, 210),
            // Bright enough to read on the dark page, and far enough apart that the next level is another color.
            rainbow: [
                Color::Rgb(238, 190, 96),
                Color::Rgb(206, 130, 230),
                Color::Rgb(98, 174, 247),
                Color::Rgb(120, 214, 150),
                Color::Rgb(255, 128, 150),
                Color::Rgb(86, 205, 214),
            ],
        }
    }

    /// Colors for a white background, in the family of the One Light theme.
    pub const fn light() -> Self {
        Self {
            keyword: Color::Rgb(166, 38, 164),
            function: Color::Rgb(64, 120, 242),
            types: Color::Rgb(176, 120, 0),
            string: Color::Rgb(60, 140, 60),
            number: Color::Rgb(152, 104, 1),
            comment: Color::Rgb(150, 152, 160),
            constant: Color::Rgb(152, 104, 1),
            property: Color::Rgb(214, 70, 60),
            operator: Color::Rgb(1, 132, 188),
            punctuation: Color::Rgb(70, 72, 82),
            attribute: Color::Rgb(176, 120, 0),
            variable: Color::Rgb(56, 58, 66),
            // The green of the scheme, darkened until it reads on a white page.
            parameter: Color::Rgb(70, 130, 30),
            doc: Color::Rgb(80, 82, 92),
            // The same hues, darkened until they read on a white page.
            rainbow: [
                Color::Rgb(176, 120, 0),
                Color::Rgb(150, 50, 170),
                Color::Rgb(30, 100, 220),
                Color::Rgb(30, 140, 70),
                Color::Rgb(214, 40, 110),
                Color::Rgb(0, 130, 150),
            ],
        }
    }

    /// The colors for `mode`.
    pub const fn for_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self::dark(),
            ThemeMode::Light => Self::light(),
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
            // `true`, `false` and `None` are not constants of the program: they are as numbers are.
            "constant.builtin" => color(self.number),
            "constant" => Some(
                SpanStyle::new()
                    .color(self.constant)
                    .font_weight(FontWeight::Bold),
            ),
            "comment" => Some(
                SpanStyle::new()
                    .color(self.comment)
                    .font_style(FontStyle::Italic),
            ),
            "property" | "field" => color(self.property),
            "operator" => color(self.operator),
            "punctuation" | "delimiter" => color(self.punctuation),
            "attribute" | "label" | "lifetime" => color(self.attribute),
            "variable.parameter" | "parameter" => color(self.parameter),
            "variable" => color(self.variable),
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
    fn the_light_palette_is_not_the_dark_one_and_styles_the_same_captures() {
        assert_eq!(
            SyntaxPalette::for_mode(ThemeMode::Dark),
            SyntaxPalette::dark()
        );
        assert_eq!(
            SyntaxPalette::for_mode(ThemeMode::Light),
            SyntaxPalette::light()
        );
        let (dark, light) = (SyntaxPalette::dark(), SyntaxPalette::light());
        assert_ne!(dark, light);
        for name in [
            "keyword",
            "function.method",
            "type",
            "string",
            "number",
            "comment",
            "property",
            "operator",
            "variable",
        ] {
            assert!(
                dark.style_for(name).is_some() && light.style_for(name).is_some(),
                "{name}"
            );
            assert_ne!(
                dark.style_for(name).and_then(|s| s.color),
                light.style_for(name).and_then(|s| s.color),
                "{name}"
            );
        }
    }

    #[test]
    fn a_bracket_is_a_color_by_its_depth_and_the_colors_come_round() {
        for palette in [SyntaxPalette::dark(), SyntaxPalette::light()] {
            let levels: Vec<Color> = (0..SyntaxPalette::RAINBOW_LEVELS)
                .map(|depth| palette.rainbow(depth))
                .collect();
            for (at, color) in levels.iter().enumerate() {
                assert!(
                    levels
                        .iter()
                        .skip(at + 1)
                        .all(|other| other != color),
                    "the levels are told apart: {levels:?}"
                );
            }
            assert_eq!(
                palette.rainbow(SyntaxPalette::RAINBOW_LEVELS),
                palette.rainbow(0),
                "after the last, the first"
            );
            assert_eq!(
                palette.rainbow(SyntaxPalette::RAINBOW_LEVELS * 7 + 2),
                palette.rainbow(2)
            );
        }
        assert_ne!(
            SyntaxPalette::dark().rainbow,
            SyntaxPalette::light().rainbow
        );
    }

    #[test]
    fn internal_and_unknown_captures_are_left_plain() {
        assert_eq!(color_of("_internal"), None);
        assert_eq!(color_of("spell"), None);
        assert_eq!(color_of("nonsense.thing"), None);
    }

    #[test]
    fn the_dark_palette_has_the_colors_of_the_scheme_for_the_tokens_a_highlights_query_names() {
        let palette = SyntaxPalette::dark();
        let hex = |red: u8, green: u8, blue: u8| Some(Color::Rgb(red, green, blue));
        // `method` and `function` #82AAFF, `keyword` #F85FCD, `struct` #FFCB6B, `variable` #E5FBCD, `parameter` #A9DC76.
        assert_eq!(color_of("function"), hex(0x82, 0xAA, 0xFF));
        assert_eq!(color_of("function.method"), hex(0x82, 0xAA, 0xFF));
        assert_eq!(color_of("function.macro"), hex(0x82, 0xAA, 0xFF));
        assert_eq!(color_of("keyword"), hex(0xF8, 0x5F, 0xCD));
        assert_eq!(color_of("type"), hex(0xFF, 0xCB, 0x6B));
        assert_eq!(color_of("type.builtin"), hex(0xFF, 0xCB, 0x6B));
        assert_eq!(color_of("variable"), hex(0xE5, 0xFB, 0xCD));
        assert_eq!(color_of("variable.parameter"), hex(0xA9, 0xDC, 0x76));
        assert_eq!(palette.parameter, Color::Rgb(0xA9, 0xDC, 0x76));
    }

    #[test]
    fn a_parameter_is_not_a_variable_and_both_are_left_as_they_were_in_the_light_palette_but_for_the_parameter()
     {
        for palette in [SyntaxPalette::dark(), SyntaxPalette::light()] {
            let of = |name: &str| {
                palette
                    .style_for(name)
                    .and_then(|style| style.color)
            };
            assert_eq!(of("variable.parameter"), Some(palette.parameter));
            assert_eq!(of("parameter"), Some(palette.parameter));
            assert_eq!(of("variable"), Some(palette.variable));
            assert_eq!(
                of("variable.builtin"),
                Some(palette.variable),
                "`self` is a variable"
            );
        }
        assert_ne!(
            SyntaxPalette::dark().parameter,
            SyntaxPalette::dark().variable
        );
        assert_ne!(
            SyntaxPalette::light().parameter,
            SyntaxPalette::light().variable
        );
    }

    #[test]
    fn a_constant_is_bold_in_its_color_and_a_builtin_one_such_as_true_is_not() {
        let palette = SyntaxPalette::dark();
        let constant = palette.style_for("constant").expect("styled");
        assert_eq!(constant.color, Some(Color::Rgb(0xFF, 0x9C, 0xAC)));
        assert_eq!(constant.font_weight, Some(FontWeight::Bold));
        let builtin = palette
            .style_for("constant.builtin")
            .expect("styled");
        assert_eq!(
            builtin.color,
            Some(palette.number),
            "`true` and `None` are not constants of the program"
        );
        assert_eq!(builtin.font_weight, None);
        assert_eq!(
            palette
                .style_for("number")
                .and_then(|style| style.font_weight),
            None
        );
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
