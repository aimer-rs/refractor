use aimer::input::input::{TextEditingController, TextSnapshot};

use super::*;

/// What typing `typed` does at the `|` of `text`, which is taken out of it.
fn typed_at(text: &str, typed: &str) -> Option<Typed> {
    let at = text.find('|').expect("a | marks the caret");
    let text = text.replacen('|', "", 1);
    AutoPair::typed(&TextSnapshot::from_text(&text), at..at, typed)
}

fn pair(open: char, close: char) -> Option<Typed> {
    Some(Typed::Pair { open, close })
}

#[test]
fn an_opening_bracket_is_closed_where_what_follows_is_not_a_word() {
    assert_eq!(typed_at("|", "("), pair('(', ')'), "the empty text");
    assert_eq!(
        typed_at("foo|", "("),
        pair('(', ')'),
        "the end of the text, which has no final line break"
    );
    assert_eq!(
        typed_at("foo|\nbar", "["),
        pair('[', ']'),
        "the end of a line"
    );
    assert_eq!(
        typed_at("foo|\r\nbar", "{"),
        pair('{', '}'),
        "the end of a line that ends in CRLF"
    );
    assert_eq!(typed_at("a|)", "("), pair('(', ')'), "before a closer");
    assert_eq!(typed_at("a| b", "("), pair('(', ')'), "before a space");
    assert_eq!(typed_at("a|, b", "("), pair('(', ')'), "before a comma");
    assert_eq!(
        typed_at("|foo", "("),
        None,
        "before a word it opens what is there, and is not closed"
    );
    assert_eq!(
        typed_at("|\u{e9}", "("),
        None,
        "before a character of more than one byte that is a letter"
    );
    assert_eq!(typed_at("a|b", "{"), None);
}

#[test]
fn a_closing_bracket_that_is_there_is_stepped_over_and_one_that_is_not_is_typed() {
    assert_eq!(typed_at("(a|)", ")"), Some(Typed::Over { len: 1 }));
    assert_eq!(typed_at("[a|]", "]"), Some(Typed::Over { len: 1 }));
    assert_eq!(typed_at("{a|}", "}"), Some(Typed::Over { len: 1 }));
    assert_eq!(typed_at("(a|b", ")"), None, "nothing to step over");
    assert_eq!(
        typed_at("(a|]", ")"),
        None,
        "another closer is not this one"
    );
    assert_eq!(typed_at("(a|", ")"), None, "the end of the text");
}

#[test]
fn a_quote_is_closed_where_a_string_may_begin_and_stepped_over_where_one_ends() {
    assert_eq!(typed_at("let s = |", "\""), pair('"', '"'));
    assert_eq!(typed_at("f(|)", "\""), pair('"', '"'));
    assert_eq!(
        typed_at("let s = \"abc|\"", "\""),
        Some(Typed::Over { len: 1 }),
        "the end of a string"
    );
    assert_eq!(
        typed_at("let s = \"abc |", "\""),
        None,
        "inside a string that has no closing quote: this one closes it"
    );
    assert_eq!(
        typed_at("let s = \"a\\\"|\"", "\""),
        Some(Typed::Over { len: 1 }),
        "an escaped quote does not end the string"
    );
    assert_eq!(
        typed_at("let s = \"a\\\\|\"", "\""),
        Some(Typed::Over { len: 1 }),
        "a backslash that is escaped does not escape the quote after it"
    );
    assert_eq!(
        typed_at("let s = r|", "\""),
        pair('"', '"'),
        "the prefix of a raw string is closed as a string is"
    );
    assert_eq!(
        typed_at("let s = r#|", "\""),
        Some(Typed::Raw { hashes: 1 }),
        "and a raw string with marks is closed with its marks"
    );
    assert_eq!(
        typed_at("let s = \"\"|", "\""),
        None,
        "after an empty string"
    );
    assert_eq!(
        typed_at("f(|\"a\")", "\""),
        None,
        "before a string that is there: it is not an end"
    );
    assert_eq!(
        typed_at("let s = \"\u{e9}|", "\""),
        None,
        "a quote after a letter that is more than a byte is a closing one"
    );
    assert_eq!(
        typed_at("let c = '\"'; let s = |", "\""),
        pair('"', '"'),
        "the character that is a quote does not begin a string"
    );
}

