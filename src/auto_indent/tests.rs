use aimer::FocusNode;
use aimer::input::input::{TextEditingController, TextSnapshot};

use super::*;

#[test]
fn a_new_line_starts_as_deep_as_the_line_before_and_a_level_deeper_after_an_opener() {
    let after = |before: &str| AutoIndent::continuation(before, 4);
    assert_eq!(after(""), "");
    assert_eq!(after("foo();"), "");
    assert_eq!(after("    foo();"), "    ");
    assert_eq!(after("fn main() {"), "    ");
    assert_eq!(after("    if x {"), "        ");
    assert_eq!(after("let v = vec!["), "    ");
    assert_eq!(after("foo("), "    ");
    assert_eq!(
        after("    } else {"),
        "        ",
        "the line is as deep as its start, and opens a block"
    );
    assert_eq!(
        after("fn main() {   "),
        "    ",
        "a space after the opener does not hide it"
    );
    assert_eq!(after("}"), "");
    assert_eq!(
        after("    "),
        "    ",
        "a blank line that has an indent keeps it"
    );
}

#[test]
fn a_comment_that_ends_in_an_opener_does_not_open_a_block_and_tabs_are_indented_with_tabs() {
    let after = |before: &str| AutoIndent::continuation(before, 4);
    assert_eq!(after("    // a map {"), "    ");
    assert_eq!(
        after("\tfn f() {"),
        "\t\t",
        "a line indented with tabs is indented one more tab"
    );
    assert_eq!(after("  \tfn f() {"), "  \t\t");
    assert_eq!(
        AutoIndent::continuation("fn f() {", 2),
        "  ",
        "the width of a level is the setting's"
    );
}

fn splitting(text: &str, spaces: usize) -> Option<(usize, String, String)> {
    let at = text.find('|').expect("a | marks the caret");
    let text = text.replacen('|', "", 1);
    AutoIndent::split(&TextSnapshot::from_text(&text), at..at, spaces)
        .map(|split| (split.at, split.first, split.second))
}

#[test]
fn return_between_an_opener_and_its_closer_makes_an_indented_line_and_puts_the_closer_under_the_opener()
 {
    assert_eq!(
        splitting("fn f() {|}", 4),
        Some((8, "\n    ".to_owned(), "\n".to_owned()))
    );
    assert_eq!(
        splitting("    if x {|}", 4),
        Some((10, "\n        ".to_owned(), "\n    ".to_owned()))
    );
    assert_eq!(
        splitting("foo(|)", 4),
        Some((4, "\n    ".to_owned(), "\n".to_owned()))
    );
    assert_eq!(
        splitting("let v = [|];", 2),
        Some((9, "\n  ".to_owned(), "\n".to_owned()))
    );
    assert_eq!(
        splitting("fn f<|>() {}", 4),
        Some((5, "\n    ".to_owned(), "\n".to_owned())),
        "the angle brackets of generics"
    );
    assert_eq!(
        splitting("\tfn f() {|}", 4),
        Some((9, "\n\t\t".to_owned(), "\n\t".to_owned()))
    );
    assert_eq!(
        splitting("{|}", 4),
        Some((1, "\n    ".to_owned(), "\n".to_owned())),
        "the empty line before it, and the end of the text"
    );
}

#[test]
fn return_anywhere_else_is_the_fields_and_so_is_return_over_a_selection() {
    assert_eq!(
        splitting("{|x}", 4),
        None,
        "something between them is not an empty pair"
    );
    assert_eq!(splitting("{ |}", 4), None);
    assert_eq!(splitting("{x|}", 4), None);
    assert_eq!(splitting("(|]", 4), None, "not a pair");
    assert_eq!(splitting("x|", 4), None);
    assert_eq!(splitting("// {|}", 4), None, "a comment");
    assert_eq!(splitting("    /// see ({|})", 4), None, "a doc comment");
    let text = TextSnapshot::from_text("{ab}");
    assert_eq!(
        AutoIndent::split(&text, 1..3, 4).map(|split| split.at),
        None
    );
    let empty = TextSnapshot::from_text("");
    assert_eq!(
        AutoIndent::split(&empty, 0..0, 4).map(|split| split.at),
        None,
        "the empty text"
    );
}

