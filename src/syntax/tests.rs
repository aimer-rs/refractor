use super::*;

const SAMPLE: &str = "use std::fmt;\n\n/// A point.\n#[derive(Debug)]\npub struct Point {\n    x: i32,\n}\n\nfn main() {\n    let name = \"caf\u{e9}\";\n    let n = 42; // done\n}\n";

fn highlighter() -> SyntaxHighlighter {
    SyntaxHighlighter::rust(SyntaxPalette::dark())
}

fn whole(text: &TextSnapshot) -> Range<usize> {
    0..text.len()
}

/// The style of the range holding `at`, if any.
fn style_at(ranges: &[StyledRange], at: usize) -> Option<&SpanStyle> {
    ranges
        .iter()
        .find(|range| range.range.contains(&at))
        .map(|range| &range.style)
}

fn offset_of(text: &str, needle: &str) -> usize {
    text.find(needle)
        .unwrap_or_else(|| panic!("{needle:?} is in the text"))
}

#[test]
fn the_tokens_of_a_rust_program_get_the_colors_of_their_kind() {
    let palette = SyntaxPalette::dark();
    let text = TextSnapshot::from_text(SAMPLE);
    let ranges = highlighter().ranges(&text, whole(&text));

    let color_at =
        |needle: &str| style_at(&ranges, offset_of(SAMPLE, needle)).and_then(|style| style.color);
    assert_eq!(color_at("pub struct"), Some(palette.keyword), "a keyword");
    assert_eq!(color_at("fn main"), Some(palette.keyword));
    assert_eq!(
        color_at("main()"),
        Some(palette.function),
        "a function name"
    );
    assert_eq!(color_at("Point {"), Some(palette.types), "a type");
    assert_eq!(color_at("\"caf"), Some(palette.string), "a string");
    assert_eq!(color_at("42"), Some(palette.number), "a number");
    assert_eq!(color_at("// done"), Some(palette.comment), "a comment");
    assert_eq!(
        style_at(&ranges, offset_of(SAMPLE, "// done")).and_then(|style| style.font_style),
        Some(aimer::style::FontStyle::Italic)
    );
}

#[test]
fn the_ranges_do_not_overlap_are_in_order_and_fall_on_characters() {
    let text = TextSnapshot::from_text(SAMPLE);
    let ranges = highlighter().ranges(&text, whole(&text));
    assert!(ranges.len() > 20, "{} ranges", ranges.len());
    let mut end = 0;
    for range in &ranges {
        assert!(
            range.range.start >= end,
            "{:?} overlaps what came before, which ends at {end}",
            range.range
        );
        assert!(range.range.start < range.range.end);
        assert!(
            SAMPLE.is_char_boundary(range.range.start) && SAMPLE.is_char_boundary(range.range.end),
            "{:?}",
            range.range
        );
        end = range.range.end;
    }
}

#[test]
fn only_the_part_asked_for_is_looked_up() {
    let text = TextSnapshot::from_text(SAMPLE);
    let from = offset_of(SAMPLE, "fn main");
    let to = offset_of(SAMPLE, "let n");
    let ranges = highlighter().ranges(&text, from..to);
    assert!(!ranges.is_empty());
    for range in &ranges {
        assert!(
            range.range.end > from && range.range.start < to,
            "{:?} is outside {from}..{to}",
            range.range
        );
    }
}

#[test]
fn asking_again_for_the_same_text_does_not_parse_again() {
    let highlighter = highlighter();
    let text = TextSnapshot::from_text(SAMPLE);
    let first = highlighter.ranges(&text, whole(&text));
    let again = highlighter.ranges(&text.clone(), whole(&text));
    assert_eq!(first, again);
    assert_eq!(highlighter.incremental_parses(), 0);
}

struct Random(u64);

impl Random {
    fn below(&mut self, below: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % below as u64) as usize
    }
}

#[test]
fn after_an_edit_the_tree_is_made_from_the_one_before_and_styles_what_a_fresh_parse_does() {
    let pieces = [
        "let a = 1;",
        "fn f() {}",
        "\n",
        "// note\n",
        "\"s\"",
        "struct S;",
        "x",
        " ",
        "}",
        "{",
        "'a",
        "r#\"raw\"#",
        "\u{e9}",
    ];
    let mut random = Random(0x9E37_79B9_7F4A_7C15);
    let kept = highlighter();
    let mut text = TextSnapshot::from_text(SAMPLE);
    kept.ranges(&text, whole(&text));
    let mut edits = 0;
    for round in 0..60 {
        let length = text.len();
        let mut start = random.below(length + 1);
        while !text.is_char_boundary(start) {
            start -= 1;
        }
        let mut end = (start + random.below(6)).min(length);
        while !text.is_char_boundary(end) {
            end += 1;
        }
        let inserted = pieces[random.below(pieces.len())];
        text = text.with_replaced(start..end, inserted);
        edits += 1;

        let incremental = kept.ranges(&text, whole(&text));
        let fresh = highlighter().ranges(&text, whole(&text));
        assert_eq!(
            incremental, fresh,
            "round {round}: {start}..{end} replaced by {inserted:?}"
        );
    }
    assert_eq!(
        kept.incremental_parses(),
        edits,
        "every edit was parsed from the tree before it"
    );
}

