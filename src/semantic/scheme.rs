//! The colors of the semantic tokens: which style a token gets from what the server calls it.
//!
//! A rule is a name of a kind of token and the modifiers that it has to carry, as a color scheme says them: `variable.mutable` is a
//! `variable` that is `mutable`. The rule of a token is the one of its kind whose modifiers it all has, and of those the one that
//! asks for the most. What no rule is for is left as the grammar colored it.

use crate::settings::ThemeMode;
use aimer::Color;
use aimer::FontWeight;
use aimer::input::input::SpanStyle;
use aimer::style::TextDecoration;

/// How a token that a rule is for is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub color: Color,
    pub bold: bool,
    pub underline: bool,
}

impl Look {
    const fn plain(red: u8, green: u8, blue: u8) -> Self {
        Self { color: Color::Rgb(red, green, blue), bold: false, underline: false }
    }

    const fn bold(red: u8, green: u8, blue: u8) -> Self {
        Self { color: Color::Rgb(red, green, blue), bold: true, underline: false }
    }

    const fn underlined(red: u8, green: u8, blue: u8) -> Self {
        Self { color: Color::Rgb(red, green, blue), bold: false, underline: true }
    }

    /// The style of the text.
    pub fn style(self) -> SpanStyle {
        let mut style = SpanStyle::new().color(self.color);
        if self.bold {
            style = style.font_weight(FontWeight::Bold);
        }
        if self.underline {
            style = style.text_decoration(TextDecoration::Underline);
        }
        style
    }
}

/// One rule: the kind of token, the modifiers it has to have, and how it is drawn.
#[derive(Clone, Copy, Debug)]
pub struct Rule {
    pub kind: &'static str,
    pub modifiers: &'static [&'static str],
    pub look: Look,
}

const fn rule(kind: &'static str, modifiers: &'static [&'static str], look: Look) -> Rule {
    Rule { kind, modifiers, look }
}

/// The rules of a scheme.
#[derive(Clone, Copy, Debug)]
pub struct Scheme {
    rules: &'static [Rule],
}

impl Scheme {
    /// The scheme of the dark page: the colors of the app's color scheme for Rust.
    const DARK: Self = Self {
        rules: &[
            rule("method", &[], Look::plain(0x82, 0xAA, 0xFF)),
            rule("function", &[], Look::plain(0x82, 0xAA, 0xFF)),
            rule("keyword", &[], Look::plain(0xF8, 0x5F, 0xCD)),
            rule("keyword", &["controlFlow"], Look::plain(0x9F, 0x1A, 0xD8)),
            rule("struct", &[], Look::plain(0xFF, 0xCB, 0x6B)),
            rule("enum", &[], Look::plain(0x08, 0xB7, 0x20)),
            rule("interface", &[], Look::bold(0xFC, 0xA2, 0x48)),
            rule("variable", &[], Look::plain(0xE5, 0xFB, 0xCD)),
            rule("variable", &["readonly"], Look::plain(0xA9, 0xDC, 0x76)),
            rule("variable", &["mutable"], Look::underlined(0xFF, 0x6B, 0x81)),
            rule("parameter", &[], Look::plain(0xA9, 0xDC, 0x76)),
            rule("parameter", &["readonly"], Look::plain(0xA9, 0xDC, 0x76)),
            rule("parameter", &["mutable"], Look::underlined(0xFF, 0x6B, 0x81)),
            rule("const", &[], Look::bold(0xFF, 0x9C, 0xAC)),
            rule("constParameter", &[], Look::plain(0xFF, 0x9C, 0xAC)),
            rule("variable", &["constant"], Look::bold(0xFF, 0x9C, 0xAC)),
            rule("static", &[], Look::plain(0x7D, 0xCF, 0xFF)),
            rule("static", &["mutable"], Look::underlined(0x7D, 0xCF, 0xFF)),
            rule("variable", &["static"], Look::plain(0x7D, 0xCF, 0xFF)),
            rule("variable", &["static", "mutable"], Look::underlined(0x7D, 0xCF, 0xFF)),
        ],
    };

    /// The same rules on a white page, with the colors darkened until they read.
    const LIGHT: Self = Self {
        rules: &[
            rule("method", &[], Look::plain(36, 92, 214)),
            rule("function", &[], Look::plain(36, 92, 214)),
            rule("keyword", &[], Look::plain(190, 40, 150)),
            rule("keyword", &["controlFlow"], Look::plain(120, 20, 170)),
            rule("struct", &[], Look::plain(160, 100, 0)),
            rule("enum", &[], Look::plain(10, 130, 30)),
            rule("interface", &[], Look::bold(190, 100, 10)),
            rule("variable", &[], Look::plain(56, 58, 66)),
            rule("variable", &["readonly"], Look::plain(60, 130, 30)),
            rule("variable", &["mutable"], Look::underlined(210, 50, 80)),
            rule("parameter", &[], Look::plain(60, 130, 30)),
            rule("parameter", &["readonly"], Look::plain(60, 130, 30)),
            rule("parameter", &["mutable"], Look::underlined(210, 50, 80)),
            rule("const", &[], Look::bold(190, 60, 90)),
            rule("constParameter", &[], Look::plain(190, 60, 90)),
            rule("variable", &["constant"], Look::bold(190, 60, 90)),
            rule("static", &[], Look::plain(0, 120, 170)),
            rule("static", &["mutable"], Look::underlined(0, 120, 170)),
            rule("variable", &["static"], Look::plain(0, 120, 170)),
            rule("variable", &["static", "mutable"], Look::underlined(0, 120, 170)),
        ],
    };

    /// The scheme for `mode`.
    pub const fn for_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self::DARK,
            ThemeMode::Light => Self::LIGHT,
        }
    }

    /// The rule of a token of the kind `kind` that has the modifiers `modifiers`: of the rules of its kind whose modifiers it has all,
    /// the one that asks for the most, and the later of two that ask for as many. None where no rule is for it.
    pub fn rule_of(&self, kind: &str, modifiers: &[&str]) -> Option<&Rule> {
        self.rules
            .iter()
            .filter(|rule| rule.kind == kind && rule.modifiers.iter().all(|wanted| modifiers.contains(wanted)))
            .max_by_key(|rule| rule.modifiers.len())
            // `max_by_key` keeps the last of those that are equal.
    }

    /// The style of a token of the kind `kind` that has the modifiers `modifiers`, or none where the grammar's color is left.
    pub fn style_for(&self, kind: &str, modifiers: &[&str]) -> Option<SpanStyle> {
        self.rule_of(kind, modifiers).map(|rule| rule.look.style())
    }
}

#[cfg(test)]
mod tests;