fn controller_at(text: &str) -> TextEditingController {
    let caret = text.find('|').expect("a | marks the caret");
    let controller = TextEditingController::with_text(text.replacen('|', "", 1));
    controller.select_bytes(caret, caret);
    controller
}

fn selection(controller: &TextEditingController) -> std::ops::Range<usize> {
    controller.value().selection().range()
}

#[test]
fn the_split_is_one_edit_with_the_caret_on_the_new_line_and_one_undo_takes_it_back() {
    let controller = controller_at("fn f() {|}");
    assert!(AutoIndent::apply_return(&controller, 4));
    assert_eq!(controller.value().text(), "fn f() {\n    \n}");
    assert_eq!(selection(&controller), 13..13);
    assert!(controller.undo());
    assert_eq!(controller.value().text(), "fn f() {}");
}

#[test]
fn a_return_that_is_not_between_an_opener_and_its_closer_is_not_taken() {
    let controller = controller_at("fn f() {|x}");
    assert!(!AutoIndent::apply_return(&controller, 4));
    assert_eq!(controller.value().text(), "fn f() {x}");
}

fn blank(text: &str) -> Option<std::ops::Range<usize>> {
    let at = text.find('|').expect("a | marks the caret");
    let text = text.replacen('|', "", 1);
    AutoIndent::blank_line(&TextSnapshot::from_text(&text), at..at)
}

#[test]
fn backspace_in_a_line_of_only_blanks_takes_the_whole_line_with_its_break() {
    assert_eq!(
        blank("a\n    |\nb"),
        Some(1..6),
        "from the end of the line before to the end of the blank line"
    );
    assert_eq!(
        blank("a\n  |  \nb"),
        Some(1..6),
        "wherever the caret is in it"
    );
    assert_eq!(blank("a\n\t\t|\nb"), Some(1..4), "tabs are blanks");
    assert_eq!(
        blank("a\n  |"),
        Some(1..4),
        "the last line, which has no break after it"
    );
    assert_eq!(
        blank("a\r\n  |\r\nb"),
        Some(1..5),
        "a line break of two characters goes whole"
    );
    assert_eq!(
        blank("    |"),
        Some(0..4),
        "the first line has no break before it: its blanks go"
    );
    assert_eq!(
        blank("a\n\n  |\nb"),
        Some(2..5),
        "the line before is an empty line"
    );
}

#[test]
fn backspace_in_any_other_line_is_the_fields() {
    assert_eq!(
        blank("a\n|\nb"),
        None,
        "an empty line: Backspace takes its break as it is"
    );
    assert_eq!(blank("a\n  x|\nb"), None);
    assert_eq!(blank("a\n  |x\nb"), None, "text after the caret");
    assert_eq!(blank("|"), None, "the empty text");
    assert_eq!(
        blank("/// doc\n    |\nfn"),
        None,
        "under a doc comment, which is not joined to what is under it"
    );
    assert_eq!(blank("//! doc\n|  \nfn"), None);
    let text = TextSnapshot::from_text("a\n    \nb");
    assert_eq!(
        AutoIndent::blank_line(&text, 2..4),
        None,
        "a selection is deleted as it is"
    );
}

#[test]
fn the_blank_line_goes_as_one_edit_and_the_caret_is_at_the_end_of_the_line_before() {
    let controller = controller_at("a\n    |\nb");
    assert!(AutoIndent::apply_backspace(&controller));
    assert_eq!(controller.value().text(), "a\nb");
    assert_eq!(selection(&controller), 1..1);
    assert!(controller.undo());
    assert_eq!(controller.value().text(), "a\n    \nb");
    let controller = controller_at("a\nb|");
    assert!(!AutoIndent::apply_backspace(&controller));
    assert_eq!(controller.value().text(), "a\nb");
}