#[test]
fn a_single_quote_is_closed_only_where_a_character_is_begun_and_not_a_lifetime() {
    assert_eq!(typed_at("let c = |", "'"), pair('\'', '\''));
    assert_eq!(typed_at("f(|)", "'"), pair('\'', '\''));
    assert_eq!(typed_at("c == |", "'"), pair('\'', '\''));
    assert_eq!(typed_at("fn f<|", "'"), None, "a lifetime");
    assert_eq!(
        typed_at("fn f(x: &|", "'"),
        None,
        "a lifetime of a reference"
    );
    assert_eq!(typed_at("fn f<'a, |", "'"), None, "the next lifetime");
    assert_eq!(typed_at("T: 'a + |", "'"), None, "a bound");
    assert_eq!(
        typed_at("b|", "'"),
        pair('\'', '\''),
        "the prefix of a byte character"
    );
    assert_eq!(
        typed_at("|", "'"),
        None,
        "the start of a line is a label or a lifetime as likely as a character"
    );
    assert_eq!(
        typed_at("let c = '+|'", "'"),
        Some(Typed::Over { len: 1 }),
        "the end of a character that is a sign"
    );
    assert_eq!(typed_at("let c = 'a|'", "'"), Some(Typed::Over { len: 1 }));
    assert_eq!(
        typed_at("let c = '|'", "'"),
        Some(Typed::Over { len: 1 }),
        "an empty one"
    );
    assert_eq!(
        typed_at("let c = '\\n|'", "'"),
        Some(Typed::Over { len: 1 }),
        "an escape"
    );
    assert_eq!(
        typed_at("let c = '\u{e9}|'", "'"),
        Some(Typed::Over { len: 1 }),
        "a letter of more than a byte"
    );
    assert_eq!(
        typed_at("f(|'a')", "'"),
        None,
        "before a character that is there"
    );
}

#[test]
fn typing_an_opener_over_a_selection_wraps_it_and_anything_else_replaces_it_as_ever() {
    let text = TextSnapshot::from_text("let x = word;");
    for (typed, open, close) in [
        ("(", '(', ')'),
        ("[", '[', ']'),
        ("{", '{', '}'),
        ("\"", '"', '"'),
        ("'", '\'', '\''),
    ] {
        assert_eq!(
            AutoPair::typed(&text, 8..12, typed),
            Some(Typed::Wrap { open, close }),
            "{typed}"
        );
    }
    assert_eq!(
        AutoPair::typed(&text, 8..12, ")"),
        None,
        "a closer replaces the selection"
    );
    assert_eq!(AutoPair::typed(&text, 8..12, "a"), None);
}

#[test]
fn only_one_typed_character_is_taken_and_not_what_a_phrase_or_a_dead_key_commits() {
    assert_eq!(typed_at("|", "(("), None);
    assert_eq!(typed_at("|", ""), None);
    assert_eq!(typed_at("|", "\u{4f60}\u{597d}"), None);
    assert_eq!(typed_at("|", "a"), None);
    assert_eq!(typed_at("|", "\u{e9}"), None);
}

#[test]
fn a_quote_after_a_very_long_line_is_left_alone_and_a_bracket_is_not() {
    let long = "x".repeat(AutoPair::LOOKBEHIND * 4);
    // How far a string is known to be open is only known as far as the line is read back.
    assert_eq!(typed_at(&format!("{long} = |"), "\""), None);
    assert_eq!(typed_at(&format!("{long}|"), "("), pair('(', ')'));
    // A line that is long but within reach is read.
    let near = "x".repeat(AutoPair::LOOKBEHIND / 2);
    assert_eq!(typed_at(&format!("{near} = |"), "\""), pair('"', '"'));
}

#[test]
fn backspace_in_an_empty_pair_takes_both_and_nowhere_else() {
    let at = |text: &str| {
        let caret = text.find('|').expect("a | marks the caret");
        let text = text.replacen('|', "", 1);
        AutoPair::backspace(&TextSnapshot::from_text(&text), caret..caret)
    };
    assert_eq!(at("a(|)b"), Some(1..3));
    assert_eq!(at("a[|]b"), Some(1..3));
    assert_eq!(at("a{|}b"), Some(1..3));
    assert_eq!(at("a\"|\"b"), Some(1..3));
    assert_eq!(at("a'|'b"), Some(1..3));
    assert_eq!(at("(|"), None, "an opener with nothing to pair it");
    assert_eq!(at("|)"), None);
    assert_eq!(at("(|]"), None, "not a pair");
    assert_eq!(at("(a|)"), None, "not empty");
    assert_eq!(at("|"), None, "the empty text");
    assert_eq!(
        at("\u{e9}|)"),
        None,
        "a character of more than a byte before"
    );
    let text = TextSnapshot::from_text("()");
    assert_eq!(
        AutoPair::backspace(&text, 0..2),
        None,
        "a selection is deleted as it is"
    );
}

