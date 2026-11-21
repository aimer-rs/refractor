use arborium::tree_sitter::{Language, Parser, Tree};

use super::*;

fn parse(text: &str) -> Tree {
    let language: Language = arborium::lang_rust::language().into();
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .expect("the Rust grammar is one this tree-sitter reads");
    parser.parse(text, None).expect("a parse")
}

/// The depth of each bracket of `text` that `Brackets::depths` finds in `region`, as `(the byte, the depth)`, in order.
fn depths_in(text: &str, region: Range<usize>) -> Vec<(usize, usize)> {
    let tree = parse(text);
    Brackets::depths(&tree, region)
        .into_iter()
        .map(|(range, depth)| (range.start, depth))
        .collect()
}

/// The place of the `nth` (from zero) `needle` in `text`.
fn at(text: &str, needle: &str, nth: usize) -> usize {
    text.match_indices(needle)
        .nth(nth)
        .unwrap_or_else(|| panic!("{needle:?} number {nth} in {text:?}"))
        .0
}

#[test]
fn a_bracket_is_as_deep_as_the_brackets_around_it() {
    let text = "fn f(a: (u8, u8)) { let v = [1, (2)]; }";
    let found = depths_in(text, 0..text.len());
    let depth = |needle: &str, nth: usize| {
        found
            .iter()
            .find(|(byte, _)| *byte == at(text, needle, nth))
            .map(|(_, depth)| *depth)
    };
    assert_eq!(depth("(", 0), Some(0), "the parameters");
    assert_eq!(depth("(", 1), Some(1), "a tuple in them");
    assert_eq!(
        depth(")", 0),
        Some(1),
        "the tuple's closer is as deep as its opener"
    );
    assert_eq!(depth(")", 1), Some(0));
    assert_eq!(depth("{", 0), Some(0));
    assert_eq!(depth("[", 0), Some(1), "in the body");
    assert_eq!(depth("(", 2), Some(2), "in the array in the body");
    assert_eq!(depth(")", 2), Some(2));
    assert_eq!(depth("]", 0), Some(1));
    assert_eq!(depth("}", 0), Some(0));
    assert_eq!(
        found.len(),
        10,
        "every bracket, and nothing else: {found:?}"
    );
}

#[test]
fn the_brackets_of_a_macro_and_of_an_attribute_are_counted_too() {
    let text = "#[derive(Debug)]\nfn f() { v![(1, 2)]; }";
    let found = depths_in(text, 0..text.len());
    let depth = |needle: &str, nth: usize| {
        found
            .iter()
            .find(|(byte, _)| *byte == at(text, needle, nth))
            .map(|(_, depth)| *depth)
    };
    assert_eq!(depth("[", 0), Some(0), "an attribute's brackets");
    assert_eq!(depth("(", 0), Some(1), "its arguments are in them");
    assert_eq!(depth("{", 0), Some(0));
    assert_eq!(depth("[", 1), Some(1), "the macro's");
    assert_eq!(depth("(", 2), Some(2), "and the tuple in it");
}

#[test]
fn what_is_in_a_string_or_a_comment_is_not_a_bracket() {
    let text = "let s = \"((\"; // ( [\nlet c = '('; x(1);";
    let found = depths_in(text, 0..text.len());
    let starts: Vec<usize> = found.iter().map(|(byte, _)| *byte).collect();
    assert_eq!(
        starts,
        [at(text, "(1", 0), at(text, "1)", 0) + 1],
        "only the call's"
    );
}

#[test]
fn only_the_brackets_in_the_region_are_found_and_they_are_as_deep_as_they_are_in_the_whole() {
    let text = "fn f() {\n    g(h(1));\n    k([2]);\n}";
    let line = at(text, "    k(", 0);
    let region = line..text.len();
    let found = depths_in(text, region.clone());
    assert!(
        found
            .iter()
            .all(|(byte, _)| region.contains(byte)),
        "{found:?}"
    );
    let depth = |needle: &str| {
        found
            .iter()
            .find(|(byte, _)| *byte == at(text, needle, 0))
            .map(|(_, depth)| *depth)
    };
    assert_eq!(depth("k("), None, "k is not a bracket");
    assert_eq!(
        depth("(["),
        Some(1),
        "inside the function's braces, though they are outside the region"
    );
    assert_eq!(depth("[2"), Some(2));
    assert_eq!(depth("}"), Some(0));
    assert!(depths_in(text, 0..0).is_empty(), "an empty region has none");
}

