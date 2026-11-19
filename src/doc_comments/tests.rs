use aimer::MarkdownTheme;
use aimer::input::input::TextSnapshot;

use super::*;

const SAMPLE: &str = "use std::fmt;\n\n/// A **bold** claim,\n/// and `code`.\n///\n/// # Heading\nfn one() {}\n\n//! inner\n//! doc\n\n    /// indented\n    fn two() {}\n//// not doc\n";

fn snapshot(text: &str) -> TextSnapshot {
    TextSnapshot::from_text(text)
}

fn renderer() -> DocRenderer {
    DocRenderer::new(MarkdownTheme::default())
}

fn embeds_of(renderer: &DocRenderer, text: &str) -> Vec<(Range<usize>, u64)> {
    let snapshot = snapshot(text);
    renderer.embeds(&snapshot, 0..text.len()).iter().map(|embed| (embed.range(), embed.key())).collect()
}

fn at(text: &str, needle: &str) -> usize {
    text.find(needle).unwrap_or_else(|| panic!("{needle:?} in the sample"))
}

#[test]
fn a_prefix_is_the_indent_the_marker_and_one_space() {
    let prefix = |line: &str| prefix_of(line).map(|prefix| (prefix.marker, prefix.indent, prefix.text));
    assert_eq!(prefix("/// text"), Some((Marker::Outer, 0, 4)));
    assert_eq!(prefix("///text"), Some((Marker::Outer, 0, 3)));
    assert_eq!(prefix("    /// indented"), Some((Marker::Outer, 4, 8)));
    assert_eq!(prefix("//! inner"), Some((Marker::Inner, 0, 4)));
    assert_eq!(prefix("///"), Some((Marker::Outer, 0, 3)), "an empty doc line is one");
    assert_eq!(prefix("//// four slashes"), None);
    assert_eq!(prefix("// plain"), None);
    assert_eq!(prefix("let a = 1; /// after code"), None, "a marker after code is not a doc line");
}

#[test]
fn a_block_is_a_run_of_one_kind_of_doc_line() {
    let text = snapshot(SAMPLE);
    assert_eq!(block_at(&text, 2), Some(DocBlock { first_line: 2, last_line: 5 }));
    assert_eq!(block_at(&text, 5), Some(DocBlock { first_line: 2, last_line: 5 }));
    assert_eq!(block_at(&text, 6), None, "code ends it");
    assert_eq!(block_at(&text, 8), Some(DocBlock { first_line: 8, last_line: 9 }), "inner docs are a block of their own");
    assert_eq!(block_at(&text, 11), Some(DocBlock { first_line: 11, last_line: 11 }), "an indented one too");
    assert_eq!(block_at(&text, 13), None, "four slashes are a plain comment");
    assert!(is_block_start(&text, 2) && !is_block_start(&text, 3) && is_block_start(&text, 8) && is_block_start(&text, 11));
    assert!(!is_block_start(&text, 6) && !is_block_start(&text, 999));
    // An outer block directly followed by an inner one are two blocks.
    let mixed = snapshot("/// a\n//! b\n");
    assert_eq!(block_at(&mixed, 0), Some(DocBlock { first_line: 0, last_line: 0 }));
    assert_eq!(block_at(&mixed, 1), Some(DocBlock { first_line: 1, last_line: 1 }));
}

#[test]
fn the_blocks_of_a_window_are_those_with_a_line_in_it() {
    let text = snapshot(SAMPLE);
    let line = |number: usize| text.line_range(number);
    assert_eq!(blocks_in(&text, line(3).start..line(3).end).len(), 1, "a window inside a block holds the whole block");
    assert_eq!(blocks_in(&text, 0..text.len()).len(), 3);
    assert_eq!(blocks_in(&text, line(6).start..line(7).end).len(), 0);
    assert!(blocks_in(&snapshot(""), 0..0).is_empty());
}

#[test]
fn a_rendered_block_is_one_embed_over_all_its_lines_and_nothing_else_is() {
    let embeds = embeds_of(&renderer(), SAMPLE);
    assert_eq!(embeds.len(), 3, "the outer block, the inner one and the indented one: {embeds:?}");
    // From the start of the first line to the end of the last, without the line break.
    let first = at(SAMPLE, "/// A **bold**")..at(SAMPLE, "/// # Heading") + "/// # Heading".len();
    assert_eq!(embeds[0].0, first);
    assert_eq!(embeds[1].0, at(SAMPLE, "//! inner")..at(SAMPLE, "//! doc") + "//! doc".len());
    assert_eq!(embeds[2].0, at(SAMPLE, "    /// indented")..at(SAMPLE, "    /// indented") + "    /// indented".len(), "an indented comment starts at its indent");
    assert!(!embeds.iter().any(|(range, _)| range.contains(&at(SAMPLE, "fn one"))), "code is not embedded");
    assert!(!embeds.iter().any(|(range, _)| range.contains(&at(SAMPLE, "//// not"))), "nor is a plain comment");
}