mod edits {
    use super::*;

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
    fn a_pair_is_one_edit_with_the_caret_between_and_one_undo_takes_it_back() {
        let controller = controller_at("f|");
        assert!(AutoPair::apply_typed(&controller, "("));
        assert_eq!(controller.value().text(), "f()");
        assert_eq!(selection(&controller), 2..2);
        assert!(controller.undo());
        assert_eq!(
            controller.value().text(),
            "f",
            "the opener and the closer go together"
        );
    }

    #[test]
    fn a_selection_is_wrapped_and_still_selected_so_the_next_pair_wraps_it_again() {
        let controller = TextEditingController::with_text("let x = word;");
        controller.select_bytes(8, 12);
        assert!(AutoPair::apply_typed(&controller, "("));
        assert_eq!(controller.value().text(), "let x = (word);");
        assert_eq!(selection(&controller), 9..13);
        assert!(AutoPair::apply_typed(&controller, "\""));
        assert_eq!(controller.value().text(), "let x = (\"word\");");
        assert_eq!(selection(&controller), 10..14);
        assert!(controller.undo());
        assert!(controller.undo());
        assert_eq!(controller.value().text(), "let x = word;");
    }

    #[test]
    fn a_selection_that_runs_backwards_is_wrapped_too() {
        let controller = TextEditingController::with_text("ab cd");
        controller.select_bytes(5, 3);
        assert!(AutoPair::apply_typed(&controller, "["));
        assert_eq!(controller.value().text(), "ab [cd]");
    }

    #[test]
    fn a_closer_that_is_there_is_stepped_over_and_the_text_is_not_touched() {
        let controller = controller_at("f(a|)");
        assert!(AutoPair::apply_typed(&controller, ")"));
        assert_eq!(controller.value().text(), "f(a)");
        assert_eq!(selection(&controller), 4..4);
    }

    #[test]
    fn a_character_of_more_than_one_byte_between_the_marks_is_stepped_over_whole() {
        let controller = controller_at("'\u{e9}|'");
        assert!(AutoPair::apply_typed(&controller, "'"));
        assert_eq!(
            selection(&controller),
            4..4,
            "the quote is a byte and the letter two"
        );
    }

    #[test]
    fn a_character_that_is_not_for_pairing_is_left_to_the_field() {
        let controller = controller_at("f|x");
        assert!(!AutoPair::apply_typed(&controller, "("));
        assert!(!AutoPair::apply_typed(&controller, "a"));
        assert_eq!(controller.value().text(), "fx");
    }

    #[test]
    fn backspace_takes_an_empty_pair_in_one_edit() {
        let controller = controller_at("f(|)g");
        assert!(AutoPair::apply_backspace(&controller));
        assert_eq!(controller.value().text(), "fg");
        assert_eq!(selection(&controller), 1..1);
        assert!(controller.undo());
        assert_eq!(controller.value().text(), "f()g");
        let controller = controller_at("f(a|)g");
        assert!(!AutoPair::apply_backspace(&controller), "left to the field");
        assert_eq!(controller.value().text(), "f(a)g");
    }
}

fn raw(hashes: usize) -> Option<Typed> {
    Some(Typed::Raw { hashes })
}

