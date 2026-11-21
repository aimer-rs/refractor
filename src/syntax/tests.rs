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
fn what_is_drawn_over_the_grammar_is_given_the_runs_of_the_part_and_what_it_says_is_the_highlight() {
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let kept = std::rc::Rc::clone(&seen);
    let over = highlighter().highlighter_over(move |runs, text, range| {
        kept.borrow_mut().push((runs.len(), text.len(), range));
        // The first run is taken away and one of ours is put in its place.
        let mut runs = runs;
        runs.remove(0);
        runs.push(StyledRange::new(0..2, SpanStyle::new().color(aimer::Color::Rgb(1, 2, 3))));
        runs
    });
    let plain = highlighter().highlighter();
    let (found, expected) = (over.highlight(SAMPLE), plain.highlight(SAMPLE));
    assert_eq!(found.len(), expected.len(), "one taken and one put");
    assert_eq!(found.last().map(|run| run.range.clone()), Some(0..2));
    let seen = seen.borrow();
    assert_eq!(seen.len(), 1);
    assert_eq!((seen[0].0, seen[0].1, seen[0].2.clone()), (expected.len(), SAMPLE.len(), 0..SAMPLE.len()), "the runs of the whole text, as the grammar made them");
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
    let expected: Vec<&StyledRange> = all.iter().filter(inside).collect();
    let got: Vec<&StyledRange> = ranges.iter().filter(inside).collect();
    assert_eq!(got, expected);
}

#[test]
fn a_file_too_large_to_parse_at_once_is_left_plain_and_an_empty_one_has_no_styles() {
    let highlighter = highlighter();
    let empty = TextSnapshot::new();
    assert!(highlighter.ranges(&empty, 0..0).is_empty());
    let huge = TextSnapshot::from_text(&"fn f() {}\n".repeat(MAX_HIGHLIGHTED_BYTES / 9));
    assert!(huge.len() > MAX_HIGHLIGHTED_BYTES);
    assert!(highlighter.ranges(&huge, 0..100).is_empty());
    // And it recovers when the text is small again.
    let small = TextSnapshot::from_text("fn f() {}");
    assert!(!highlighter.ranges(&small, 0..9).is_empty());
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
    let color =
        |needle: &str| style_at(&ranges, offset_of(DOCS, needle)).and_then(|style| style.color);
    assert_eq!(color("/// A"), Some(palette.comment));
    assert_eq!(color("pub struct"), Some(palette.keyword));
    assert_eq!(color("// plain"), Some(palette.comment));
}

/// The ranges of the embeds the embedder finds in `DOCS`.
fn ranges_of(embedder: &aimer::input::input::TextEmbedder) -> Vec<Range<usize>> {
    embedder
        .embeds(DOCS, &HighlightContext::default())
        .iter()
        .map(|embed| embed.range())
        .collect()
}

#[test]
fn the_embedder_embeds_each_rendered_block_and_none_that_is_written() {
    let highlighter = highlighter();
    let embedder = highlighter.doc_embedder();
    let embeds = ranges_of(&embedder);
    assert_eq!(
        embeds,
        vec![
            0..offset_of(DOCS, "\n\n"),
            offset_of(DOCS, "/// A")..offset_of(DOCS, "\npub struct")
        ]
    );
    highlighter.toggle_doc_block(2);
    assert_eq!(
        ranges_of(&embedder),
        vec![0..offset_of(DOCS, "\n\n")],
        "the block that is shown as written is not embedded"
    );
}