#[test]
fn only_the_blocks_with_a_line_in_the_window_are_embedded() {
    let text = snapshot(SAMPLE);
    let line = |number: usize| text.line_range(number);
    let renderer = renderer();
    let in_window = |bytes: Range<usize>| renderer.embeds(&text, bytes).len();
    assert_eq!(in_window(line(3).start..line(3).end), 1, "the window is inside a block: the whole block");
    assert_eq!(in_window(line(6).start..line(7).end), 0);
    assert_eq!(in_window(0..text.len()), 3);
}

#[test]
fn a_block_shown_as_written_is_not_embedded_and_toggles_back() {
    let mut renderer = renderer();
    assert!(renderer.is_rendered(2));
    renderer.toggle(2);
    assert!(!renderer.is_rendered(2));
    let embeds = embeds_of(&renderer, SAMPLE);
    assert_eq!(embeds.len(), 2);
    assert!(embeds.iter().all(|(range, _)| range.start >= at(SAMPLE, "//! inner")), "{embeds:?}");
    renderer.toggle(2);
    assert!(renderer.is_rendered(2));
    assert_eq!(embeds_of(&renderer, SAMPLE).len(), 3);
}

#[test]
fn what_an_embed_shows_is_the_markdown_of_its_lines_and_its_key_follows_it() {
    let text = snapshot(SAMPLE);
    let block = block_at(&text, 2).expect("a block");
    let (markdown, indent) = markdown_of(&text, &block);
    assert_eq!(markdown, "A **bold** claim,\nand `code`.\n\n# Heading");
    assert_eq!(indent, 0);
    let indented = markdown_of(&text, &block_at(&text, 11).expect("a block"));
    assert_eq!(indented, ("indented".to_owned(), 4));

    // Editing a block gives it a new key (a new widget); the others keep theirs.
    let before = embeds_of(&renderer(), SAMPLE);
    let edited = SAMPLE.replacen("claim", "claims", 1);
    let after = embeds_of(&renderer(), &edited);
    assert_ne!(before[0].1, after[0].1);
    assert_eq!((before[1].1, before[2].1), (after[1].1, after[2].1));
    // Identical blocks are told apart, and so are the same block on another line.
    let twice = embeds_of(&renderer(), "/// same\nfn a() {}\n/// same\nfn b() {}\n");
    assert_ne!(twice[0].1, twice[1].1);
}

#[test]
fn the_markers_are_taken_off_whatever_the_line_ending_or_the_characters() {
    let source = "/// caf\u{e9} **bold** \u{4f60}\u{597d}\r\n///\r\n/// end\r\nfn x() {}\r\n";
    let text = snapshot(source);
    let block = block_at(&text, 0).expect("a block");
    assert_eq!(markdown_of(&text, &block).0, "caf\u{e9} **bold** \u{4f60}\u{597d}\n\nend");
    let embeds = embeds_of(&renderer(), source);
    assert_eq!(embeds.len(), 1);
    assert_eq!(embeds[0].0, 0..source.find("end").unwrap() + 3, "ends before the line break, not after the \\r");
    assert!(source.is_char_boundary(embeds[0].0.end));
}

#[test]
fn only_the_first_line_of_a_rendered_block_has_a_bar_at_its_indent() {
    let text = snapshot(SAMPLE);
    let mut renderer = renderer();
    assert_eq!(renderer.bar_indent(&text, 2), Some(0));
    assert_eq!(renderer.bar_indent(&text, 4), Some(0), "any line of the block asks for the block's bar");
    assert_eq!(renderer.bar_indent(&text, 11), Some(4), "an indented comment's bar is at its indent");
    assert_eq!(renderer.bar_indent(&text, 6), None, "code has none");
    renderer.toggle(2);
    assert_eq!(renderer.bar_indent(&text, 2), None, "a block shown as written has none");
}

