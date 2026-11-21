//! The links of a doc comment: what rustdoc writes as ``[`Name`]``, a name in code font between brackets.
//!
//! Markdown has no target for such a name, so it would stay three pieces (a bracket, a code span, a bracket). So a
//! block is scanned before it is parsed: each link is given a target of its own, `(doc-link:N)` with the number of the
//! link, which makes it a link the viewer draws as one (a code chip, underlined) inside the line it is in, and the link is
//! kept with the place of its name, which is where the language server is asked what it names. A click on it arrives
//! with that target, which [`DocLink::index_of`] reads.

use std::fmt::Write;
use std::ops::Range;

/// A link of a doc comment: the name in it, and where the name is written.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DocLink {
    /// The name between the backticks, as written: `DocRenderer::protects`.
    pub label: String,
    /// The line of the text the name is on, counted from 0.
    pub line: usize,
    /// The byte of that line the name starts at.
    pub column: usize,
}

impl DocLink {
    /// What the target of a link starts with, followed by the number of the link.
    pub const TARGET_PREFIX: &'static str = "doc-link:";

    /// The number of the link a target names, from what follows [`TARGET_PREFIX`](Self::TARGET_PREFIX).
    pub fn index_of(rest: &str) -> Option<usize> {
        rest.parse().ok()
    }
}

/// Takes the links out of the lines of a block, one line after the other, in order. A fenced code block and a code
/// span are not scanned: what is written in code is code, and a reference that has a target of its own (`[`a`](url)`,
/// `[`a`][b]`, or a definition `[`a`]: url`) is the markdown's.
#[derive(Default)]
pub struct LinkScanner {
    /// The fence the scan is inside of: its character and how many of it opened it.
    fence: Option<(u8, usize)>,
    links: Vec<DocLink>,
}

impl LinkScanner {
    /// A scanner that has seen no line.
    pub fn new() -> Self {
        Self::default()
    }

    /// Scans `text`, a line of the block with its marker taken off and no line break, which is the line number `line` of
    /// the text and whose first byte is the byte `column` of it. Writes the line to `out`, each link followed by its target.
    pub fn scan_line(&mut self, text: &str, line: usize, column: usize, out: &mut String) {
        if self.in_fence(text) {
            out.push_str(text);
            return;
        }
        let bytes = text.as_bytes();
        // Everything before `copied` is written to `out`; the scan only moves on over ASCII bytes, so both are on
        // the boundary of a character whenever they are used to cut the text.
        let (mut at, mut copied) = (0, 0);
        while at < bytes.len() {
            match bytes[at] {
                b'\\' => at += 2,
                b'`' => {
                    let open = run_of(bytes, at);
                    at = closing_run(bytes, at + open, open).unwrap_or(at + open);
                }
                b'[' if bytes.get(at + 1) == Some(&b'`') && !(at > 0 && bytes[at - 1] == b'!') => {
                    let Some((name, end)) = link_at(text, at) else {
                        at += 1;
                        continue;
                    };
                    out.push_str(&text[copied..end]);
                    let _ = write!(out, "({}{})", DocLink::TARGET_PREFIX, self.links.len());
                    self.links.push(DocLink {
                        label: text[name.clone()].to_owned(),
                        line,
                        column: column + name.start,
                    });
                    copied = end;
                    at = end;
                }
                _ => at += 1,
            }
        }
        out.push_str(&text[copied.min(text.len())..]);
    }

    /// Whether `text` is inside a fenced code block or is the line that opens or closes one, which is not scanned.
    fn in_fence(&mut self, text: &str) -> bool {
        let trimmed = text.trim_start();
        let bytes = trimmed.as_bytes();
        // The run of fence characters the line starts with, and which character it is.
        let (character, run) = (bytes.first().copied().unwrap_or(0), run_of(bytes, 0));
        if let Some((open, length)) = self.fence {
            // A fence is closed by a line of as many of its character, or more, and nothing else.
            if character == open && run >= length && trimmed[run..].trim().is_empty() {
                self.fence = None;
            }
            return true;
        }
        if matches!(character, b'`' | b'~') && run >= 3 {
            self.fence = Some((character, run));
            return true;
        }
        false
    }

    /// The links found so far, numbered as their targets say.
    pub fn into_links(self) -> Vec<DocLink> {
        self.links
    }
}

/// How many of the byte at `at` there are in a row from there.
fn run_of(bytes: &[u8], at: usize) -> usize {
    let Some(&byte) = bytes.get(at) else {
        return 0;
    };
    bytes[at..]
        .iter()
        .take_while(|&&next| next == byte)
        .count()
}

/// Where the code span opened by `length` backticks, that start right before `from`, ends (after its closing backticks): at
/// the next run of exactly as many.
fn closing_run(bytes: &[u8], from: usize, length: usize) -> Option<usize> {
    let mut at = from;
    while at < bytes.len() {
        if bytes[at] == b'`' {
            let run = run_of(bytes, at);
            if run == length {
                return Some(at + run);
            }
            at += run;
        } else {
            at += 1;
        }
    }
    None
}

