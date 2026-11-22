//! The part of the card of a place that is about what is wrong there: what the server said, with the name of the kind of problem, and
//! what can be done about it, as buttons under it.

use crate::components::pad::pad;
use crate::fonts;
use crate::hover::{Card, Fix, HoverCell, Problem};
use crate::lsp::Weight;
use crate::theme::CodeTheme;
use aimer::console::info;
use aimer::decoration::Decoration;
use aimer::input::button::Button;
use aimer::style::{
    BorderRadius, BorderSlice, BorderStyle, BoxBorder, LayoutSpacing, TextDecoration,
    TextDecorationLine, TextDecorationStyle, TextOverflow, TextStyle,
};
use aimer::{
    AnyWidget, BoxAlignment, Color, Column, Container, Dimension, Expanded, JustifyContent,
    NumberExt, RichText, Row, SizedBox, SpanStyle, Text, TextSpan, Widget, ZeroSizedBox, list,
};
use std::rc::Rc;

/// What is pressed to do the first fix, and to see the others, as an editor writes them.
pub const KEY_FIX: &str = "\u{2325}\u{21e7}\u{21a9}";
pub const KEY_MORE: &str = "\u{2325}\u{21a9}";

/// How big the text of the card is: a little smaller than the code it is about, as the rest of the chrome of the window is.
const TEXT_SIZE: u32 = 12;
/// How big the mark of how wrong a problem is, how far it is from its text, and the padding of the card.
const MARK: u32 = 7;
const MARK_GAP: u32 = 8;
const PADDING: u32 = 10;
/// What a line of a message, a gap between problems, the strip of buttons and a row of a list of fixes are worth in height: the height of
/// the section is estimated from these, since it is not known until it is laid out and where the card goes has to be decided before.
const LINE: f32 = 17.0;
const GAP: f32 = 6.0;
const STRIP: f32 = 30.0;
const FIX_ROW: f32 = 24.0;
/// About how wide a character of the card is, and how wide the card is at the least and at the most: it is as wide as what it says.
const CHAR_WIDTH: f32 = 6.6;
pub const MIN_WIDTH: f32 = 240.0;
pub const MAX_WIDTH: f32 = 420.0;

/// The characters of the first line of a message, with its code and the brackets it is in: what a line of the card holds.
fn line_chars(problem: &Problem) -> usize {
    problem
        .message
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .count()
        + problem
            .code
            .as_ref()
            .map_or(0, |code| code.chars().count() + 3)
}

/// How wide the card is for `card`, which has problems: as wide as the longest of its messages, and no narrower or wider than it may be.
pub fn card_width(card: &Card) -> f32 {
    let widest = card
        .problems
        .iter()
        .map(line_chars)
        .max()
        .unwrap_or(0) as f32;
    (widest * CHAR_WIDTH + 2.0 * PADDING as f32 + (MARK + MARK_GAP) as f32)
        .clamp(MIN_WIDTH, MAX_WIDTH)
}

/// How many characters of a message fit in a line of a card as wide as `width`.
fn chars_per_line(width: f32) -> usize {
    (((width - 2.0 * PADDING as f32 - (MARK + MARK_GAP) as f32) / CHAR_WIDTH).floor() as usize)
        .max(1)
}

/// The color a problem of this weight is marked with: the color of the line under it.
pub fn color_of(weight: Weight, theme: CodeTheme) -> Color {
    match weight {
        Weight::Error => theme.error_line,
        Weight::Warning => theme.warning_line,
        Weight::Weak => theme.weak_line,
    }
}

/// About how tall the section is for `card`: its padding, the lines of each message, and the strip of buttons, or the list of fixes where
/// it is open.
pub fn estimated_height(card: &Card, expanded: bool) -> f32 {
    if card.problems.is_empty() {
        return 0.0;
    }
    let per_line = chars_per_line(card_width(card));
    let messages: f32 = card
        .problems
        .iter()
        .map(|problem| LINE * (1 + line_chars(problem).saturating_sub(1) / per_line) as f32)
        .sum();
    let gaps = GAP * (card.problems.len().saturating_sub(1)) as f32;
    let actions = match card.fixes.len() {
        0 => 0.0,
        _ if expanded => STRIP + FIX_ROW * card.fixes.len().saturating_sub(1) as f32,
        _ => STRIP,
    };
    2.0 * PADDING as f32 + messages + gaps + actions
}

