use aimer::input::input::{LineBar, TextSnapshot};

use super::*;
use crate::model::syntax::SyntaxPalette;

const CODE: &str = "fn f() {\n    let a = 1;\n\n    if a > 0 {\n        g();\n\n        h();\n    }\n}\n\nfn k() {}\n";

fn levels_of(text: &str) -> Vec<usize> {
    let snapshot = TextSnapshot::from_text(text);
    (0..snapshot.line_count())
        .map(|line| IndentGuides::levels(&snapshot, line, 4))
        .collect()
}

#[test]
fn a_line_has_a_guide_for_each_whole_level_of_its_indent() {
    assert_eq!(
        levels_of("a\n    b\n        c\n          d\n  e"),
        [0, 1, 2, 2, 0],
        "two spaces are not a level"
    );
    assert_eq!(levels_of("a\n\tb\n\t\tc"), [0, 1, 2], "a tab is a level");
    assert_eq!(
        levels_of("a\n  \tb"),
        [0, 1],
        "blanks of both kinds: two spaces and a tab are two levels' worth, but a tab is one level of its own"
    );
    assert_eq!(
        IndentGuides::levels(&TextSnapshot::from_text("a\n      b"), 1, 2),
        3,
        "the width of a level is the setting's"
    );
}

#[test]
fn a_blank_line_has_the_guides_of_the_block_it_is_in() {
    // Lines: 0 fn f() {, 1 let, 2 blank, 3 if {, 4 g(), 5 blank, 6 h(), 7 }, 8 }, 9 blank, 10 fn k() {}, 11 the empty last line.
    let levels = levels_of(CODE);
    assert_eq!(levels, [0, 1, 1, 1, 2, 2, 2, 1, 0, 0, 0, 0], "{levels:?}");
}

#[test]
fn a_blank_line_after_an_opener_is_inside_it_and_one_before_a_closer_has_the_closers_level() {
    // Between `{` and its first line, and between the last line and `}`.
    assert_eq!(levels_of("fn f() {\n\n    a;\n\n}\n"), [0, 1, 1, 0, 0, 0]);
    // Blank lines in a row.
    assert_eq!(levels_of("fn f() {\n\n\n\n    a;\n}"), [0, 1, 1, 1, 1, 0]);
    // The start and the end of the text have nothing around them.
    assert_eq!(levels_of("\n\n    a;\n\n"), [0, 0, 1, 0, 0]);
}

#[test]
fn a_blank_line_made_of_blanks_is_blank_and_a_very_long_run_of_them_is_left_without_guides() {
    assert_eq!(
        levels_of("fn f() {\n        \n    a;\n}"),
        [0, 1, 1, 0],
        "blanks only"
    );
    let far = format!(
        "fn f() {{\n{}    a;\n}}",
        "\n".repeat(IndentGuides::LOOKAROUND * 3)
    );
    let snapshot = TextSnapshot::from_text(&far);
    assert_eq!(
        IndentGuides::levels(&snapshot, 1, 4),
        0,
        "too far from both ends to be looked for"
    );
    assert_eq!(
        IndentGuides::levels(&snapshot, IndentGuides::LOOKAROUND * 3 + 1, 4),
        1,
        "the line itself"
    );
}

#[test]
fn a_line_of_a_comment_that_ends_in_an_opener_does_not_make_the_blank_line_after_it_deeper() {
    assert_eq!(levels_of("// a map {\n\nfn f() {}\n"), [0, 0, 0, 0]);
}

#[test]
fn crlf_lines_and_the_empty_text_and_a_line_past_the_end_are_answered() {
    assert_eq!(
        levels_of("fn f() {\r\n\r\n    a;\r\n}\r\n"),
        [0, 1, 1, 0, 0]
    );
    assert_eq!(levels_of(""), [0]);
    let snapshot = TextSnapshot::from_text("a");
    assert_eq!(
        IndentGuides::levels(&snapshot, 9, 4),
        0,
        "a line the text does not have"
    );
}

#[test]
fn a_line_of_a_very_long_indent_is_answered_without_reading_all_of_it() {
    let long = format!("{}x", " ".repeat(10_000));
    let snapshot = TextSnapshot::from_text(&long);
    assert_eq!(
        IndentGuides::levels(&snapshot, 0, 4),
        IndentGuides::MAX_INDENT / 4
    );
}