/// The link that starts with the bracket at `open` of `text`, as the bytes of its name and where it ends (after its closing
/// bracket), when what is there is a plain link: a name in one pair of backticks and a bracket, with no target after it.
fn link_at(text: &str, open: usize) -> Option<(Range<usize>, usize)> {
    let bytes = text.as_bytes();
    let start = open + 2;
    let close = start
        + bytes[start..]
            .iter()
            .position(|&byte| byte == b'`')?;
    if text[start..close].trim().is_empty() || bytes.get(close + 1) != Some(&b']') {
        return None;
    }
    let end = close + 2;
    match bytes.get(end) {
        // A target, or a reference to one.
        Some(b'(' | b'[') => None,
        // A definition, when it stands at the start of the line; in a sentence it is a colon.
        Some(b':') if text[..open].trim().is_empty() => None,
        _ => Some((start..close, end)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lines, one after the other as a block's are, and what they come to with the links found.
    fn scan(lines: &[&str]) -> (String, Vec<DocLink>) {
        let mut scanner = LinkScanner::new();
        let mut out = String::new();
        for (number, line) in lines.iter().enumerate() {
            if number > 0 {
                out.push('\n');
            }
            scanner.scan_line(line, 10 + number, 4, &mut out);
        }
        (out, scanner.into_links())
    }

    /// A link as it is written to the markdown: the name between its brackets and the target that numbers it.
    fn linked(label: &str, index: usize) -> String {
        format!("[`{label}`]({}{index})", DocLink::TARGET_PREFIX)
    }

    #[test]
    fn a_name_in_code_font_between_brackets_is_a_link_with_the_place_of_its_name() {
        let (out, links) = scan(&["Says which [`DocRenderer::protects`] refuses."]);
        assert_eq!(
            out,
            format!("Says which {} refuses.", linked("DocRenderer::protects", 0))
        );
        assert_eq!(
            links,
            [DocLink {
                label: "DocRenderer::protects".into(),
                line: 10,
                column: 4 + "Says which [`".len()
            }]
        );
    }

    #[test]
    fn links_are_numbered_in_order_across_lines() {
        let (out, links) = scan(&["[`a`] and [`b`]", "then [`c`]"]);
        assert_eq!(
            out,
            format!(
                "{} and {}\nthen {}",
                linked("a", 0),
                linked("b", 1),
                linked("c", 2)
            )
        );
        assert_eq!(
            links
                .iter()
                .map(|link| (link.label.as_str(), link.line, link.column))
                .collect::<Vec<_>>(),
            [("a", 10, 6), ("b", 10, 4 + 12), ("c", 11, 4 + 7)]
        );
    }

    #[test]
    fn a_column_is_in_bytes_after_multi_byte_text() {
        let (out, links) = scan(&["caf\u{e9} \u{4f60}\u{597d} [`x`]"]);
        assert_eq!(
            out,
            format!("caf\u{e9} \u{4f60}\u{597d} {}", linked("x", 0))
        );
        assert_eq!(links[0].column, 4 + "caf\u{e9} \u{4f60}\u{597d} [`".len());
    }

    #[test]
    fn a_name_that_is_not_a_plain_link_is_left_as_written() {
        for line in [
            "[`a`](https://example.com) has a target",
            "[`a`][b] has a reference",
            "![`a`] is an image",
            "\\[`a`] is escaped",
            "[``] names nothing",
            "[` `] names nothing",
            "[`a] is not closed",
            "[`a`]: https://example.com is a definition",
            "[a] is not in code font",
            "`[`a`]` is in a code span",
            "``[`a`]`` is in a code span",
        ] {
            let (out, links) = scan(&[line]);
            assert_eq!(out, line);
            assert!(links.is_empty(), "{line:?} gave {links:?}");
        }
    }

    #[test]
    fn a_colon_after_a_link_is_not_a_definition_when_the_link_is_in_a_sentence() {
        let (out, links) = scan(&["See [`a`]: it says"]);
        assert_eq!(out, format!("See {}: it says", linked("a", 0)));
        assert_eq!(links.len(), 1);
    }

    #[test]
    fn a_fenced_code_block_is_not_scanned_and_the_text_after_it_is() {
        let lines = [
            "[`a`]",
            "```rust",
            "let x = [`b`];",
            "```",
            "[`c`]",
            "~~~~",
            "[`d`]",
            "~~~",
            "[`e`]",
            "~~~~",
            "[`f`]",
        ];
        let (out, links) = scan(&lines);
        assert_eq!(
            out,
            [
                linked("a", 0).as_str(),
                "```rust",
                "let x = [`b`];",
                "```",
                linked("c", 1).as_str(),
                "~~~~",
                "[`d`]",
                "~~~",
                "[`e`]",
                "~~~~",
                linked("f", 2).as_str()
            ]
            .join("\n")
        );
        assert_eq!(
            links
                .iter()
                .map(|link| link.label.as_str())
                .collect::<Vec<_>>(),
            ["a", "c", "f"]
        );
    }

    #[test]
    fn an_empty_line_and_a_line_without_links_are_copied() {
        assert_eq!(scan(&[""]).0, "");
        assert_eq!(
            scan(&["plain `code` text", "", "more"]).0,
            "plain `code` text\n\nmore"
        );
    }

    #[test]
    fn a_very_long_line_is_scanned_once_through() {
        let line = format!(
            "{} [`a`] {}",
            "word ".repeat(20_000),
            "tail ".repeat(20_000)
        );
        let (out, links) = scan(&[&line]);
        assert_eq!(links.len(), 1);
        assert_eq!(out.len(), line.len() + "(doc-link:0)".len());
    }

    #[test]
    fn a_target_names_its_link_by_number() {
        assert_eq!(DocLink::index_of("12"), Some(12));
        assert_eq!(DocLink::index_of("x"), None);
        assert_eq!(DocLink::index_of(""), None);
    }
}