#[test]
fn an_angle_bracket_is_closed_where_a_generic_begins_and_not_where_it_compares() {
    for before in [
        "let v: Vec|",
        "let m: HashMap|",
        "-> Self|",
        "x.collect::|",
        "fn f|",
        "pub fn new|",
        "pub(crate) struct Wrapper|",
        "enum E|",
        "trait T|",
        "type Alias|",
        "impl|",
        "impl |",
        "for|",
        "let x = |",
        "f(|)",
        "let a: (u8, |)",
        "let t: |",
    ] {
        assert_eq!(typed_at(before, "<"), pair('<', '>'), "{before:?}");
    }
    for before in [
        "if a|",
        "if a |",
        "while count|",
        "if 1|",
        "if x.len() |",
        "a <|",
        "let v = Vec|word",
        "|word",
        "let s = \"Vec|",
        "let s = \"a{|",
        "|",
    ] {
        assert_eq!(typed_at(before, "<"), None, "{before:?}");
    }
    assert_eq!(
        typed_at("Vec<|>", "<"),
        pair('<', '>'),
        "a generic in a generic"
    );
    assert_eq!(typed_at("a <|", "<"), None, "a shift is not a generic");
    let near = "x".repeat(AutoPair::LOOKBEHIND / 2);
    assert_eq!(typed_at(&format!("{near} Vec|"), "<"), pair('<', '>'));
    let long = "x".repeat(AutoPair::LOOKBEHIND * 4);
    assert_eq!(
        typed_at(&format!("{long} Vec|"), "<"),
        None,
        "a line too long to tell if it is in a string"
    );
}

#[test]
fn a_closing_angle_bracket_that_is_there_is_stepped_over_only_for_one_that_was_opened() {
    let over = Some(Typed::Over { len: 1 });
    assert_eq!(typed_at("let v: Vec<u8|>", ">"), over);
    assert_eq!(typed_at("let v: Vec<Vec<u8>|>", ">"), over);
    assert_eq!(typed_at("let v: Vec<Vec<u8|>>", ">"), over);
    assert_eq!(typed_at("let v: Vec<|>", ">"), over);
    assert_eq!(
        typed_at("let f: Box<dyn Fn() -|>", ">"),
        None,
        "the arrow of a return type is not the end of the generic"
    );
    assert_eq!(typed_at("x =|>", ">"), None, "nor is the arrow of an arm");
    assert_eq!(typed_at("if a|>", ">"), None, "nothing was opened");
    assert_eq!(typed_at("let v: Vec<u8|", ">"), None, "nothing is there to step over");
    assert_eq!(
        typed_at("fn f<\n    T|>", ">"),
        None,
        "an opener on another line is not read"
    );
}

#[test]
fn backspace_takes_an_empty_angle_pair() {
    let at = |text: &str| {
        let caret = text.find('|').expect("a | marks the caret");
        let text = text.replacen('|', "", 1);
        AutoPair::backspace(&TextSnapshot::from_text(&text), caret..caret)
    };
    assert_eq!(at("Vec<|>"), Some(3..5));
    assert_eq!(at("Vec<a|>"), None);
}

#[test]
fn an_angle_bracket_over_a_selection_replaces_it_as_ever() {
    let text = TextSnapshot::from_text("Vec");
    assert_eq!(AutoPair::typed(&text, 0..3, "<"), None);
    assert_eq!(AutoPair::typed(&text, 0..3, ">"), None);
}

#[test]
fn a_prefix_of_a_string_is_closed_and_a_word_that_ends_in_one_is_not() {
    for before in ["let s = r|", "b|", "br|", "c|", "cr|", "f(r|)", "r|", "x = br|"] {
        assert_eq!(typed_at(before, "\""), pair('"', '"'), "{before:?}");
    }
    for before in ["bar|", "var|", "format_r|", "xbr|", "r2|", "\u{e9}r|"] {
        assert_eq!(typed_at(before, "\""), None, "{before:?}");
    }
    assert_eq!(typed_at("r|word", "\""), None, "before a word");
}

#[test]
fn a_raw_string_is_closed_with_as_many_marks_as_it_was_opened_with() {
    assert_eq!(typed_at("let s = r#|", "\""), raw(1));
    assert_eq!(typed_at("let s = r##|", "\""), raw(2));
    assert_eq!(typed_at("let s = br#|", "\""), raw(1));
    assert_eq!(typed_at("let s = cr###|", "\""), raw(3));
    assert_eq!(typed_at("f(r#|)", "\""), raw(1));
    assert_eq!(typed_at("let s = ar#|", "\""), None, "a word, and not a raw string");
    assert_eq!(typed_at("let s = b#|", "\""), None, "a byte string is not raw");
    assert_eq!(typed_at("let s = #|", "\""), None);
    assert_eq!(typed_at("let s = x#|", "\""), None);
    assert_eq!(typed_at("r#|word", "\""), None, "before a word");
}

