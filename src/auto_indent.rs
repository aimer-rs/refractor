//! Indentation as lines are made and taken away: a new line starts as deep as the one before it, and a level deeper
//! inside a block or brackets; Return between an opener and its closer puts the closer on a line of its own under the
//! opener; Backspace in a line of only blanks takes the whole line; and Tab and Shift+Tab over a selection put a level in
//! front of each line it touches, or take one from it.
//!
//! Like [`AutoPair`](crate::auto_pair::AutoPair), the rules read only the line they are at, and what is done is one
//! edit through the controller, which one undo takes back.

use std::ops::Range;

use aimer::FocusNode;
use aimer::input::ControllerListener;
use aimer::input::input::{TextEditingController, TextSnapshot};

use crate::auto_pair::AutoPair;
use crate::doc_comments;

/// What Return does between an opener and its closer: the line break and the indent that goes before the caret, and the
/// line break and the indent that goes after it, before the closer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Split {
    /// Where the caret is, in bytes, and where both go in.
    pub at: usize,
    pub first: String,
    pub second: String,
}

/// What Tab or Shift+Tab does with a selection: an edit for each line that changes, in the order of the text, and the
/// selection that keeps the same text selected once they are made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shifted {
    pub edits: Vec<(Range<usize>, String)>,
    /// Where the selection starts and where the caret is, in bytes of the text after the edits.
    pub anchor: usize,
    pub focus: usize,
}

impl Shifted {
    /// Makes the edits as one edit that one undo takes back, and selects what is selected. Whether there was an edit to make.
    pub(crate) fn apply(&self, controller: &TextEditingController) -> bool {
        if self.edits.is_empty() {
            return false;
        }
        let edits: Vec<(Range<usize>, &str)> = self
            .edits
            .iter()
            .map(|(range, text)| (range.clone(), text.as_str()))
            .collect();
        controller.replace_ranges(&edits, 0);
        controller.select_bytes(self.anchor, self.focus);
        true
    }
}

/// The rules of indenting.
pub struct AutoIndent;

impl AutoIndent {
    /// The most bytes of a line's indent that are read: a line that is blank for longer than this is not an indent.
    const MAX_INDENT: usize = 256;
    /// The marks that open a block or a list, which a line may end with, and which end up one level deeper.
    const OPENERS: [char; 3] = ['{', '(', '['];

    /// What a new line starts with when Return is pressed at the end of `before`, the line up to the caret: as much indent
    /// as the line has, and a level more where the line ends by opening a block or a list. A level is `spaces` spaces,
    /// or a tab where the line is indented with tabs.
    pub fn continuation(before: &str, spaces: usize) -> String {
        let trimmed = before.trim_start_matches([' ', '\t']);
        let indent = &before[..before.len() - trimmed.len()];
        // A comment that happens to end in a bracket does not open anything.
        let opens = !trimmed.starts_with("//") && trimmed.trim_end().ends_with(Self::OPENERS);
        if opens {
            format!("{indent}{}", Self::level(indent, spaces))
        } else {
            indent.to_owned()
        }
    }

    /// One level of indent, for a line indented as `indent` is.
    fn level(indent: &str, spaces: usize) -> String {
        if indent.contains('\t') {
            "\t".to_owned()
        } else {
            " ".repeat(spaces)
        }
    }

    /// What Return does with `selection` selected in `text`, where it is between an opener and the closer that goes
    /// with it. `None` for any other Return, which is the field's.
    pub fn split(text: &TextSnapshot, selection: Range<usize>, spaces: usize) -> Option<Split> {
        if !selection.is_empty() {
            return None;
        }
        let at = selection.start;
        let before = AutoPair::char_before(text, at)?;
        let after = AutoPair::char_after(text, at)?;
        let closes = match before {
            '{' => '}',
            '(' => ')',
            '[' => ']',
            '<' => '>',
            _ => return None,
        };
        if after != closes {
            return None;
        }
        let line = text.line_range(text.byte_to_line(at));
        let indent = Self::indent_of(text, line.start, at)?;
        // The line is a comment: what is in it is not code to be split.
        if text.byte_at(line.start + indent.len()) == Some(b'/')
            && text.byte_at(line.start + indent.len() + 1) == Some(b'/')
        {
            return None;
        }
        let level = Self::level(&indent, spaces);
        Some(Split {
            at,
            first: format!("\n{indent}{level}"),
            second: format!("\n{indent}"),
        })
    }

