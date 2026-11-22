//! The small badge that tells what kind a thing of the code is: a glyph in the color of the kind, on a fainter fill of it, in an edge
//! of it. The menu of completions and the search draw the same, so that a function is the same green mark in both.

use crate::fonts;
use crate::lsp::ItemKind;
use aimer::style::{BorderSlice, BorderStyle, BoxBorder, BoxDecoration, TextAlign, TextStyle};
use aimer::{BuildContext, Center, Color, Container, StatelessWidget, Text, Widget};

/// The colors the badges of the kinds of items are in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hue {
    Pink,
    Green,
    Purple,
    Orange,
    Blue,
    Amber,
    Teal,
    Gray,
}

impl Hue {
    /// The color of the glyph and, fainter, of the edge and the fill of the badge.
    pub fn color(self) -> Color {
        match self {
            Self::Pink => Color::Rgb(240, 100, 178),
            Self::Green => Color::Rgb(104, 200, 150),
            Self::Purple => Color::Rgb(150, 124, 244),
            Self::Orange => Color::Rgb(250, 128, 92),
            Self::Blue => Color::Rgb(112, 150, 246),
            Self::Amber => Color::Rgb(238, 168, 72),
            Self::Teal => Color::Rgb(88, 196, 210),
            Self::Gray => Color::Rgb(156, 152, 176),
        }
    }
}

/// What a badge says of an item: the glyph in it, its color, and the kind in words for a row that has no detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glyph {
    pub text: &'static str,
    pub hue: Hue,
    pub name: &'static str,
}

impl Glyph {
    /// The glyph of an item of `kind`, which is what the menu of completions and the search both give it.
    pub fn of(kind: ItemKind) -> Self {
        let glyph = Self::new;
        match kind {
            ItemKind::Function => glyph("fn", Hue::Green, "function"),
            ItemKind::Method => glyph("fn", Hue::Green, "method"),
            ItemKind::Constructor => glyph("fn", Hue::Green, "constructor"),
            ItemKind::Field | ItemKind::Property => glyph("p", Hue::Teal, "field"),
            ItemKind::Variable | ItemKind::Value => glyph("v", Hue::Teal, "variable"),
            ItemKind::Class | ItemKind::Struct => glyph("S", Hue::Pink, "struct"),
            ItemKind::Interface => glyph("T", Hue::Purple, "trait"),
            ItemKind::Module | ItemKind::Folder | ItemKind::File => glyph("M", Hue::Blue, "module"),
            ItemKind::Enum => glyph("E", Hue::Amber, "enum"),
            ItemKind::EnumMember => glyph("e", Hue::Amber, "variant"),
            ItemKind::Constant => glyph("C", Hue::Blue, "constant"),
            ItemKind::Keyword => glyph("k", Hue::Gray, "keyword"),
            ItemKind::Snippet => glyph("s", Hue::Gray, "snippet"),
            ItemKind::TypeParameter => glyph("T", Hue::Purple, "type parameter"),
            ItemKind::Unit
            | ItemKind::Event
            | ItemKind::Operator
            | ItemKind::Reference
            | ItemKind::Color
            | ItemKind::Text
            | ItemKind::Other => glyph("\u{b7}", Hue::Gray, ""),
        }
    }

    pub const fn new(text: &'static str, hue: Hue, name: &'static str) -> Self {
        Self { text, hue, name }
    }
}

/// The badge for a [`Glyph`].
#[derive(Clone, StatelessWidget)]
pub struct Badge {
    glyph: Glyph,
}

impl Badge {
    /// The side of the badge, in logical pixels.
    pub const SIZE: u32 = 14;

    pub fn new(glyph: Glyph) -> Self {
        Self { glyph }
    }
}

impl StatelessWidget for Badge {
    fn build(&self, _ctx: &BuildContext) -> impl Widget {
        let glyph = self.glyph;
        let color = glyph.hue.color();
        Container::new()
            .width(Self::SIZE)
            .height(Self::SIZE)
            .decoration(
                BoxDecoration::new()
                    .background_color(color.with_alpha(0.08))
                    .border_radius(2)
                    .border(BoxBorder::all(
                        BorderSlice::new()
                            .color(color.with_alpha(0.75))
                            .style(BorderStyle::Solid)
                            .stroke(1.0),
                    )),
            )
            .child(
                Center::new().box_child(
                    Text::new(glyph.text)
                        .text_align(TextAlign::MidCenter)
                        .text_style(
                            TextStyle::new()
                                .font_family(fonts::ui_family())
                                .font_size(if glyph.text.len() > 1 { 8 } else { 10 })
                                .font_weight(aimer::FontWeight::Bold)
                                .color(color),
                        ),
                ),
            )
    }
}