#[test]
fn a_range_of_the_edited_text_is_the_same_as_that_range_of_the_whole() {
    let highlighter = highlighter();
    let mut text = TextSnapshot::from_text(&SAMPLE.repeat(40));
    highlighter.ranges(&text, whole(&text));
    let middle = text.len() / 2;
    let middle = (0..=middle)
        .rev()
        .find(|&byte| text.is_char_boundary(byte))
        .expect("a boundary");
    text = text.with_replaced(middle..middle, "let inserted = 7; ");
    let window = middle.saturating_sub(300)..(middle + 300).min(text.len());
    let ranges = highlighter.ranges(&text, window.clone());
    let all = highlighter.ranges(&text, whole(&text));
    let inside =
        |range: &&StyledRange| range.range.end > window.start && range.range.start < window.end;
    let expected: Vec<&StyledRange> = all
        .iter()
        .filter(inside)
        .collect();
    let got: Vec<&StyledRange> = ranges
        .iter()
        .filter(inside)
        .collect();
    assert_eq!(got, expected);
}

#[test]
fn a_file_too_large_to_parse_at_once_is_left_plain_and_an_empty_one_has_no_styles() {
    let highlighter = highlighter();
    let empty = TextSnapshot::new();
    assert!(
        highlighter
            .ranges(&empty, 0..0)
            .is_empty()
    );
    let huge = TextSnapshot::from_text(&"fn f() {}\n".repeat(MAX_HIGHLIGHTED_BYTES / 9));
    assert!(huge.len() > MAX_HIGHLIGHTED_BYTES);
    assert!(
        highlighter
            .ranges(&huge, 0..100)
            .is_empty()
    );
    // And it recovers when the text is small again.
    let small = TextSnapshot::from_text("fn f() {}");
    assert!(
        !highlighter
            .ranges(&small, 0..9)
            .is_empty()
    );
}

#[test]
fn text_that_is_not_valid_rust_is_still_styled_where_it_can_be() {
    let text = TextSnapshot::from_text("fn broken( { let x = \"unclosed\n");
    let ranges = highlighter().ranges(&text, 0..text.len());
    assert!(
        style_at(&ranges, 0).is_some(),
        "fn is a keyword even in a broken file"
    );
}

const ITEMS: &str = "mod outer {\n    struct Point { x: i32 }\n\n    impl Point {\n        fn norm(&self) -> i32 {\n            let a = 1;\n            a\n        }\n    }\n\n    impl Display for Point {}\n}\n\nfn free() {}\n";

#[test]
fn the_symbol_at_a_byte_is_the_innermost_item_that_holds_it() {
    let highlighter = highlighter();
    let text = TextSnapshot::from_text(ITEMS);
    let symbol = |needle: &str| highlighter.symbol_at(&text, offset_of(ITEMS, needle));
    assert_eq!(
        symbol("let a"),
        Some("fn norm".to_owned()),
        "inside a method"
    );
    assert_eq!(symbol("x: i32"), Some("struct Point".to_owned()));
    assert_eq!(symbol("fn norm"), Some("fn norm".to_owned()));
    assert_eq!(
        symbol("Point {}"),
        Some("impl Display for Point".to_owned())
    );
    assert_eq!(symbol("struct Point"), Some("struct Point".to_owned()));
    assert_eq!(symbol("free"), Some("fn free".to_owned()));
    // An `impl` holds the method; a module holds the impl but the nearer one wins.
    assert_eq!(symbol("Point {\n        fn"), Some("impl Point".to_owned()));
    assert_eq!(symbol("mod outer"), Some("mod outer".to_owned()));
    assert_eq!(
        highlighter.symbol_at(&text, ITEMS.len()),
        None,
        "after the last item"
    );
    assert_eq!(
        highlighter.symbol_at(&TextSnapshot::new(), 0),
        None,
        "in an empty file"
    );
}

#[test]
fn the_symbol_follows_the_text_as_it_is_edited() {
    let highlighter = highlighter();
    let text = TextSnapshot::from_text(ITEMS);
    let at = offset_of(ITEMS, "let a");
    assert_eq!(highlighter.symbol_at(&text, at), Some("fn norm".to_owned()));
    let renamed = text.with_replaced(
        offset_of(ITEMS, "norm")..offset_of(ITEMS, "norm") + 4,
        "length",
    );
    assert_eq!(
        highlighter.symbol_at(&renamed, at + 2),
        Some("fn length".to_owned())
    );
    assert!(
        highlighter.incremental_parses() >= 1,
        "from the tree before it"
    );
}