fn pair_in(_text: &str, open: usize, close: usize) -> (Range<usize>, Range<usize>) {
    (open..open + 1, close..close + 1)
}

#[test]
fn the_block_of_a_pair_is_the_lines_between_its_brackets_at_the_level_of_the_line_that_opens_it() {
    let snapshot = TextSnapshot::from_text(CODE);
    // The braces of the function: line 0, and line 8.
    let (open, close) = pair_in(
        CODE,
        CODE.find('{').unwrap(),
        CODE.find("\n}\n").unwrap() + 1,
    );
    assert_eq!(
        IndentGuides::block(&snapshot, &open, &close, 4),
        Some(Block {
            level: 0,
            lines: 1..8
        })
    );
    // The braces of the `if`, which is at one level: its guide is the one in line with the `if`.
    let if_open = CODE.find("0 {").unwrap() + 2;
    let if_close = CODE.find("    }\n").unwrap() + 4;
    let (open, close) = pair_in(CODE, if_open, if_close);
    assert_eq!(
        IndentGuides::block(&snapshot, &open, &close, 4),
        Some(Block {
            level: 1,
            lines: 4..7
        })
    );
}

#[test]
fn a_pair_on_one_line_or_on_two_lines_in_a_row_holds_no_lines_and_has_no_block() {
    let snapshot = TextSnapshot::from_text("f(a, b);\ng(\n);\nh(\n    x);\n");
    let at = |needle: &str| {
        snapshot
            .slice(0..snapshot.len())
            .find(needle)
            .unwrap()
    };
    let one = pair_in("", at("("), at(")"));
    assert_eq!(
        IndentGuides::block(&snapshot, &one.0, &one.1, 4),
        None,
        "one line"
    );
    let two = pair_in("", at("g(") + 1, at(");\nh"));
    assert_eq!(
        IndentGuides::block(&snapshot, &two.0, &two.1, 4),
        None,
        "the closer is on the next line: no line between"
    );
}

fn style() -> GuideStyle {
    GuideStyle {
        palette: SyntaxPalette::dark(),
        unit: 4,
        code_size: 10.0,
    }
}

fn bars(text: &str, line: usize, block: Option<&Block>) -> Vec<LineBar> {
    let snapshot = TextSnapshot::from_text(text);
    let mut out = Vec::new();
    IndentGuides::bars(&snapshot, line, block, &style(), &mut out);
    out
}

#[test]
fn a_line_has_a_bar_for_each_level_in_the_color_of_the_level_and_at_the_indent_of_the_level() {
    let palette = SyntaxPalette::dark();
    let got = bars("a\n        b", 1, None);
    let dim = |level: usize| {
        LineBar::new(
            palette
                .rainbow(level)
                .with_alpha(IndentGuides::DIM),
            (level * 4) as u32 * 6,
        )
        .width(IndentGuides::DIM_WIDTH)
    };
    assert_eq!(
        got,
        [dim(0), dim(1)],
        "two levels, a bar each, at 0 and four columns of 6 px"
    );
    assert!(
        bars("a", 0, None).is_empty(),
        "a line that is not indented has none"
    );
}

#[test]
fn the_bar_of_the_block_the_caret_is_in_is_brighter_and_thicker_and_only_on_the_lines_of_it() {
    let palette = SyntaxPalette::dark();
    let block = Block {
        level: 1,
        lines: 1..3,
    };
    let text = "fn f() {\n    if x {\n        a;\n        b;\n    }\n}";
    let on = bars(text, 2, Some(&block));
    assert_eq!(on.len(), 2);
    assert_eq!(
        on[0],
        LineBar::new(palette.rainbow(0).with_alpha(IndentGuides::DIM), 0)
            .width(IndentGuides::DIM_WIDTH),
        "the level outside it stays dim"
    );
    assert_eq!(
        on[1],
        LineBar::new(
            palette
                .rainbow(1)
                .with_alpha(IndentGuides::BRIGHT),
            24
        )
        .width(IndentGuides::BRIGHT_WIDTH),
        "its own is bright"
    );
    let after = bars(text, 3, Some(&block));
    assert_eq!(
        after[1],
        LineBar::new(palette.rainbow(1).with_alpha(IndentGuides::DIM), 24)
            .width(IndentGuides::DIM_WIDTH),
        "a line that is not between its brackets has it dim"
    );
}