#[test]
fn a_block_toggled_to_written_reports_it_and_toggles_back() {
    let text = TextSnapshot::from_text(DOCS);
    let highlighter = highlighter();
    assert_eq!(
        highlighter.doc_block_at(&text, 0),
        Some(true),
        "a block starts on the first line, rendered"
    );
    assert_eq!(
        highlighter.doc_block_at(&text, 1),
        None,
        "not on a blank line"
    );
    assert_eq!(highlighter.doc_block_at(&text, 2), Some(true));
    assert_eq!(highlighter.doc_block_at(&text, 4), None, "nor in code");
    highlighter.toggle_doc_block(2);
    assert_eq!(highlighter.doc_block_at(&text, 2), Some(false));
    assert_eq!(
        highlighter.doc_block_at(&text, 0),
        Some(true),
        "the other block is untouched"
    );
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
    assert_eq!(
        highlighter.doc_block_at(&edited, 4),
        Some(false),
        "the block moved to line 4 and is still written"
    );
    assert_eq!(
        highlighter.doc_block_at(&edited, 2),
        Some(true),
        "and the crate docs, now on line 2, are rendered"
    );
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

/// A Rust text of about `lines` lines with strings, comments, docs, attributes and nesting in it.
fn program(lines: usize) -> String {
    let mut text = String::new();
    for number in 0..lines / 8 + 1 {
        text.push_str(&format!(
            "/// Doc line for item {number}, with `code`.\n#[derive(Debug)]\npub struct Item{number} {{\n    name: &'static str, // a comment {number}\n}}\n\nfn run_{number}(value: u32) -> u32 {{\n    let text = \"caf\u{e9} {number}\"; value + {number}\n}}\n"
        ));
    }
    text
}

/// A source of numbers that is the same on every run.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, below: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % below.max(1)
    }
}

/// What a query of the whole text gives, which the cache must always agree with.
fn expected(highlighter: &SyntaxHighlighter, text: &TextSnapshot) -> Vec<StyledRange> {
    highlighter.uncached_ranges(text, whole(text))
}

fn overlapping(ranges: &[StyledRange], window: &Range<usize>) -> Vec<StyledRange> {
    ranges
        .iter()
        .filter(|range| range.range.start < window.end && range.range.end > window.start)
        .cloned()
        .collect()
}

#[test]
fn the_highlight_kept_between_edits_is_the_one_a_new_query_gives_after_any_edit() {
    let pieces = [
        "\"", "'", "/*", "*/", "//", "///", "{", "}", "(", ")", "\n", " ", "x", "fn ", "r#\"", "#",
        "\\", "\u{e9}", "0", "::", "<", ">", ";",
    ];
    let mut random = Lcg(7);
    let mut source = program(40);
    let highlighter = highlighter();
    let first = TextSnapshot::from_text(&source);
    assert_eq!(
        highlighter.ranges(&first, whole(&first)),
        expected(&highlighter, &first)
    );

    for step in 0..300 {
        let boundaries: Vec<usize> = source
            .char_indices()
            .map(|(at, _)| at)
            .chain([source.len()])
            .collect();
        let at = boundaries[random.next(boundaries.len())];
        let what = if random.next(3) == 0 && at < source.len() {
            let end = boundaries[(boundaries
                .iter()
                .position(|b| *b == at)
                .unwrap()
                + 1
                + random.next(12))
            .min(boundaries.len() - 1)];
            source.replace_range(at..end, "");
            format!("delete {at}..{end}")
        } else {
            let piece = pieces[random.next(pieces.len())];
            source.insert_str(at, piece);
            format!("insert {piece:?} at {at}")
        };
        let text = TextSnapshot::from_text(&source);
        // Now and then only a window is asked for, as a scrolled field does, and the whole text later.
        if step % 5 == 2 {
            let start = boundaries[random.next(boundaries.len())].min(source.len());
            let window = start.min(source.len())..(start + 400).min(source.len());
            let window = window.start.min(window.end)..window.end;
            let kept = highlighter.ranges(&text, window.clone());
            let all = expected(&highlighter, &text);
            assert_eq!(
                kept,
                overlapping(&all, &window),
                "step {step}, a window {window:?} after {what}"
            );
        }
        let kept = highlighter.ranges(&text, whole(&text));
        let all = expected(&highlighter, &text);
        assert_eq!(
            kept, all,
            "step {step}: the kept highlight differs from a new query after {what}"
        );
    }
}

