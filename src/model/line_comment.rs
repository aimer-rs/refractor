//! Cmd+/ (Ctrl+/ elsewhere): comments the lines the selection touches, or the line the caret is on, with a marker at the indent they
//! share, or takes the marker away where every line that has text has one.
//!
//! Like [`AutoIndent`], the rules read only the lines they are at, and what is done is one edit through the controller, which one
//! undo takes back.

use std::ops::Range;

use aimer::input::input::{TextEditingController, TextSnapshot};

use crate::model::auto_indent::{AutoIndent, Shifted};

/// The rules of commenting lines.
pub struct LineComment;

impl LineComment {
    /// The marker of a line comment in Rust.
    pub const RUST: &'static str = "//";
    /// What begins a comment in TOML.
    pub const TOML: &'static str = "#";
    /// The key: the command key on macOS and the control key elsewhere, with a slash.
    pub const SHORTCUT: &'static str = "CmdOrCtrl+/";
    /// The most bytes of a line's indent that are read to find what it shares with the others.
    const MAX_INDENT: usize = 256;

    /// What the shortcut does with the selection `anchor` to `focus` in `text`, where `marker` begins a line comment: the lines that
    /// have text are all commented, at the indent that they share, with the marker and a space, or all uncommented, where each
    /// is commented, with the marker and the space after it taken away. Empty lines and lines of blanks are left as they are, and do
    /// not count. `None` where no line has text.
    ///
    /// A doc comment is not a comment to take away: `///` and `//!` are text, and are commented like any, so that taking that comment
    /// away leaves them as they were.
    pub fn toggle(
        text: &TextSnapshot,
        anchor: usize,
        focus: usize,
        marker: &str,
    ) -> Option<Shifted> {
        let (start, end) = (anchor.min(focus), anchor.max(focus));
        // Each line that has text: where its text starts, and how many bytes of marker and space it begins with, if it is commented.
        let lines: Vec<(Range<usize>, usize, Option<usize>)> =
            AutoIndent::touched_lines(text, start, end)
                .into_iter()
                .filter_map(|line| {
                    let indent = Self::indent_of(text, &line)?;
                    let commented = Self::marker_at(text, line.start + indent, line.end, marker);
                    Some((line, indent, commented))
                })
                .collect();
        let first = lines.first()?;
        let edits: Vec<(Range<usize>, String)> = if lines
            .iter()
            .all(|(_, _, commented)| commented.is_some())
        {
            lines
                .iter()
                .filter_map(|(line, indent, commented)| {
                    let at = line.start + indent;
                    commented.map(|length| (at..at + length, String::new()))
                })
                .collect()
        } else {
            // What the lines have in front of them in common: a line that is deeper than the rest has the marker in its indent.
            let shared = lines
                .iter()
                .skip(1)
                .fold(first.1, |shared, (line, ..)| {
                    Self::common_indent(text, first.0.start, line.start, shared)
                });
            let put = format!("{marker} ");
            lines
                .iter()
                .map(|(line, ..)| (line.start + shared..line.start + shared, put.clone()))
                .collect()
        };
        // A caret goes on after what is put in at it, and the ends of a selection do not.
        let after_insert = start == end;
        Some(Shifted {
            anchor: AutoIndent::moved(anchor, &edits, after_insert),
            focus: AutoIndent::moved(focus, &edits, after_insert),
            edits,
        })
    }

    /// How many bytes of blanks `line` starts with, or none where it has nothing but blanks, or nothing.
    fn indent_of(text: &TextSnapshot, line: &Range<usize>) -> Option<usize> {
        let mut at = line.start;
        while at < line.end {
            if !matches!(text.byte_at(at)?, b' ' | b'\t') {
                return Some(at - line.start);
            }
            at += 1;
        }
        None
    }

    /// How many bytes of the indent of the line that starts at `from`, which is `shared` long, the line that starts at `other` has the
    /// same.
    fn common_indent(text: &TextSnapshot, from: usize, other: usize, shared: usize) -> usize {
        (0..shared.min(Self::MAX_INDENT))
            .take_while(|&offset| text.byte_at(from + offset) == text.byte_at(other + offset))
            .count()
    }

    /// How many bytes the marker and the space after it are at `at`, if `marker` is there. A marker that a doc comment begins with is
    /// not one.
    fn marker_at(text: &TextSnapshot, at: usize, end: usize, marker: &str) -> Option<usize> {
        let fits = at + marker.len() <= end;
        if !fits
            || !marker
                .bytes()
                .enumerate()
                .all(|(offset, byte)| text.byte_at(at + offset) == Some(byte))
        {
            return None;
        }
        let after = at + marker.len();
        let next = if after < end {
            text.byte_at(after)
        } else {
            None
        };
        if marker == Self::RUST && matches!(next, Some(b'/' | b'!')) {
            return None;
        }
        Some(marker.len() + usize::from(next == Some(b' ')))
    }

    /// Does what the shortcut does, as one edit that one undo takes back, with the same text still selected. Whether anything was
    /// changed: `false` where no line has text.
    pub fn apply(controller: &TextEditingController, marker: &str) -> bool {
        let selection = controller.value().selection();
        Self::toggle(
            &controller.snapshot(),
            selection.anchor(),
            selection.focus(),
            marker,
        )
        .is_some_and(|toggled| toggled.apply(controller))
    }
}

#[cfg(test)]
#[path = "../tests/model/line_comment.rs"]
mod tests;
