use std::rc::Rc;

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

fn nobody() -> Rc<dyn Fn(usize)> {
    Rc::new(|_| {})
}

fn embeds_of(renderer: &DocRenderer, text: &str) -> Vec<(Range<usize>, u64)> {
    let snapshot = snapshot(text);
    renderer.embeds(&snapshot, 0..text.len(), &nobody()).iter().map(|embed| (embed.range(), embed.key())).collect()
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
    let in_window = |bytes: Range<usize>| renderer.embeds(&text, bytes, &nobody()).len();
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
fn typing_into_a_rendered_block_opens_it_once() {
    let text = snapshot(SAMPLE);
    let mut renderer = renderer();
    assert!(!renderer.open_at(&text, 6), "code is not a block");
    assert!(renderer.open_at(&text, 4), "a line in the middle of a block opens the block");
    assert!(!renderer.is_rendered(2), "by its first line");
    assert!(!renderer.open_at(&text, 3), "a block that is open has nothing more to open");
    assert!(renderer.is_rendered(8), "the others are untouched");
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