#[test]
fn an_edit_asks_tree_sitter_again_only_about_the_lines_it_touched() {
    let source = program(4000);
    let highlighter = highlighter();
    let text = TextSnapshot::from_text(&source);
    let _ = highlighter.ranges(&text, whole(&text));
    let first = highlighter.queried_bytes();
    assert!(
        first >= source.len() - 100,
        "the first request asks for all of it: {first} of {}",
        source.len()
    );

    let middle = source[..source.len() / 2]
        .rfind('\n')
        .expect("a line")
        + 1;
    let edited = text.with_replaced(middle..middle, "x");
    let _ = highlighter.ranges(&edited, whole(&edited));
    let asked = highlighter.queried_bytes() - first;
    assert!(
        asked < 1000,
        "one character asked about {asked} bytes of {}",
        source.len()
    );
    assert_eq!(
        highlighter.ranges(&edited, whole(&edited)),
        expected(&highlighter, &edited)
    );

    // The same text asked for again asks for nothing.
    let before = highlighter.queried_bytes();
    let _ = highlighter.ranges(&edited, whole(&edited));
    assert_eq!(highlighter.queried_bytes(), before, "nothing changed");
}

#[test]
fn scrolling_asks_only_about_the_lines_that_come_into_view() {
    let source = program(4000);
    let highlighter = highlighter();
    let text = TextSnapshot::from_text(&source);
    let _ = highlighter.ranges(&text, 10_000..14_000);
    let first = highlighter.queried_bytes();
    assert_eq!(first, 4000);
    let _ = highlighter.ranges(&text, 10_500..14_500);
    assert_eq!(
        highlighter.queried_bytes() - first,
        500,
        "the half-window that is new"
    );
    let _ = highlighter.ranges(&text, 9_800..13_000);
    assert_eq!(
        highlighter.queried_bytes() - first,
        500 + 200,
        "and the other way"
    );
    let _ = highlighter.ranges(&text, 11_000..12_000);
    assert_eq!(
        highlighter.queried_bytes() - first,
        700,
        "a window already held asks for nothing"
    );
    // A window far from what is held is a new start.
    assert_eq!(
        highlighter.ranges(&text, 100_000..100_500),
        overlapping(&expected(&highlighter, &text), &(100_000..100_500))
    );
}

#[test]
fn undoing_and_redoing_edits_keeps_the_highlight_right() {
    let mut source = program(60);
    let highlighter = highlighter();
    let mut history = vec![source.clone()];
    let first = TextSnapshot::from_text(&source);
    let _ = highlighter.ranges(&first, whole(&first));
    let mut random = Lcg(3);
    for _ in 0..40 {
        let at = (0..source.len())
            .filter(|at| source.is_char_boundary(*at))
            .nth(random.next(source.len() / 2))
            .unwrap_or(0);
        source.insert_str(at, ["\"", "/*", "x", "\n", "{"][random.next(5)]);
        history.push(source.clone());
        let text = TextSnapshot::from_text(&source);
        let _ = highlighter.ranges(&text, whole(&text));
    }
    // Holding undo goes back through all of them, one value at a time, and redo goes forward.
    for version in history.iter().rev().chain(history.iter()) {
        let text = TextSnapshot::from_text(version);
        assert_eq!(
            highlighter.ranges(&text, whole(&text)),
            expected(&highlighter, &text)
        );
    }
}

#[test]
fn typing_an_opening_comment_a_character_at_a_time_restyles_the_text_after_it() {
    let source = program(120);
    let at = source.find("fn run_3(").expect("a line") - 1;
    let highlighter = highlighter();
    let mut text = TextSnapshot::from_text(&source);
    let _ = highlighter.ranges(&text, whole(&text));
    for (n, typed) in ["/", "*"].into_iter().enumerate() {
        text = text.with_replaced(at + n..at + n, typed);
        let kept = highlighter.ranges(&text, whole(&text));
        let fresh = self::highlighter().ranges(&text, whole(&text));
        let after = source.find("fn run_9(").expect("a later line") + 2;
        assert_eq!(kept, fresh, "after typing {typed:?}");
        if n == 1 {
            assert_eq!(
                style_at(&kept, after).and_then(|style| style.color),
                Some(SyntaxPalette::dark().comment),
                "the code below an opened comment is in it"
            );
        }
    }
}