/// One problem: the mark of its weight, and what the server said with the name of the kind of problem after it.
fn problem_row(problem: &Problem, theme: CodeTheme) -> AnyWidget {
    // The first line is the message; the lines after it are what the server adds, which a card has not the room for.
    info!("Problem: {problem:?}");
    let message = problem
        .message
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_owned();
    let mut spans = vec![TextSpan::new(message.as_str())];
    if let Some(code) = &problem.code {
        spans.push(TextSpan::new(" ["));
        spans.push(TextSpan::new(code.as_str()).style(SpanStyle::new().color(theme.link_hover)));
        spans.push(TextSpan::new("]"));
    }
    RichText::new(TextSpan::new("").children(spans))
        .text_style(
            fonts::ui()
                .font_size(TEXT_SIZE)
                .color(theme.text)
                .text_overflow(TextOverflow::Wrap),
        )
        .boxed()
}

/// A button of the strip that is a word or two and, after it, the keys that do it.
fn action(
    label: String,
    keys: Option<&'static str>,
    theme: CodeTheme,
    press: impl Fn() + 'static,
) -> AnyWidget {
    Button::new()
        .no_decor()
        .on_press(press)
        .build(move |state| {
            let (text_color, decor) = if state.is_hovered {
                (
                    theme.text,
                    TextDecoration::new().line(TextDecorationLine::UNDERLINE),
                )
            } else {
                (theme.marker, TextDecoration::None)
            };
            Container::new()
                .padding(pad(3, 0, 3, GAP as u32))
                .child(
                    Text::new(label.to_string()).text_style(
                        fonts::ui()
                            .font_size(TEXT_SIZE - 1)
                            .color(text_color)
                            .text_decoration(decor)
                            .text_overflow(TextOverflow::Clip),
                    ),
                )
        })
        .boxed()
}

/// The buttons under the problems: the first fix, and a way to see the others.
fn strip(card: &Card, cell: &Rc<HoverCell>, theme: CodeTheme) -> AnyWidget {
    let expanded = cell.is_expanded();
    let run = |fix: &Fix, cell: &Rc<HoverCell>| {
        let (fix, cell) = (fix.clone(), Rc::clone(cell));
        move || {
            // The card goes first: what the fix does changes the text, and a card of a place that has been changed is not true.
            cell.hide();
            fix.run();
        }
    };
    let spacing = |width| SizedBox::new().width(width);
    let first = &card.fixes[0];
    let mut buttons: Vec<AnyWidget> = list![action(
        first.title.to_string(),
        Some(KEY_FIX),
        theme,
        run(first, cell),
    )];
    if card.fixes.len() > 1 {
        let toggle = Rc::clone(cell);
        let label = if expanded {
            "Fewer actions\u{2026}" // \u{2026}  elipsis
        } else {
            "More actions\u{2026}"
        };
        buttons.push(Expanded::new().box_child(ZeroSizedBox));
        buttons.push(action(label.to_owned(), Some(KEY_MORE), theme, move || {
            toggle.toggle_expanded()
        }));
        buttons.push(spacing(GAP * 2.0).boxed())
    }
    let row = Row::new()
        .vertical_alignment(BoxAlignment::Center)
        .children(buttons);
    let mut column: Vec<AnyWidget> = vec![row.boxed()];
    if expanded {
        for fix in card.fixes.iter().skip(1) {
            column.push(action(fix.title.clone(), None, theme, run(fix, cell)));
        }
    }
    Container::new()
        .width(Dimension::Percent(100.0))
        .padding(pad(3, PADDING - 6, 3, PADDING - 6))
        .decoration([
            BorderRadius::new()
                .bottom_left(7)
                .bottom_right(7)
                .notype(),
            // The color of the side panel: the pink-black of the theme, a step from the card as the strip of a card is.
            theme.sidebar.notype(),
        ])
        .child(
            Column::new()
                .gaps(LayoutSpacing::all(2))
                .children(column),
        )
        .boxed()
}

