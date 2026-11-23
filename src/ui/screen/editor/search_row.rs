use crate::ui::components::badge::{Badge, Glyph};
use crate::ui::components::icon::{icon, material};
use crate::ui::components::popup::Popup;
use crate::ui::file_icons::file_icon;
use crate::ui::fonts;
use crate::services::search::{Hit, Kind};
use crate::ui::theme::CodeTheme;
use aimer::decoration::Decoration;
use aimer::input::button::Button;
use aimer::input::mouse_region::MouseRegion;
use aimer::style::{BorderRadius, FontWeight, LayoutSpacing, TextOverflow, TextStyle, Theme};
use aimer::{
    AnyWidget, BoxAlignment, BuildContext, Center, Color, Container, Dimension, Expanded, Icons,
    RichText, Row, SizedBox, SpanStyle, StatelessWidget, Text, TextSpan, Widget,
};
use std::ops::Range;
use std::rc::Rc;

/// How tall a row is, in logical pixels, and how wide the mark of what it is.
pub const ROW_HEIGHT: u32 = 24;
/// The side of the room the mark of a hit has, which is as wide as the icon of a file.
const MARK: u32 = 16;
const UI_SIZE: u32 = 13;

/// One hit of the search: the mark of what it is, its name with the letters that were typed in bold, and where it is in the
/// color of what is not read first. The row that is chosen is marked in the pink of the app; the pointer on a row chooses it, and a
/// press goes to it.
#[derive(Clone, StatelessWidget)]
pub struct SearchRow {
    hit: Hit,
    index: usize,
    selected: bool,
    /// Chooses the row, for the pointer on it.
    choose: Rc<dyn Fn(usize)>,
    /// Goes to the row, for a press.
    go: Rc<dyn Fn(usize)>,
}

impl SearchRow {
    pub fn new(
        hit: Hit,
        index: usize,
        selected: bool,
        choose: Rc<dyn Fn(usize)>,
        go: Rc<dyn Fn(usize)>,
    ) -> Self {
        Self {
            hit,
            index,
            selected,
            choose,
            go,
        }
    }
}

/// Splits `title` into the runs that are marked and the ones that are not, in order and with no gaps. A mark that is not on
/// the characters of the title, which a title that changed under it would give, is left out.
fn runs(title: &str, marked: &[Range<usize>]) -> Vec<(Range<usize>, bool)> {
    let mut runs = Vec::new();
    let mut at = 0;
    for mark in marked {
        if mark.start < at || mark.end > title.len() || mark.start >= mark.end {
            continue;
        }
        if !title.is_char_boundary(mark.start) || !title.is_char_boundary(mark.end) {
            continue;
        }
        if mark.start > at {
            runs.push((at..mark.start, false));
        }
        runs.push((mark.clone(), true));
        at = mark.end;
    }
    if at < title.len() {
        runs.push((at..title.len(), false));
    }
    runs
}

fn mark(hit: &Hit, theme: &CodeTheme) -> AnyWidget {
    match &hit.kind {
        Kind::File => material(file_icon(&hit.title), 16).boxed(),
        Kind::Text => icon(Icons::TEXT_SEARCH, 15, theme.muted).boxed(),
        Kind::Action => icon(Icons::ZAP, 15, theme.muted).boxed(),
        Kind::Symbol(kind) => Badge::new(Glyph::of(kind.item_kind())).boxed(),
    }
}

/// The widgets of the row: the mark, the name and, if there is one, where it is.
fn line(hit: &Hit, theme: &CodeTheme) -> Vec<AnyWidget> {
    let text = |color: Color| -> TextStyle {
        fonts::ui()
            .font_size(UI_SIZE)
            .color(color)
            .text_overflow(TextOverflow::Clip)
    };
    // A text hit is a line of code, which is read in the font of code.
    let title_style = if hit.kind == Kind::Text {
        TextStyle::new()
            .font_family(fonts::code_family())
            .font_size(12)
            .color(theme.code)
            .text_overflow(TextOverflow::Clip)
    } else {
        text(theme.text)
    };
    let spans = runs(&hit.title, &hit.marked)
        .into_iter()
        .map(|(range, marked)| {
            let piece = TextSpan::new(&hit.title[range]);
            if marked {
                let mut style = SpanStyle::new().color(theme.marker);
                style.font_weight = Some(FontWeight::Bold);
                piece.style(style)
            } else {
                piece
            }
        });
    let title = RichText::new(TextSpan::new("").children(spans)).text_style(title_style);
    // The name takes what the mark and the place leave; the place is as wide as it is.
    let mut line: Vec<AnyWidget> = vec![
        // The same room whatever the mark is: a badge is a little smaller than the icon of a file, and the names would not line up.
        SizedBox::new()
            .width(MARK)
            .height(MARK)
            .box_child(Center::new().box_child(mark(hit, theme)))
            .boxed(),
        Expanded::new().child(title).boxed(),
    ];
    if !hit.detail.is_empty() {
        line.push(
            Text::new(hit.detail.clone())
                .text_style(text(theme.muted).font_size(11))
                .boxed(),
        );
    }
    line
}

impl StatelessWidget for SearchRow {
    fn build(&self, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let (index, selected, hit) = (self.index, self.selected, self.hit.clone());
        let background = if selected {
            theme.marker.with_alpha(Popup::ROW_HIGHLIGHT)
        } else {
            Color::Transparent
        };
        let (choose, go) = (Rc::clone(&self.choose), Rc::clone(&self.go));
        MouseRegion::new()
            .on_hover_enter(move || choose(index))
            .box_child(
                Button::new()
                    .no_decor()
                    .on_press(move || go(index))
                    .build(move |_state| {
                        SizedBox::new()
                            .width(Dimension::Percent(100.0))
                            .height(ROW_HEIGHT)
                            .box_child(
                                Container::new()
                                    .width(Dimension::Percent(100.0))
                                    .padding(LayoutSpacing::new().left(8).right(8))
                                    .decoration([
                                        BorderRadius::all(4).notype(),
                                        background.notype(),
                                    ])
                                    .child(
                                        Row::new()
                                            .vertical_alignment(BoxAlignment::Center)
                                            .gaps(LayoutSpacing::all(8))
                                            .children(line(&hit, &theme)),
                                    ),
                            )
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_is_split_into_what_is_marked_and_what_is_not_with_no_gaps() {
        assert_eq!(
            runs("RequestBuilder", &[0..3, 7..8]),
            [(0..3, true), (3..7, false), (7..8, true), (8..14, false)]
        );
        assert_eq!(runs("abc", &[]), [(0..3, false)]);
        assert_eq!(runs("", &[]), []);
        assert_eq!(runs("abc", &[0..3]), [(0..3, true)]);
    }

    #[test]
    fn a_mark_that_does_not_fit_the_title_is_left_out_and_so_is_one_inside_a_character() {
        assert_eq!(runs("abc", &[2..9]), [(0..3, false)], "past the end");
        assert_eq!(runs("abc", &[2..2]), [(0..3, false)], "nothing marked");
        assert_eq!(
            runs("été", &[1..2]),
            [(0..5, false)],
            "inside the first accent"
        );
        assert_eq!(
            runs("été", &[0..2, 2..3]),
            [(0..2, true), (2..3, true), (3..5, false)]
        );
        assert_eq!(
            runs("abcd", &[2..3, 0..1]),
            [(0..2, false), (2..3, true), (3..4, false)],
            "out of order"
        );
    }
}