#[test]
fn a_blank_line_made_by_a_split_goes_with_one_backspace() {
    let controller = controller_at("if x {|}");
    assert!(AutoIndent::apply_return(&controller, 4));
    assert_eq!(controller.value().text(), "if x {\n    \n}");
    assert!(AutoIndent::apply_backspace(&controller));
    assert_eq!(
        controller.value().text(),
        "if x {\n}",
        "the indent and its line are gone, and the closer is where it was"
    );
}

/// Selects `anchor` to `focus` in `text`, presses Tab, or Shift+Tab where `outdent`, and gives the text and the selection that is left.
fn shifted(text: &str, anchor: usize, focus: usize, spaces: usize, outdent: bool) -> (String, usize, usize) {
    let controller = TextEditingController::with_text(text);
    controller.select_bytes(anchor, focus);
    AutoIndent::apply_shift(&controller, spaces, outdent);
    let value = controller.value();
    (value.text().to_string(), value.selection().anchor(), value.selection().focus())
}

#[test]
fn tab_over_a_selection_indents_each_line_it_touches_and_keeps_the_same_text_selected() {
    // From the middle of the first line to the middle of the third.
    let text = "aa\nbb\ncc\ndd";
    let (after, anchor, focus) = shifted(text, 1, 7, 4, false);
    assert_eq!(after, "    aa\n    bb\n    cc\ndd");
    assert_eq!((anchor, focus), (5, 19), "the ends move by what went in before them, so the same letters are selected");
    assert_eq!(&after[anchor..focus], "a\n    bb\n    c");
    // A selection inside one line indents that line.
    assert_eq!(shifted("fn f() {}", 3, 4, 4, false).0, "    fn f() {}");
    // The width of a level is the setting's.
    assert_eq!(shifted("a\nb", 0, 3, 2, false).0, "  a\n  b");
}

#[test]
fn a_selection_that_starts_at_the_start_of_a_line_takes_the_indent_in_and_one_that_ends_there_leaves_that_line_alone() {
    let (after, anchor, focus) = shifted("aa\nbb\ncc", 0, 6, 4, false);
    assert_eq!(after, "    aa\n    bb\ncc", "the line the selection ends at the start of is not touched");
    assert_eq!((anchor, focus), (0, 14), "and the start stays, so the indent is selected and the next Tab goes on from it");
    // Backwards: the same selection, with the caret at its start.
    let (_, anchor, focus) = shifted("aa\nbb\ncc", 6, 0, 4, false);
    assert_eq!((anchor, focus), (14, 0), "which end is the caret is kept");
    // Ending at the start of the last line of a text that has a break at its end: the empty line after it is not one.
    assert_eq!(shifted("aa\n", 0, 3, 4, false).0, "    aa\n");
}

#[test]
fn empty_lines_are_not_indented_and_blank_ones_are() {
    assert_eq!(shifted("a\n\nb", 0, 4, 4, false).0, "    a\n\n    b");
    assert_eq!(shifted("a\n  \nb", 0, 6, 4, false).0, "    a\n      \n    b");
}

#[test]
fn a_block_that_is_indented_with_tabs_is_indented_with_a_tab() {
    assert_eq!(shifted("\ta\n\t\tb", 0, 6, 4, false).0, "\t\ta\n\t\t\tb");
    assert_eq!(shifted("a\n\tb", 0, 4, 4, false).0, "\ta\n\t\tb", "one line that has a tab says what the block is indented with");
    assert_eq!(shifted("a\n  b", 0, 5, 4, false).0, "    a\n      b", "spaces are not a tab's");
}

#[test]
fn shift_tab_takes_a_level_from_each_line_and_what_is_less_than_a_level_goes_whole() {
    let text = "        a\n    b\n  c\nd\n\te";
    let (after, anchor, focus) = shifted(text, 0, text.len(), 4, true);
    assert_eq!(after, "    a\nb\nc\nd\ne", "a level of four, or what there is of it, and a tab is one");
    assert_eq!((anchor, focus), (0, after.len()), "the ends are kept at the ends");
    // Inside what is taken: the end is brought back to where the taking began, and is not before the line.
    let (after, anchor, focus) = shifted("      x\n      y", 2, 11, 4, true);
    assert_eq!(after, "  x\n  y");
    assert_eq!((anchor, focus), (0, 4), "a point inside what is taken goes to where it was taken from");
    // A line with only a part of a level of spaces before a tab loses the spaces.
    assert_eq!(shifted("  \tx", 0, 4, 4, true).0, "\tx");
}

