use aimer::input::input::TextEditingController;

use super::*;

/// Selects `anchor` to `focus` in `text` and toggles the comment with `//`: the text and the selection that are left.
fn toggled(text: &str, anchor: usize, focus: usize) -> (String, usize, usize) {
    let controller = TextEditingController::with_text(text);
    controller.select_bytes(anchor, focus);
    LineComment::apply(&controller, LineComment::RUST);
    let value = controller.value();
    (
        value.text().to_string(),
        value.selection().anchor(),
        value.selection().focus(),
    )
}

#[test]
fn the_shortcut_is_one_the_framework_reads() {
    // A chord that does not parse panics when the editor is built, which is far from where it was written.
    let chord = aimer::shortcut::KeyChord::parse(LineComment::SHORTCUT).expect("the chord parses");
    let other = aimer::shortcut::KeyChord::parse("CmdOrCtrl+S").expect("parses");
    assert_ne!(chord, other);
}

#[test]
fn a_caret_comments_its_line_at_the_indent_and_goes_on_after_what_was_put_in() {
    let (after, anchor, focus) = toggled("    let x = 1;\n", 8, 8);
    assert_eq!(after, "    // let x = 1;\n");
    assert_eq!((anchor, focus), (11, 11), "the caret stays with its text");
    // At the indent itself, where what is put in goes: the caret is after it, so that typing goes on from the line.
    let (_, anchor, focus) = toggled("    let x = 1;\n", 4, 4);
    assert_eq!((anchor, focus), (7, 7));
    assert_eq!(
        toggled("fn f() {}", 0, 0).0,
        "// fn f() {}",
        "a line that is not indented"
    );
    assert_eq!(
        toggled("x", 1, 1),
        ("// x".to_owned(), 4, 4),
        "at the end of a text with no line break"
    );
}

#[test]
fn a_commented_line_is_uncommented_with_the_space_that_follows_the_marker() {
    let (after, anchor, focus) = toggled("    // let x = 1;\n", 12, 12);
    assert_eq!(after, "    let x = 1;\n");
    assert_eq!((anchor, focus), (9, 9));
    assert_eq!(
        toggled("//let x;", 4, 4).0,
        "let x;",
        "a marker with no space after it"
    );
    assert_eq!(
        toggled("//  two spaces", 5, 5).0,
        " two spaces",
        "only one space goes: the rest is the text's"
    );
    assert_eq!(toggled("//", 1, 1).0, "", "a marker alone");
    assert_eq!(toggled("\t// x", 3, 3).0, "\tx", "after a tab");
    // A caret inside what is taken goes to where the taking began.
    assert_eq!((toggled("// x", 1, 1).1, toggled("// x", 1, 1).2), (0, 0));
}

#[test]
fn a_selection_comments_all_its_lines_at_the_indent_they_share_and_keeps_the_same_text_selected() {
    let text = "fn f() {\n    a();\n\n        b();\n}\n";
    // From before the `a` to after the `b`.
    let (after, anchor, focus) = toggled(text, 13, 28);
    assert_eq!(
        after, "fn f() {\n    // a();\n\n    //     b();\n}\n",
        "the marker is where the indent that both lines have ends, and the empty line is left alone"
    );
    assert_eq!(
        (anchor, focus),
        (13, 34),
        "the start stays at the indent, so what was put in is selected too"
    );
    assert_eq!(&after[anchor..focus], "// a();\n\n    //     b");
}

#[test]
fn lines_are_uncommented_when_every_line_that_has_text_is_commented_and_commented_again_when_one_is_not()
 {
    assert_eq!(
        toggled("// a\n\n// b", 0, 10).0,
        "a\n\nb",
        "empty lines do not count"
    );
    assert_eq!(
        toggled("// a\nb", 0, 6).0,
        "// // a\n// b",
        "one line that is not commented: all are commented"
    );
    assert_eq!(
        toggled("  // a\n  \n  // b", 0, 16).0,
        "  a\n  \n  b",
        "a line of blanks does not count either"
    );
}

#[test]
fn a_selection_that_ends_at_the_start_of_a_line_leaves_that_line_alone() {
    assert_eq!(toggled("a\nb", 0, 2).0, "// a\nb");
    assert_eq!(
        toggled("a\n", 0, 2).0,
        "// a\n",
        "and the empty line a final break makes is not one"
    );
}

#[test]
fn doc_comments_are_not_a_marker_to_take_away_and_are_commented_like_code() {
    assert_eq!(toggled("/// doc", 2, 2).0, "// /// doc");
    assert_eq!(
        toggled("// /// doc", 5, 5).0,
        "/// doc",
        "and the comment is taken away again, leaving the doc comment"
    );
    assert_eq!(toggled("//! inner", 2, 2).0, "// //! inner");
    assert_eq!(toggled("///", 1, 1).0, "// ///");
}

#[test]
fn a_line_of_only_blanks_or_no_text_has_nothing_to_comment() {
    for text in ["", "   ", "\n", "a\n\nb"] {
        let controller = TextEditingController::with_text(text);
        controller.select_bytes(
            if text == "a\n\nb" { 2 } else { 0 },
            if text == "a\n\nb" { 2 } else { 0 },
        );
        assert!(
            !LineComment::apply(&controller, LineComment::RUST),
            "{text:?}"
        );
        assert_eq!(controller.value().text(), text);
    }
}

#[test]
fn lines_ending_in_two_characters_and_text_of_many_bytes_are_commented_where_the_indent_ends() {
    let text = "\u{e9}a\r\n  \u{e9}\u{301}b\r\nc";
    let (after, anchor, focus) = toggled(text, 0, text.len());
    assert_eq!(
        after, "// \u{e9}a\r\n// \u{20}\u{20}\u{e9}\u{301}b\r\n// c",
        "the break is not part of a line, and the letters are not cut"
    );
    assert_eq!(
        toggled(&after, 0, after.len()).0,
        "\u{e9}a\r\n  \u{e9}\u{301}b\r\nc",
        "and it is taken away exactly as it was put"
    );
    assert!(anchor <= focus);
}

#[test]
fn a_very_long_line_is_commented_and_the_comment_is_one_edit_that_one_undo_takes_back() {
    let long = "x".repeat(100_000);
    let (after, ..) = toggled(&long, 0, 0);
    assert_eq!(after.len(), long.len() + 3);
    let controller = TextEditingController::with_text("a\nb");
    controller.select_bytes(0, 3);
    assert!(LineComment::apply(&controller, LineComment::RUST));
    assert_eq!(controller.value().text(), "// a\n// b");
    assert!(controller.undo());
    assert_eq!(controller.value().text(), "a\nb");
}

#[test]
fn a_marker_other_than_the_slashes_is_the_languages() {
    let controller = TextEditingController::with_text("key = 1\n");
    controller.select_bytes(0, 0);
    assert!(LineComment::apply(&controller, "#"));
    assert_eq!(controller.value().text(), "# key = 1\n");
}

#[test]
fn only_rust_has_a_marker_and_prose_and_plain_text_are_left_alone() {
    use crate::model::document::Language;
    assert_eq!(Language::Rust.line_comment(), Some(LineComment::RUST));
    assert_eq!(
        Language::Markdown.line_comment(),
        None,
        "a comment in markdown is not a line's"
    );
    assert_eq!(Language::PlainText.line_comment(), None);
}