    /// The bytes of `line_start..limit` that are blanks from the start: the indent of the line. `None` where it is longer
    /// than [`Self::MAX_INDENT`].
    fn indent_of(text: &TextSnapshot, line_start: usize, limit: usize) -> Option<String> {
        let mut indent = String::new();
        let mut at = line_start;
        while at < limit {
            match text.byte_at(at)? {
                byte @ (b' ' | b'\t') => indent.push(byte as char),
                _ => break,
            }
            if indent.len() > Self::MAX_INDENT {
                return None;
            }
            at += 1;
        }
        Some(indent)
    }

    /// The bytes that Backspace takes with `selection` empty in a line of only blanks, which is all of the line and the
    /// line break before it, or only its blanks where it is the first. `None` for any other line, which is the field's:
    /// an empty line has nothing to take but its break, which Backspace takes as it is, and a line under a doc comment is
    /// not joined to it.
    pub fn blank_line(text: &TextSnapshot, selection: Range<usize>) -> Option<Range<usize>> {
        if !selection.is_empty() {
            return None;
        }
        let number = text.byte_to_line(selection.start);
        let line = doc_comments::line_bytes(text, number);
        if line.is_empty()
            || line.len() > Self::MAX_INDENT
            || !line
                .clone()
                .all(|at| matches!(text.byte_at(at), Some(b' ' | b'\t')))
        {
            return None;
        }
        if number == 0 {
            return Some(line);
        }
        if doc_comments::block_at(text, number - 1).is_some() {
            return None;
        }
        Some(doc_comments::line_bytes(text, number - 1).end..line.end)
    }

    /// What Tab does with the selection `anchor` to `focus`, or Shift+Tab where `outdent`: each line that the selection touches has a level
    /// put in front of it, or one taken from the front of it. A line that the selection only reaches the start of, by ending there, is
    /// not touched. A level is `spaces` spaces, or a tab where a line of the block is indented with a tab. Empty lines are not
    /// indented, as there is nothing in them to be deeper; a line that has less than a level to take loses what it has. `None` for a
    /// caret, which is not a selection: Tab and Shift+Tab are the field's.
    ///
    /// The ends of the selection stay with the text they were at. The start of a selection that begins at the start of a line stays
    /// there, so that what was put in is selected, and the next Tab goes on from it.
    pub fn shift_lines(text: &TextSnapshot, anchor: usize, focus: usize, spaces: usize, outdent: bool) -> Option<Shifted> {
        let (start, end) = (anchor.min(focus), anchor.max(focus));
        if start == end {
            return None;
        }
        let lines = Self::touched_lines(text, start, end);
        let mut edits: Vec<(Range<usize>, String)> = Vec::new();
        if outdent {
            for line in &lines {
                let taken = Self::level_taken(text, line, spaces);
                if taken > 0 {
                    edits.push((line.start..line.start + taken, String::new()));
                }
            }
        } else {
            let level = Self::block_level(text, &lines, spaces);
            for line in lines.iter().filter(|line| !line.is_empty()) {
                edits.push((line.start..line.start, level.clone()));
            }
        }
        let (anchor, focus) = (Self::moved(anchor, &edits, false), Self::moved(focus, &edits, false));
        Some(Shifted { edits, anchor, focus })
    }

    /// The lines, without their line breaks, that the selection `start` to `end` touches. A line that the selection only reaches the
    /// start of, by ending there, is not one, unless it is the only line. A caret touches the line it is on.
    pub(crate) fn touched_lines(text: &TextSnapshot, start: usize, end: usize) -> Vec<Range<usize>> {
        let first = text.byte_to_line(start);
        let mut last = text.byte_to_line(end);
        if last > first && end == text.line_range(last).start {
            last -= 1;
        }
        (first..=last)
            .map(|number| doc_comments::line_bytes(text, number))
            .collect()
    }