#[test]
fn the_empty_text_and_text_that_is_not_code_have_no_brackets() {
    assert!(depths_in("", 0..0).is_empty());
    assert!(depths_in("let x = 1;", 0..10).is_empty());
}

#[test]
fn a_very_long_line_and_very_deep_nesting_are_answered() {
    let long = format!("f({});", "(1),".repeat(2000));
    assert_eq!(depths_in(&long, 0..long.len()).len(), 2 + 2000 * 2);
    let deep = format!("{}1{}", "(".repeat(300), ")".repeat(300));
    let found = depths_in(&deep, 0..deep.len());
    assert!(found.len() <= 600, "{}", found.len());
}

fn pair_next_to(text: &str) -> Option<(Range<usize>, Range<usize>, usize)> {
    let caret = text.find('|').expect("a | marks the caret");
    let text = text.replacen('|', "", 1);
    let tree = parse(&text);
    Brackets::adjacent(&tree, caret).map(|pair| (pair.open, pair.close, pair.depth))
}

#[test]
fn the_caret_beside_a_bracket_on_either_side_of_it_is_next_to_its_pair() {
    let call = Some((1..2, 6..7, 0));
    assert_eq!(pair_next_to("f(a, b)|;"), call, "after the closer");
    assert_eq!(pair_next_to("f|(a, b);"), call, "before the opener");
    assert_eq!(pair_next_to("f(|a, b);"), call, "after the opener");
    assert_eq!(pair_next_to("f(a, b|);"), call, "before the closer");
    assert_eq!(
        pair_next_to("fn f() {\n    x\n}|"),
        Some((7..8, 15..16, 0)),
        "a block, across lines"
    );
    assert_eq!(
        pair_next_to("\u{e9}(a)|"),
        Some((2..3, 4..5, 0)),
        "after a letter of more than a byte"
    );
}

#[test]
fn of_two_brackets_beside_the_caret_the_one_before_it_is_the_pair() {
    assert_eq!(
        pair_next_to("f(a)|(b)"),
        Some((1..2, 3..4, 0)),
        "the one just typed"
    );
    assert_eq!(
        pair_next_to("f(g(x)|)"),
        Some((3..4, 5..6, 1)),
        "the inner, which is before the caret, and as deep as it is"
    );
}

#[test]
fn the_caret_beside_no_bracket_or_one_with_no_other_half_or_one_in_a_string_is_next_to_nothing() {
    assert_eq!(pair_next_to("let x = 1;|"), None);
    assert_eq!(pair_next_to("|"), None, "the empty text");
    assert_eq!(pair_next_to("le|t x = (1);"), None, "in a word");
    assert_eq!(pair_next_to("let s = \"(|(\";"), None, "in a string");
    assert_eq!(pair_next_to("// (|\nlet x = 1;"), None, "in a comment");
    assert_eq!(
        pair_next_to("x = 1)|;"),
        None,
        "a closer that nothing opened"
    );
}

fn enclosing_pair(text: &str) -> Option<(Range<usize>, Range<usize>, usize)> {
    let caret = text.find('|').expect("a | marks the caret");
    let text = text.replacen('|', "", 1);
    let tree = parse(&text);
    Brackets::enclosing(&tree, caret).map(|pair| (pair.open, pair.close, pair.depth))
}

#[test]
fn the_pair_that_holds_the_caret_is_the_innermost_one() {
    assert_eq!(
        enclosing_pair("f(a, g(b|));"),
        Some((6..7, 8..9, 1)),
        "inside the inner call"
    );
    assert_eq!(
        enclosing_pair("f(a|, g(b));"),
        Some((1..2, 9..10, 0)),
        "between the outer's"
    );
    assert_eq!(
        enclosing_pair("fn f() {\n    |\n}"),
        Some((7..8, 14..15, 0)),
        "in the braces of a function, on a line of its own"
    );
    assert_eq!(
        enclosing_pair("f(|a);"),
        Some((1..2, 3..4, 0)),
        "right after the opener is inside"
    );
    assert_eq!(
        enclosing_pair("f(a|);"),
        Some((1..2, 3..4, 0)),
        "and right before the closer"
    );
}

#[test]
fn a_caret_not_inside_any_pair_has_none() {
    assert_eq!(enclosing_pair("let x = |1;"), None);
    assert_eq!(
        enclosing_pair("f|(a);"),
        None,
        "before the opener is outside"
    );
    assert_eq!(enclosing_pair("f(a)|;"), None, "and after the closer");
    assert_eq!(enclosing_pair("|"), None);
}