#[test]
fn the_blocks_shown_as_written_keep_their_lines_as_the_text_is_edited() {
    let mut renderer = renderer();
    renderer.toggle(10);
    renderer.toggle(2);
    // Two lines are added above both: they move down by two.
    renderer.edited(0, 0, 2);
    assert!(!renderer.is_rendered(12) && !renderer.is_rendered(4));
    assert!(renderer.is_rendered(10) && renderer.is_rendered(2));
    // A line is taken out between them.
    renderer.edited(6, 1, 0);
    assert!(!renderer.is_rendered(11) && !renderer.is_rendered(4));
    // The block's own first line is deleted.
    renderer.edited(3, 1, 0);
    assert!(renderer.is_rendered(4), "a block that is gone is forgotten, so what is on its line is rendered");
    assert!(!renderer.is_rendered(10), "and the one below it moved up a line");
    // An edit that changes no line count moves nothing.
    renderer.edited(0, 3, 3);
    assert!(!renderer.is_rendered(10));
}

#[test]
fn a_new_line_in_a_doc_comment_goes_on_with_the_same_marker_and_indent() {
    assert_eq!(continuation("/// text").as_deref(), Some("/// "));
    assert_eq!(continuation("    /// indented").as_deref(), Some("    /// "));
    assert_eq!(continuation("//! inner").as_deref(), Some("//! "));
    assert_eq!(continuation("///").as_deref(), Some("/// "), "an empty doc line goes on too");
    assert_eq!(continuation("\t/// tab").as_deref(), Some("\t/// "));
    assert_eq!(continuation("//// four slashes"), None);
    assert_eq!(continuation("// plain"), None);
    assert_eq!(continuation("let a = 1;"), None);
    assert_eq!(continuation("//"), None, "a caret inside the marker has not made a comment yet");
}

/// Whether removing `removed` from `text` (an empty range is an insertion at that point) is refused by
/// a renderer that shows every block rendered.
fn refused(text: &str, removed: Range<usize>) -> bool {
    renderer().protects(&snapshot(text), removed)
}

#[test]
fn an_edit_inside_a_rendered_block_is_refused_wherever_in_it_the_caret_is() {
    let line = |number: usize| snapshot(SAMPLE).line_range(number);
    // The start, the middle and the end of a line of the block, the first and the last of its lines.
    for point in [line(2).start, line(3).start + 4, line(3).end - 1, line(5).start, line(5).end - 1] {
        assert!(refused(SAMPLE, point..point), "typing at {point}");
    }
    assert!(refused(SAMPLE, line(2).start..line(2).start + 1), "deleting the first character of the block");
    assert!(refused(SAMPLE, line(4).start + 3..line(4).start + 4), "deleting inside an empty `///` line");
    assert!(refused(SAMPLE, line(11).start..line(11).start), "an indented block is one too");
}

#[test]
fn the_lines_around_a_rendered_block_stay_editable() {
    let text = snapshot(SAMPLE);
    let line = |number: usize| text.line_range(number);
    assert!(!refused(SAMPLE, line(1).start..line(1).start), "the empty line above it");
    assert!(!refused(SAMPLE, line(0).end - 1..line(0).end - 1), "the end of the code above it");
    assert!(!refused(SAMPLE, line(6).start..line(6).start), "the start of the code below it");
    assert!(!refused(SAMPLE, line(6).start + 3..line(6).start + 4), "a character of the code below it");
    assert!(!refused(SAMPLE, line(6).end..line(6).end), "the end of the code below it");
    assert!(!refused(SAMPLE, 0..0), "the start of the file");
    assert!(!refused(SAMPLE, line(0).start..line(1).start), "a whole line of code with its line break");
}

#[test]
fn a_line_break_at_the_edge_of_a_block_is_not_deleted_when_that_changes_the_block() {
    // Delete at the end of the code above: the block's first line would join it.
    let above = "let a = 1;\n/// doc\nfn x() {}\n";
    assert!(refused(above, 10..11), "the line break before the block, with code before it on the line");
    assert!(!refused("\n/// doc\nfn x() {}\n", 0..1), "an empty line above it joins nothing: the block is as it was");
    // Backspace at the start of the code below: it would join the block's last line.
    assert!(refused(above, 18..19), "the line break after the block");
    // A selection that ends where the block begins takes that line break too.
    assert!(refused(above, 4..11), "from the middle of the code above to the block");
    assert!(!refused(above, 0..11), "a whole line above it, with its line break, leaves the block alone");
    assert!(!refused(above, 0..4), "part of the code above it");
}