/// The section for `card`, which has problems: each one, and under them the buttons for what can be done, if the server has said.
pub fn problem_section(card: &Card, cell: &Rc<HoverCell>, theme: CodeTheme) -> AnyWidget {
    let rows: Vec<AnyWidget> = card
        .problems
        .iter()
        .map(|problem| problem_row(problem, theme))
        .collect();
    let mut parts: Vec<AnyWidget> = vec![
        Container::new()
            .padding(LayoutSpacing::all(PADDING))
            .child(
                Column::new()
                    .gaps(LayoutSpacing::all(GAP as u32))
                    .children(rows),
            )
            .boxed(),
    ];
    if !card.fixes.is_empty() {
        parts.push(strip(card, cell, theme));
    }
    SizedBox::new()
        .width(Dimension::Percent(100.0))
        .box_child(Column::new().children(parts))
        .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hover::Fix;
    use aimer::input::input::HoverSpot;

    fn card(problems: Vec<Problem>, fixes: Vec<Fix>) -> Card {
        let spot = HoverSpot {
            range: 0..1,
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
            extent: None,
        };
        Card {
            spot,
            markdown: String::new(),
            problems,
            fixes,
        }
    }

    fn problem(message: &str, code: Option<&str>) -> Problem {
        Problem {
            weight: Weight::Error,
            message: message.to_owned(),
            code: code.map(str::to_owned),
        }
    }

    fn fix() -> Fix {
        Fix::new("Import", true, || true)
    }

    #[test]
    fn the_card_is_as_wide_as_what_it_says_and_no_wider_than_the_most_it_may_be_nor_narrower_than_the_least()
     {
        let width = |message: &str, code: Option<&str>| {
            card_width(&card(vec![problem(message, code)], Vec::new()))
        };
        assert_eq!(
            width("x", None),
            MIN_WIDTH,
            "a short message does not make a card of a few characters"
        );
        assert_eq!(
            width(&"x".repeat(500), None),
            MAX_WIDTH,
            "and a long one wraps in the most"
        );
        let mid = width(&"x".repeat(30), Some("E0412"));
        assert!(mid > MIN_WIDTH && mid < MAX_WIDTH, "{mid}");
        assert!(
            width(&"x".repeat(30), Some("E0412")) > width(&"x".repeat(30), None),
            "the code is part of the line"
        );
        // The widest of the problems is what it is as wide as, and only the first line of a message counts.
        let two = card_width(&card(
            vec![problem("short", None), problem(&"y".repeat(40), None)],
            Vec::new(),
        ));
        assert_eq!(two, width(&"y".repeat(40), None));
        assert_eq!(
            width(
                "short\nan awfully long second line that is not in the card at all, not even a little",
                None
            ),
            MIN_WIDTH
        );
        assert!(
            MAX_WIDTH <= 440.0,
            "smaller than the card of documentation, which has a page to read"
        );
    }

    #[test]
    fn a_message_wraps_in_the_width_the_card_has_and_the_height_follows() {
        let long = "x".repeat(200);
        let wrapped = estimated_height(&card(vec![problem(&long, None)], Vec::new()), false);
        let one_line = estimated_height(&card(vec![problem("x", None)], Vec::new()), false);
        assert!(wrapped > one_line + 2.0 * LINE, "{wrapped} and {one_line}");
    }

    #[test]
    fn a_card_with_no_problem_has_no_section() {
        assert_eq!(estimated_height(&card(Vec::new(), vec![fix()]), false), 0.0);
    }

    #[test]
    fn the_section_is_as_tall_as_its_messages_and_the_strip_when_there_are_fixes() {
        let base = 2.0 * PADDING as f32;
        assert_eq!(
            estimated_height(&card(vec![problem("short", None)], Vec::new()), false),
            base + LINE
        );
        assert_eq!(
            estimated_height(&card(vec![problem("short", None)], vec![fix()]), false),
            base + LINE + STRIP
        );
        assert_eq!(
            estimated_height(
                &card(vec![problem("one", None), problem("two", None)], Vec::new()),
                false
            ),
            base + 2.0 * LINE + GAP,
            "a gap between two"
        );
    }

    #[test]
    fn a_code_is_part_of_the_line_and_only_the_first_line_of_a_message_is_in_the_card() {
        let base = 2.0 * PADDING as f32;
        // A message that fills a line of the widest card, and the code after it that does not fit in the line.
        let full = "y".repeat(chars_per_line(MAX_WIDTH));
        assert_eq!(
            estimated_height(&card(vec![problem(&full, None)], Vec::new()), false),
            base + LINE
        );
        assert_eq!(
            estimated_height(
                &card(vec![problem(&full, Some("E0412"))], Vec::new()),
                false
            ),
            base + 2.0 * LINE
        );
        assert_eq!(
            estimated_height(
                &card(vec![problem("first\nsecond\nthird", None)], Vec::new()),
                false
            ),
            base + LINE
        );
    }

    #[test]
    fn the_list_of_the_other_fixes_makes_the_section_taller_a_row_for_each() {
        let base = 2.0 * PADDING as f32 + LINE;
        let three = card(vec![problem("m", None)], vec![fix(), fix(), fix()]);
        assert_eq!(estimated_height(&three, false), base + STRIP);
        assert_eq!(estimated_height(&three, true), base + STRIP + 2.0 * FIX_ROW);
        let one = card(vec![problem("m", None)], vec![fix()]);
        assert_eq!(
            estimated_height(&one, true),
            base + STRIP,
            "there is no other to list"
        );
    }

    #[test]
    fn each_weight_has_its_own_color_in_the_color_of_its_line() {
        let theme = CodeTheme::dark();
        assert_eq!(color_of(Weight::Error, theme), theme.error_line);
        assert_eq!(color_of(Weight::Warning, theme), theme.warning_line);
        assert_eq!(color_of(Weight::Weak, theme), theme.weak_line);
    }
}