const DOCS: &str = "//! Crate **docs**.\n\n/// A *point* of `code`.\npub struct Point {\n    x: i32, // plain\n}\n";

#[test]
fn a_doc_comment_keeps_the_comment_colors_underneath_its_embedded_rendering() {
    // The rendered block is an embed, which replaces its lines; the syntax still colors them as the
    // comment they are, which is what shows when the block is shown as written.
    let palette = SyntaxPalette::dark();
    let text = TextSnapshot::from_text(DOCS);
    let ranges = highlighter().ranges(&text, whole(&text));
    for pair in ranges.windows(2) {
        assert!(pair[0].range.end <= pair[1].range.start, "{pair:?}");
    }
    let color = |needle: &str| style_at(&ranges, offset_of(DOCS, needle)).and_then(|style| style.color);
    assert_eq!(color("/// A"), Some(palette.comment));
    assert_eq!(color("pub struct"), Some(palette.keyword));
    assert_eq!(color("// plain"), Some(palette.comment));
}

/// The ranges of the embeds the embedder finds in `DOCS`.
fn ranges_of(embedder: &aimer::input::input::TextEmbedder) -> Vec<Range<usize>> {
    embedder.embeds(DOCS, &HighlightContext::default()).iter().map(|embed| embed.range()).collect()
}

#[test]
fn the_embedder_embeds_each_rendered_block_and_none_that_is_written() {
    let highlighter = highlighter();
    let on_open: Rc<dyn Fn(usize)> = Rc::new(|_| {});
    let embedder = highlighter.doc_embedder(on_open);
    let embeds = ranges_of(&embedder);
    assert_eq!(embeds, vec![0..offset_of(DOCS, "\n\n"), offset_of(DOCS, "/// A")..offset_of(DOCS, "\npub struct")]);
    highlighter.toggle_doc_block(2);
    assert_eq!(ranges_of(&embedder), vec![0..offset_of(DOCS, "\n\n")], "the block that is shown as written is not embedded");
}

#[test]
fn a_block_toggled_to_written_reports_it_and_toggles_back() {
    let text = TextSnapshot::from_text(DOCS);
    let highlighter = highlighter();
    assert_eq!(highlighter.doc_block_at(&text, 0), Some(true), "a block starts on the first line, rendered");
    assert_eq!(highlighter.doc_block_at(&text, 1), None, "not on a blank line");
    assert_eq!(highlighter.doc_block_at(&text, 2), Some(true));
    assert_eq!(highlighter.doc_block_at(&text, 4), None, "nor in code");
    highlighter.toggle_doc_block(2);
    assert_eq!(highlighter.doc_block_at(&text, 2), Some(false));
    assert_eq!(highlighter.doc_block_at(&text, 0), Some(true), "the other block is untouched");
    highlighter.toggle_doc_block(2);
    assert_eq!(highlighter.doc_block_at(&text, 2), Some(true));
}

#[test]
fn a_block_shown_as_written_stays_with_its_lines_when_lines_are_added_above_it() {
    let text = TextSnapshot::from_text(DOCS);
    let highlighter = highlighter();
    let _ = highlighter.ranges(&text, whole(&text));
    highlighter.toggle_doc_block(2);
    // Two lines are typed in at the very top.
    let edited = text.with_replaced(0..0, "// a\n// b\n");
    let _ = highlighter.ranges(&edited, whole(&edited));
    assert_eq!(highlighter.doc_block_at(&edited, 4), Some(false), "the block moved to line 4 and is still written");
    assert_eq!(highlighter.doc_block_at(&edited, 2), Some(true), "and the crate docs, now on line 2, are rendered");
}

#[test]
fn typing_into_a_rendered_block_shows_it_as_written() {
    let text = TextSnapshot::from_text(DOCS);
    let highlighter = highlighter();
    assert!(highlighter.open_doc_block_at(&text, 2));
    assert_eq!(highlighter.doc_block_at(&text, 2), Some(false));
    assert!(!highlighter.open_doc_block_at(&text, 3), "code is not a doc block");
}

#[test]
fn a_rendered_block_has_a_bar_at_its_indent_and_a_written_one_has_none() {
    let source = "/// one\n    /// two\nfn f() {}\n";
    let text = TextSnapshot::from_text(source);
    let highlighter = highlighter();
    assert_eq!(highlighter.doc_bar(&text, 0), Some(0));
    assert_eq!(highlighter.doc_bar(&text, 2), None, "code has none");
    highlighter.toggle_doc_block(0);
    assert_eq!(highlighter.doc_bar(&text, 0), None);
}