#[test]
fn the_highlight_follows_a_quote_typed_undone_and_a_comment_opened_after_it() {
    let source = program(120);
    let at = source.find("fn run_3(").expect("a line") - 1;
    let highlighter = highlighter();
    let original = TextSnapshot::from_text(&source);
    let _ = highlighter.ranges(&original, whole(&original));
    let after = source.find("fn run_9(").expect("a later line") + 2;
    let mut text = original.clone();
    let steps: [(&str, Box<dyn Fn(&TextSnapshot) -> TextSnapshot>); 4] = [
        (
            "type a quote",
            Box::new(move |text| text.with_replaced(at..at, "\"")),
        ),
        // Undo hands back the text as it was, made apart and sharing nothing with the one before.
        (
            "undo it",
            Box::new(move |_| TextSnapshot::from_text(&program(120))),
        ),
        (
            "type a slash",
            Box::new(move |text| text.with_replaced(at..at, "/")),
        ),
        (
            "type a star",
            Box::new(move |text| text.with_replaced(at + 1..at + 1, "*")),
        ),
    ];
    for (what, step) in steps {
        text = step(&text);
        let kept = highlighter.ranges(&text, whole(&text));
        let fresh = self::highlighter().ranges(&text, whole(&text));
        assert_eq!(kept, fresh, "after {what}");
        let query = highlighter.uncached_ranges(&text, whole(&text));
        assert_eq!(
            kept, query,
            "after {what}, against the same tree asked again"
        );
    }
    assert_eq!(
        style_at(&highlighter.ranges(&text, whole(&text)), after).and_then(|style| style.color),
        Some(SyntaxPalette::dark().comment)
    );
}

const LINKS: &str = "// call helper here\nfn helper() -> Point {\n    let text = \"helper\";\n    let raw = r#\"helper\"#;\n    let n = 42;\n    println!(\"{}\", helper());\n    Point { x: n }\n}\n";

#[test]
fn only_a_name_is_a_link_and_not_a_comment_a_string_a_raw_string_a_keyword_or_a_number() {
    let text = TextSnapshot::from_text(LINKS);
    let syntax = highlighter();
    let link = |needle: &str, nth: usize| {
        let at = LINKS
            .match_indices(needle)
            .nth(nth)
            .unwrap_or_else(|| panic!("{needle:?} #{nth} is in the text"))
            .0;
        syntax.link_at(&text, at)
    };
    assert_eq!(
        link("helper", 1),
        Some(LINKS.find("fn helper").unwrap() + 3..LINKS.find("fn helper").unwrap() + 9),
        "a function's name where it is declared"
    );
    assert_eq!(link("helper", 0), None, "a word of a comment");
    assert_eq!(link("helper", 2), None, "a word of a string");
    assert_eq!(link("helper", 3), None, "a word of a raw string");
    let call = LINKS.rfind("helper").unwrap();
    assert_eq!(link("helper", 4), Some(call..call + 6), "a call of it");
    assert_eq!(link("Point", 0).map(|range| range.len()), Some(5), "a type");
    assert_eq!(link("x:", 0).map(|range| range.len()), Some(1), "a field");
    assert_eq!(
        link("println", 0).map(|range| range.len()),
        Some(7),
        "a macro's name"
    );
    assert_eq!(link("let", 0), None, "a keyword");
    assert_eq!(link("fn", 0), None, "a keyword");
    assert_eq!(link("42", 0), None, "a number");
    assert_eq!(link("{}", 0), None, "a format string");
    assert_eq!(
        syntax.link_at(&text, LINKS.len()),
        None,
        "the end of the text"
    );
    assert_eq!(
        syntax.link_at(&text, LINKS.len() + 50),
        None,
        "past the end"
    );
}

#[test]
fn a_link_is_found_in_bytes_after_wide_characters_and_in_an_empty_or_unfinished_text() {
    let source = "let caf\u{e9} = 1; // \u{e9}\nlet \u{e9}t\u{e9} = caf\u{e9};";
    let text = TextSnapshot::from_text(source);
    let syntax = highlighter();
    let second = source.rfind("caf\u{e9}").unwrap();
    assert_eq!(
        syntax.link_at(&text, second),
        Some(second..second + 5),
        "a name with an accent is five bytes"
    );
    assert_eq!(
        syntax.link_at(&TextSnapshot::from_text(""), 0),
        None,
        "no text"
    );
    assert_eq!(
        syntax.link_at(&TextSnapshot::from_text("fn ("), 1),
        None,
        "text that does not parse"
    );
}