#[test]
fn a_selection_that_reaches_into_a_rendered_block_is_refused_and_one_that_misses_it_is_not() {
    let text = snapshot(SAMPLE);
    let line = |number: usize| text.line_range(number);
    assert!(refused(SAMPLE, 0..SAMPLE.len()), "select all");
    assert!(refused(SAMPLE, line(0).start..line(3).start + 2), "from the code above into it");
    assert!(refused(SAMPLE, line(4).start..line(7).end), "from inside it into the code below");
    assert!(!refused(SAMPLE, line(6).start..line(7).end), "code between two blocks");
    assert!(!refused(SAMPLE, line(12).start..line(13).end), "code after the last one");
}

#[test]
fn a_block_shown_as_written_is_edited_like_any_text() {
    let mut renderer = renderer();
    let text = snapshot(SAMPLE);
    let inside = text.line_range(3).start + 2;
    assert!(renderer.protects(&text, inside..inside));
    renderer.toggle(2);
    assert!(!renderer.protects(&text, inside..inside), "once it is open");
    assert!(renderer.protects(&text, text.line_range(8).start..text.line_range(8).start), "the other blocks are still closed");
    renderer.toggle(2);
    assert!(renderer.protects(&text, inside..inside), "and closed again");
}

#[test]
fn the_boundaries_of_the_text_do_not_trip_the_guard() {
    assert!(!refused("", 0..0), "empty text");
    assert!(!refused("fn x() {}", 0..0), "a text of one line that is code");
    assert!(refused("/// doc", 7..7), "a block with no final line break, at its end");
    assert!(refused("/// doc\nfn x() {}", 0..0), "a block at the very start of the file");
    assert!(!refused("/// doc\nfn x() {}", 8..8), "the code after it");
    assert!(!refused("fn x() {}", 50..60), "a range past the end is clamped, not a panic");
    assert!(refused("/// doc", 3..900), "and one that begins inside a block still is refused");
}

#[test]
fn crlf_and_multibyte_text_are_measured_in_bytes_of_the_line_breaks_and_characters() {
    let crlf = "a\r\n/// doc\r\nb\r\n";
    assert!(refused(crlf, 1..3), "the CRLF before a block with code before it");
    assert!(!refused(crlf, 0..3), "the whole line above it with its CRLF");
    assert!(refused(crlf, 10..12), "the CRLF after the block");
    assert!(!refused(crlf, 12..12), "the start of the code below");
    // "é" is two bytes, so the line break is at byte 2.
    let accents = "é\n/// d\u{e9}\nz\n";
    assert!(refused(accents, 2..3), "the line break after a two-byte character");
    assert!(!refused(accents, 0..3), "the whole line");
    assert!(refused(accents, 6..6), "inside the block, after a two-byte character");
    assert!(!refused(accents, 13..13), "the code after it");
}

/// Whether Backspace (`backwards`) or Delete with the selection `selection` is refused.
fn key_refused(text: &str, selection: Range<usize>, backwards: bool) -> bool {
    renderer().refuses_deletion(&snapshot(text), selection, backwards)
}

#[test]
fn backspace_and_delete_are_refused_where_they_would_join_code_to_a_rendered_block() {
    let text = "let a = 1;\n/// doc\nfn x() {}\n";
    // Delete at the end of the code above, and Backspace at the start of the code below.
    assert!(key_refused(text, 10..10, false), "Delete before the block's first line");
    assert!(key_refused(text, 19..19, true), "Backspace after the block's last line");
    // Mid-line they are ordinary.
    assert!(!key_refused(text, 5..5, true) && !key_refused(text, 5..5, false), "in the code above");
    assert!(!key_refused(text, 23..23, true) && !key_refused(text, 23..23, false), "in the code below");
    // Inside the block, Backspace and Delete are refused whichever way they go.
    assert!(key_refused(text, 14..14, true) && key_refused(text, 14..14, false));
    assert!(key_refused(text, 11..11, true), "Backspace at the start of the block's first line takes the break before it");
    // Backspace at the start of the code above and Delete at the end of the code below go nowhere.
    assert!(!key_refused(text, 0..0, true), "Backspace at the start of the file");
    assert!(!key_refused(text, text.len()..text.len(), false), "Delete at the end of the file");
}

#[test]
fn a_selection_is_what_a_deletion_key_removes_when_there_is_one() {
    let text = "let a = 1;\n/// doc\nfn x() {}\n";
    assert!(key_refused(text, 4..14, true) && key_refused(text, 4..14, false), "a selection that reaches into the block");
    assert!(!key_refused(text, 0..4, true) && !key_refused(text, 0..4, false), "one that does not");
}