#[test]
fn the_end_of_a_raw_string_is_stepped_over_a_mark_at_a_time_and_a_quote_in_it_is_typed() {
    let over = Some(Typed::Over { len: 1 });
    assert_eq!(typed_at("let s = r#\"abc|\"#", "\""), over, "the quote");
    assert_eq!(typed_at("let s = r#\"abc\"|#", "#"), over, "the mark");
    assert_eq!(typed_at("let s = r##\"abc\"|##", "#"), over);
    assert_eq!(typed_at("let s = r##\"abc\"#|#", "#"), over);
    assert_eq!(typed_at("let s = r\"abc|\"", "\""), over, "a raw string with no marks");
    assert_eq!(
        typed_at("let s = r\"abc\\|\"", "\""),
        over,
        "a backslash does not escape in a raw string"
    );
    assert_eq!(
        typed_at("let s = r#\"say |hi\"#", "\""),
        None,
        "a quote in the text of it is typed"
    );
    assert_eq!(
        typed_at("let s = r#\"a|\"", "\""),
        None,
        "a quote that has no marks after it does not end it"
    );
    assert_eq!(
        typed_at("let s = r##\"a|\"#", "\""),
        None,
        "nor does one that has too few"
    );
    assert_eq!(
        typed_at("let s = r#\"abc\"|", "#"),
        None,
        "no mark after the caret is a mark to step over"
    );
    assert_eq!(typed_at("let s = r#\"abc|#", "#"), None, "a mark in the text");
    assert_eq!(typed_at("let a = r#\"x\"#; let b = |", "\""), pair('"', '"'), "a string that was closed");
    assert_eq!(typed_at("#|#", "#"), None, "marks outside a string are typed");
    assert_eq!(
        typed_at("let s = \"a\"|#", "#"),
        None,
        "a mark after a string that is not raw"
    );
}

#[test]
fn a_byte_character_is_closed_and_a_label_that_ends_in_b_is_not() {
    assert_eq!(typed_at("let c = b|", "'"), pair('\'', '\''));
    assert_eq!(typed_at("match x { b|", "'"), pair('\'', '\''));
    assert_eq!(typed_at("let c = ab|", "'"), None);
    assert_eq!(typed_at("let c = |b", "'"), None);
}

mod literals {
    use super::*;

    fn controller_at(text: &str) -> TextEditingController {
        let caret = text.find('|').expect("a | marks the caret");
        let controller = TextEditingController::with_text(text.replacen('|', "", 1));
        controller.select_bytes(caret, caret);
        controller
    }

    #[test]
    fn a_raw_string_is_one_edit_with_the_caret_between_its_quotes_and_one_undo_takes_it_back() {
        let controller = controller_at("let s = r##|");
        assert!(AutoPair::apply_typed(&controller, "\""));
        assert_eq!(controller.value().text(), "let s = r##\"\"##");
        assert_eq!(controller.value().selection().range(), 12..12);
        assert!(controller.undo());
        assert_eq!(controller.value().text(), "let s = r##");
    }

    #[test]
    fn typing_a_raw_string_through_gives_the_text_that_was_meant() {
        let controller = controller_at("let s = r#|");
        for typed in ["\"", "h", "i", "\"", "#"] {
            if !AutoPair::apply_typed(&controller, typed) {
                let at = controller.value().selection().range().start;
                controller.replace_range(at..at, typed);
            }
        }
        assert_eq!(controller.value().text(), "let s = r#\"hi\"#");
        assert_eq!(controller.value().selection().range(), 15..15);
    }

    #[test]
    fn typing_a_generic_through_gives_the_text_that_was_meant() {
        let controller = controller_at("let v: |");
        for typed in ["V", "e", "c", "<", "V", "e", "c", "<", "u", "8", ">", ">"] {
            if !AutoPair::apply_typed(&controller, typed) {
                let at = controller.value().selection().range().start;
                controller.replace_range(at..at, typed);
            }
        }
        assert_eq!(controller.value().text(), "let v: Vec<Vec<u8>>");
        assert_eq!(controller.value().selection().range(), 19..19);
    }

    #[test]
    fn a_generic_is_one_edit_and_backspace_takes_both_marks() {
        let controller = controller_at("let v: Vec|");
        assert!(AutoPair::apply_typed(&controller, "<"));
        assert_eq!(controller.value().text(), "let v: Vec<>");
        assert!(AutoPair::apply_backspace(&controller));
        assert_eq!(controller.value().text(), "let v: Vec");
    }
}