#[test]
fn a_new_palette_restyles_what_was_already_highlighted_and_keeps_the_parse() {
    let (dark, light) = (SyntaxPalette::dark(), SyntaxPalette::light());
    let text = TextSnapshot::from_text(SAMPLE);
    let highlighter = highlighter();
    let keyword = |highlighter: &SyntaxHighlighter| {
        let ranges = highlighter.ranges(&text, whole(&text));
        style_at(&ranges, offset_of(SAMPLE, "pub struct")).and_then(|style| style.color)
    };
    assert_eq!(keyword(&highlighter), Some(dark.keyword));
    let queried = highlighter.queried_bytes();

    // The same palette changes nothing, and does not throw away what was kept.
    highlighter.set_palette(dark);
    assert_eq!(keyword(&highlighter), Some(dark.keyword));
    assert_eq!(
        highlighter.queried_bytes(),
        queried,
        "the highlight that was kept is still used"
    );

    // Another does, in what was highlighted before it was set.
    highlighter.set_palette(light);
    assert_eq!(keyword(&highlighter), Some(light.keyword));
    let ranges = highlighter.ranges(&text, whole(&text));
    let comment = style_at(&ranges, offset_of(SAMPLE, "// done")).and_then(|style| style.color);
    assert_eq!(comment, Some(light.comment));
    assert_eq!(
        highlighter.incremental_parses(),
        0,
        "the text did not change, so nothing was parsed again"
    );
}

#[test]
fn a_bracket_is_the_color_of_how_deep_it_is_and_what_is_not_a_bracket_keeps_its_own() {
    let palette = SyntaxPalette::dark();
    let source = "fn f(a: (u8, u8)) { g([1]); }";
    let text = TextSnapshot::from_text(source);
    let ranges = highlighter().ranges(&text, whole(&text));
    let color_at = |at: usize| style_at(&ranges, at).and_then(|style| style.color);
    let nth = |needle: &str, nth: usize| {
        source
            .match_indices(needle)
            .nth(nth)
            .expect("it is there")
            .0
    };
    assert_eq!(
        color_at(nth("(", 0)),
        Some(palette.rainbow(0)),
        "the parameters"
    );
    assert_eq!(
        color_at(nth("(", 1)),
        Some(palette.rainbow(1)),
        "a tuple in them"
    );
    assert_eq!(
        color_at(nth(")", 0)),
        Some(palette.rainbow(1)),
        "its closer"
    );
    assert_eq!(color_at(nth(")", 1)), Some(palette.rainbow(0)));
    assert_eq!(color_at(nth("{", 0)), Some(palette.rainbow(0)));
    assert_eq!(
        color_at(nth("(", 2)),
        Some(palette.rainbow(1)),
        "the call, in the body"
    );
    assert_eq!(
        color_at(nth("[", 0)),
        Some(palette.rainbow(2)),
        "the array in it"
    );
    assert_eq!(
        color_at(nth("fn", 0)),
        Some(palette.keyword),
        "a keyword is as it was"
    );
    assert_eq!(color_at(nth("u8", 0)), Some(palette.types));
    assert_eq!(
        color_at(nth(";", 0)),
        Some(palette.punctuation),
        "and so is a mark that is not a bracket"
    );
}

#[test]
fn the_colors_come_round_after_the_last_and_the_text_that_is_not_a_bracket_is_not_one() {
    let palette = SyntaxPalette::dark();
    let source = format!("{}1{}", "(".repeat(8), ")".repeat(8));
    let text = TextSnapshot::from_text(&source);
    let ranges = highlighter().ranges(&text, whole(&text));
    for depth in 0..8 {
        assert_eq!(
            style_at(&ranges, depth).and_then(|style| style.color),
            Some(palette.rainbow(depth)),
            "depth {depth}"
        );
        assert_eq!(
            style_at(&ranges, source.len() - 1 - depth).and_then(|style| style.color),
            Some(palette.rainbow(depth)),
            "depth {depth}, closing"
        );
    }
    let quoted = TextSnapshot::from_text("let s = \"(\"; // (\n");
    let plain = highlighter().ranges(&quoted, whole(&quoted));
    assert!(
        plain
            .iter()
            .all(|run| run.style.color != Some(palette.rainbow(0))),
        "a string and a comment have none"
    );
}