#[test]
fn a_line_break_is_removed_whole_by_a_deletion_key_whether_it_is_lf_or_crlf() {
    let text = "a\r\n/// doc\r\nb\r\n";
    assert!(key_refused(text, 3..3, true), "Backspace at the start of the block takes the CRLF before it");
    assert!(key_refused(text, 1..1, false), "Delete before the CRLF takes it, and joins the block to `a`");
    assert!(key_refused(text, 12..12, true), "Backspace at the start of `b` takes the CRLF after the block");
    assert!(!key_refused("\r\n/// doc\r\nb", 0..0, false), "an empty line above joins nothing");
    let accents = "\u{e9}\n/// d\n";
    assert!(key_refused(accents, 2..2, false), "Delete after a two-byte character, before the break");
    assert!(!key_refused("x\u{e9}\ny", 1..1, true), "Backspace over a two-byte character in code");
}

/// The blocks of a window as the lines alone say them: each line asked for its block, the way
/// `blocks_in` used to find them. What a faster way of finding them must agree with.
fn blocks_by_lines(text: &TextSnapshot, bytes: Range<usize>) -> Vec<DocBlock> {
    if text.len() == 0 {
        return Vec::new();
    }
    let first = text.byte_to_line(bytes.start.min(text.len()));
    let last = text.byte_to_line(bytes.end.max(bytes.start).saturating_sub(1).min(text.len()));
    let mut blocks = Vec::new();
    let mut number = first;
    while number <= last {
        match block_at(text, number) {
            Some(block) => {
                number = block.last_line + 1;
                blocks.push(block);
            }
            None => number += 1,
        }
    }
    blocks
}

/// A text with every kind of line a scan has to tell apart, `copies` times over.
fn varied(copies: usize) -> String {
    let unit = "/// outer one\n/// outer two\nfn a() {}\n\n    /// indented\n    /// indented two\n//! inner\n//// four slashes\n// plain\n///\n///no space\n\t/// tab\nlet x = 1; /// after code\n//!\n/// caf\u{e9} \u{4f60}\u{597d}\n";
    unit.repeat(copies)
}

#[test]
fn the_kind_of_each_line_is_found_in_one_pass_as_the_line_alone_says_it() {
    for source in [
        varied(1),
        varied(40),
        varied(400),
        varied(3).replace('\n', "\r\n"),
        "no final break /// ".to_owned() + "\n///",
        "///".to_owned(),
        "\n\n\n".to_owned(),
        format!("{}/// after a very long indent\n{}", " ".repeat(5000), "x".repeat(9000) + "\n///"),
    ] {
        let text = snapshot(&source);
        let last = text.line_count() - 1;
        for (first, last) in [(0, last), (last / 2, last), (0, last / 3), (last / 3, (last / 3 + 5).min(last)), (last, last)] {
            let found = markers_of(&text, first, last);
            let by_line: Vec<_> = (first..=last).map(|number| marker_of(&text, number)).collect();
            assert_eq!(found, by_line, "lines {first}..={last} of a text of {} lines and {} chunks", text.line_count(), text.chunk_count());
        }
    }
    assert!(snapshot(&varied(400)).chunk_count() > 3, "the long text is held in several chunks, so lines cross them");
}

#[test]
fn the_blocks_of_a_window_are_the_ones_the_lines_give_wherever_the_window_is() {
    let long_run = "/// a line\n".repeat(700) + "fn after() {}\n" + &"//! inner\n".repeat(5);
    // The oracle asks each line for its block, which for a run of 700 lines is a long way each: a
    // run that long is tried with windows of a few lines, one inside it and one where it ends.
    let text = snapshot(&long_run);
    for window in [3000..3600, 7700..8100] {
        assert_eq!(blocks_in(&text, window.clone()), blocks_by_lines(&text, window.clone()), "a run too long for a block, window {window:?}");
    }
    for source in [varied(1), varied(25), varied(60), varied(4).replace('\n', "\r\n"), SAMPLE.to_owned(), String::new(), "/// one".to_owned()] {
        let text = snapshot(&source);
        let len = text.len();
        let mut windows = vec![0..len, 0..0, len..len, 0..1, len.saturating_sub(1)..len];
        for step in 0..12 {
            let start = len * step / 12;
            windows.push(start..(start + 300).min(len));
            windows.push(start..(start + len / 5).min(len));
        }
        for window in windows {
            assert_eq!(blocks_in(&text, window.clone()), blocks_by_lines(&text, window.clone()), "window {window:?} of a text of {len} bytes");
        }
    }
}