    /// The level that goes in front of the `lines` of a block: a tab where any of them is indented with one, and spaces otherwise.
    fn block_level(text: &TextSnapshot, lines: &[Range<usize>], spaces: usize) -> String {
        let tabbed = lines.iter().any(|line| {
            line.clone()
                .take(Self::MAX_INDENT)
                .map_while(|at| text.byte_at(at).filter(|byte| matches!(byte, b' ' | b'\t')))
                .any(|byte| byte == b'\t')
        });
        if tabbed { "\t".to_owned() } else { " ".repeat(spaces) }
    }

    /// How many bytes at the start of `line` a level is: a tab, or the spaces there are, up to the width of a level.
    fn level_taken(text: &TextSnapshot, line: &Range<usize>, spaces: usize) -> usize {
        match text.byte_at(line.start) {
            _ if line.is_empty() => 0,
            Some(b'\t') => 1,
            Some(b' ') => line
                .clone()
                .take(spaces)
                .take_while(|&at| text.byte_at(at) == Some(b' '))
                .count(),
            _ => 0,
        }
    }

    /// Where the byte `at` is once `edits`, which are in the order of the text and do not overlap, are made. What is put in before it
    /// moves it on, and a place that is in what is taken goes to where the taking began. What is put in at `at` itself is after it
    /// where `after_insert` is, as for a caret, which goes on after what is put in at it, and before it otherwise, as for the start of
    /// a selection, which then has what was put in inside it.
    pub(crate) fn moved(at: usize, edits: &[(Range<usize>, String)], after_insert: bool) -> usize {
        let mut shift: isize = 0;
        for (range, text) in edits {
            let past = if after_insert && range.is_empty() { range.start > at } else { range.start >= at };
            if past {
                break;
            }
            // How much of what is taken is before `at`: all of it, or as far as `at`.
            shift += text.len() as isize - (range.end.min(at) - range.start) as isize;
        }
        at.checked_add_signed(shift).expect("an edit does not take more than there is before the place")
    }

    /// Does what Tab does with a selection, or Shift+Tab where `outdent`, as one edit that one undo takes back, with the same text
    /// still selected. Whether it was the key's: `false` for a caret, which leaves the key to the field. A selection that has nothing
    /// to take is the key's too, though nothing is changed.
    pub fn apply_shift(controller: &TextEditingController, spaces: usize, outdent: bool) -> bool {
        let selection = controller.value().selection();
        let Some(shifted) = Self::shift_lines(&controller.snapshot(), selection.anchor(), selection.focus(), spaces, outdent) else {
            return false;
        };
        // Nothing to take is still the key's: it is not the field's to move focus with.
        shifted.apply(controller);
        true
    }

    /// Keeps `focus` holding Shift+Tab for the editor for as long as something is selected in `controller`, for as long as the listener
    /// that is returned lives. The framework moves focus to the one before on Shift+Tab, before the editor is asked, unless the focus
    /// owner holds the key: and a selection is the only time Shift+Tab is the editor's, to outdent it. With a caret it leaves the
    /// editor as it does from anything, so that the keyboard is not caught in it. Tab is the editor's already.
    pub fn hold_tab_while_selected(focus: FocusNode, controller: TextEditingController) -> ControllerListener {
        focus.set_holds_tab(!controller.value().selection().is_collapsed());
        let watched = controller.clone();
        controller.listen(move |_| focus.set_holds_tab(!watched.value().selection().is_collapsed()))
    }

    /// Does what Return does between an opener and its closer, as one edit, with the caret on the line between them.
    /// Whether it did: `false` leaves the key to the field.
    pub fn apply_return(controller: &TextEditingController, spaces: usize) -> bool {
        let selection = controller.value().selection().range();
        let Some(split) = Self::split(&controller.snapshot(), selection, spaces) else {
            return false;
        };
        // Two insertions at the one place, which touch: the caret goes after the first.
        controller.replace_ranges(
            &[
                (split.at..split.at, split.first.as_str()),
                (split.at..split.at, split.second.as_str()),
            ],
            0,
        )
    }

    /// Does what Backspace does in a line of only blanks, as one edit. Whether it did: `false` leaves the key to the
    /// field.
    pub fn apply_backspace(controller: &TextEditingController) -> bool {
        let selection = controller.value().selection().range();
        match Self::blank_line(&controller.snapshot(), selection) {
            Some(blank) => controller.replace_range(blank, ""),
            None => false,
        }
    }
}

#[cfg(test)]
mod tests;