#[test]
fn an_edit_that_wraps_code_in_a_bracket_makes_the_brackets_inside_it_one_deeper_in_the_whole_and_in_a_window()
 {
    let palette = SyntaxPalette::dark();
    let highlighter = highlighter();
    let before = "fn f() {\n    g((1));\n}\n";
    let first = TextSnapshot::from_text(before);
    let _ = highlighter.ranges(&first, whole(&first));
    // The function is put in a module, which the lines of the function are not touched by.
    let after = format!("mod m {{\n{before}}}\n");
    let text = TextSnapshot::from_text(&after);
    let at = after.find("((").expect("it is there");
    let all = highlighter.ranges(&text, whole(&text));
    let window = highlighter.ranges(&text, at..at + 2);
    for ranges in [&all, &window] {
        let color_at = |at: usize| style_at(ranges, at).and_then(|style| style.color);
        assert_eq!(
            color_at(at),
            Some(palette.rainbow(2)),
            "inside the module's braces and the function's, and the call's"
        );
        assert_eq!(color_at(at + 1), Some(palette.rainbow(3)));
    }
}

#[test]
fn the_pair_beside_the_caret_and_the_pair_around_it_follow_the_text_as_it_is_edited() {
    let highlighter = highlighter();
    let before = TextSnapshot::from_text("fn f() { g(1); }");
    let pair = highlighter
        .pair_beside(&before, 8)
        .expect("right after the brace");
    assert_eq!((pair.open, pair.close, pair.depth), (7..8, 15..16, 0));
    let inner = highlighter
        .pair_around(&before, 12)
        .expect("inside the call");
    assert_eq!((inner.open, inner.close, inner.depth), (10..11, 12..13, 1));
    // A line is added in the braces: the same pair is further on.
    let after = TextSnapshot::from_text("fn f() {\n    g(1); }");
    let moved = highlighter
        .pair_beside(&after, 8)
        .expect("right after the brace, which is at the end of its line");
    assert_eq!((moved.open, moved.close), (7..8, 19..20));
    assert_eq!(highlighter.pair_beside(&after, 12), None, "beside a letter");
    assert_eq!(
        highlighter.pair_beside(&TextSnapshot::from_text(""), 0),
        None,
        "the empty text"
    );
}

#[test]
fn a_line_of_code_that_is_shown_on_its_own_is_colored_as_it_is_in_a_file_and_leaves_the_parse_of_a_file_alone() {
    let palette = SyntaxPalette::dark();
    let snippets = highlighter();
    // The parse of a document, which colors for lines of other files must not disturb.
    let document = highlighter();
    let text = TextSnapshot::from_text(SAMPLE);
    let before = document.ranges(&text, whole(&text));

    let line = "let name = Point { x: 1 };";
    let ranges = snippets.snippet(line);
    let color_at = |needle: &str| style_at(&ranges, offset_of(line, needle)).and_then(|style| style.color);
    assert_eq!(color_at("let"), Some(palette.keyword));
    assert_eq!(color_at("1"), Some(palette.number));
    // A fragment, which is not a whole program, and one that is not code at all, do not fail.
    for fragment in [".child(x)", "} else {", "x: &'a str,", "\u{e9}\u{e9}", "\"unfinished"] {
        let ranges = snippets.snippet(fragment);
        assert!(
            ranges.iter().all(|range| range.range.end <= fragment.len() && fragment.is_char_boundary(range.range.start) && fragment.is_char_boundary(range.range.end)),
            "{fragment:?}: {ranges:?} are inside the text and between characters"
        );
    }
    assert!(snippets.snippet("").is_empty());
    assert_eq!(document.ranges(&text, whole(&text)), before, "what a file is colored with is as it was");
}