#[test]
fn shift_tab_where_there_is_nothing_to_take_changes_nothing_and_is_still_the_selections() {
    let controller = TextEditingController::with_text("a\nb");
    controller.select_bytes(0, 3);
    assert!(AutoIndent::apply_shift(&controller, 4, true), "the key is taken, so it does not go on to move the focus");
    assert_eq!(controller.value().text(), "a\nb");
    assert!(!controller.undo(), "and it was not an edit");
}

#[test]
fn tab_and_shift_tab_are_the_fields_where_nothing_is_selected_or_there_is_no_text() {
    let controller = controller_at("a|b");
    assert!(!AutoIndent::apply_shift(&controller, 4, false));
    assert!(!AutoIndent::apply_shift(&controller, 4, true));
    assert_eq!(controller.value().text(), "ab");
    let empty = TextEditingController::with_text("");
    assert!(!AutoIndent::apply_shift(&empty, 4, false));
}

#[test]
fn the_indent_is_one_edit_that_one_undo_takes_back() {
    let controller = TextEditingController::with_text("a\nb\nc");
    controller.select_bytes(0, 5);
    assert!(AutoIndent::apply_shift(&controller, 4, false));
    assert_eq!(controller.value().text(), "    a\n    b\n    c");
    assert!(controller.undo());
    assert_eq!(controller.value().text(), "a\nb\nc");
}

#[test]
fn lines_ending_in_two_characters_and_text_of_many_bytes_are_indented_where_they_start() {
    let text = "\u{e9}a\r\n\u{e9}\u{301}b\r\nc";
    let (after, anchor, focus) = shifted(text, 2, text.len(), 4, false);
    assert_eq!(after, "    \u{e9}a\r\n    \u{e9}\u{301}b\r\n    c", "the break is not part of a line, and the letters are not cut");
    assert_eq!((anchor, focus), (6, after.len()), "the start is after the first accent, which is two bytes, and the end is the end");
    let (back, ..) = shifted(&after, 0, after.len(), 4, true);
    assert_eq!(back, text, "and Shift+Tab undoes it exactly");
}

#[test]
fn a_text_with_no_break_at_the_end_and_a_very_long_line_are_indented() {
    assert_eq!(shifted("a", 0, 1, 4, false).0, "    a");
    let long = "x".repeat(100_000);
    let (after, _, focus) = shifted(&long, 0, long.len(), 4, false);
    assert_eq!((after.len(), focus), (long.len() + 4, long.len() + 4));
}

#[test]
fn shift_tab_is_the_editors_while_something_is_selected_and_leaves_focus_where_there_is_only_a_caret() {
    let focus = FocusNode::new();
    let controller = TextEditingController::with_text("ab\ncd");
    let _listener = AutoIndent::hold_tab_while_selected(focus.clone(), controller.clone());
    assert!(!focus.holds_tab(), "a caret: Shift+Tab moves focus out, as it does everywhere");
    controller.select_bytes(0, 3);
    assert!(focus.holds_tab(), "a selection: Shift+Tab is for outdenting it, and focus stays");
    controller.select_bytes(1, 1);
    assert!(!focus.holds_tab(), "the selection is gone");
    controller.select_bytes(4, 1);
    assert!(focus.holds_tab(), "a selection made backwards is one");
    controller.set_text("ab");
    controller.select_bytes(0, 0);
    assert!(!focus.holds_tab());
}

#[test]
fn a_selection_that_is_there_already_is_held_from_the_start() {
    let focus = FocusNode::new();
    let controller = TextEditingController::with_text("abc");
    controller.select_bytes(0, 2);
    let listener = AutoIndent::hold_tab_while_selected(focus.clone(), controller.clone());
    assert!(focus.holds_tab());
    drop(listener);
}
