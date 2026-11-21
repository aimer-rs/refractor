//! Guides down the indents of code: a thin line at each level of indent, each level in its own color, and the one of the block
//! the caret is in brighter and thicker than the rest.
//!
//! A line has a guide for each whole level of its indent, at the column where the level begins. A blank line has those of
//! the block it is in: the fewer of the lines on either side of it, so a guide runs on through the blank lines of a block
//! and stops where the block does. The block the caret is in is that of the pair of brackets the caret is beside or inside:
//! its guide is the one in line with the line that opens it, and runs down the lines between the brackets.
//!
//! Like the other rules of the editor these read only the lines they are about, and a blank line looks no further than
//! [`IndentGuides::LOOKAROUND`] lines either way.

use std::ops::Range;

use aimer::input::input::{LineBar, TextSnapshot};

use crate::doc_comments;
use crate::syntax::SyntaxPalette;

/// The block the caret is in: the level of its guide, and the lines that the guide is bright on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub level: usize,
    pub lines: Range<usize>,
}

/// How the guides are drawn.
#[derive(Clone, Copy, Debug)]
pub struct GuideStyle {
    /// The colors of the levels.
    pub palette: SyntaxPalette,
    /// How many columns a level of indent is.
    pub unit: usize,
    /// The size of the code, in logical pixels: what a column is worth.
    pub code_size: f32,
}

/// The rules of the guides.
pub struct IndentGuides;

impl IndentGuides {
    /// How many lines a blank line looks, up and down, for the lines it is between.
    pub const LOOKAROUND: usize = 64;
    /// The most bytes of the start of a line that are read for its indent.
    pub const MAX_INDENT: usize = 256;
    /// How much of its color a guide has, out of `1.0`, and how thick it is in logical pixels: the guide of the block that holds
    /// the caret, and the others.
    pub const BRIGHT: f32 = 0.95;
    pub const BRIGHT_WIDTH: f32 = 2.0;
    pub const DIM: f32 = 0.1;
    pub const DIM_WIDTH: f32 = 1.0;

    /// How many columns of indent `line` has, a tab being a level; `None` for a line that has nothing but blanks.
    fn indent(text: &TextSnapshot, line: usize, unit: usize) -> Option<usize> {
        let bytes = doc_comments::line_bytes(text, line);
        let mut columns = 0;
        for (read, at) in bytes.clone().enumerate() {
            match text.byte_at(at)? {
                b' ' => columns += 1,
                b'\t' => columns += unit,
                _ => return Some(columns),
            }
            if read + 1 >= Self::MAX_INDENT {
                // Only blanks as far as it is read: a line that is indented that far is indented, and not blank.
                return Some(columns);
            }
        }
        None
    }

    /// How many guides `line` has: one for each whole level of its indent.
    pub fn levels(text: &TextSnapshot, line: usize, unit: usize) -> usize {
        let unit = unit.max(1);
        match Self::indent(text, line, unit) {
            Some(columns) => columns / unit,
            None => Self::between(text, line, unit),
        }
    }

    /// The guides of a blank line: the fewer of what the line before it, and the line after it, have, where the line before
    /// opens a block that the line is in. `0` where either is further than [`Self::LOOKAROUND`] lines away.
    fn between(text: &TextSnapshot, line: usize, unit: usize) -> usize {
        let last = text.line_count().saturating_sub(1);
        let before = (line.saturating_sub(Self::LOOKAROUND)..line.min(last + 1))
            .rev()
            .find_map(|at| {
                Self::indent(text, at, unit)
                    .map(|columns| columns / unit + usize::from(Self::opens(text, at)))
            });
        let after = (line + 1..=(line + Self::LOOKAROUND).min(last))
            .find_map(|at| Self::indent(text, at, unit).map(|columns| columns / unit));
        before.unwrap_or(0).min(after.unwrap_or(0))
    }

    /// Whether `line` ends by opening a block or a list. A comment does not.
    fn opens(text: &TextSnapshot, line: usize) -> bool {
        let bytes = doc_comments::line_bytes(text, line);
        let mut end = bytes.end;
        while end > bytes.start && matches!(text.byte_at(end - 1), Some(b' ' | b'\t')) {
            end -= 1;
        }
        if end == bytes.start || !matches!(text.byte_at(end - 1), Some(b'{' | b'(' | b'[')) {
            return false;
        }
        let mut start = bytes.start;
        while start < end && matches!(text.byte_at(start), Some(b' ' | b'\t')) {
            start += 1;
        }
        !(text.byte_at(start) == Some(b'/') && text.byte_at(start + 1) == Some(b'/'))
    }

    /// The block of the pair of brackets whose opener is at `open` and closer at `close`: the guide in line with the line that
    /// opens it, down the lines between. `None` for a pair that has no line between its brackets.
    pub fn block(
        text: &TextSnapshot,
        open: &Range<usize>,
        close: &Range<usize>,
        unit: usize,
    ) -> Option<Block> {
        let unit = unit.max(1);
        let (first, last) = (
            text.byte_to_line(open.start.min(text.len())),
            text.byte_to_line(close.start.min(text.len())),
        );
        if last <= first + 1 {
            return None;
        }
        let level = Self::indent(text, first, unit)? / unit;
        Some(Block {
            level,
            lines: first + 1..last,
        })
    }

    /// Puts the guides of `line` in `out`: a bar for each level, in the color of the level at the column it begins at, and
    /// the one of `block` brighter on the lines of it.
    pub fn bars(
        text: &TextSnapshot,
        line: usize,
        block: Option<&Block>,
        style: &GuideStyle,
        out: &mut Vec<LineBar>,
    ) {
        let levels = Self::levels(text, line, style.unit);
        for level in 0..levels {
            let held =
                block.is_some_and(|block| block.level == level && block.lines.contains(&line));
            let (alpha, width) = if held {
                (Self::BRIGHT, Self::BRIGHT_WIDTH)
            } else {
                (Self::DIM, Self::DIM_WIDTH)
            };
            let offset =
                doc_comments::indent_px(level * style.unit, style.code_size).round() as u32;
            out.push(
                LineBar::new(style.palette.rainbow(level).with_alpha(alpha), offset).width(width),
            );
        }
    }
}

#[cfg(test)]
mod tests;
