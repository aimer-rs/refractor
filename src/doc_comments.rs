//! Markdown rendered inside the doc comments of Rust code.
//!
//! A run of `///` lines (or `//!`) is a block. Its text, with the markers taken off, is markdown, and a
//! rendered block is drawn as that markdown by the framework's viewer (lists, emphasis, links and code
//! as what they mean), standing in for all the lines of the block, which is why the block has one line
//! number and the height of its rendered text.
//!
//! A rendered block is not edited in place. It is shown as written by the button in the margin, by a
//! click on it, and by typing into it ([`DocRenderer::open_at`]); [`DocRenderer::toggle`] switches one.
//! A block is told apart by its first line, and the lines are kept on their blocks as the text is
//! edited ([`DocRenderer::edited`]).

use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;

use aimer::MarkdownTheme;
use aimer::input::button::Button;
use aimer::input::input::{TextEmbed, TextSnapshot};
use aimer::style::LayoutSpacing;
use aimer::Container;

/// How wide, in logical pixels, the gap is between the bar of a rendered block and its text.
pub const GAP: f32 = 24.0;
/// How wide a character of the code font is, as a share of its size: what an indent of spaces is worth
/// in pixels. The bar stands at the block's indent, and its text after it.
pub const CODE_ADVANCE: f32 = 0.6;
/// The size of the code the indent is measured in.
const CODE_SIZE: f32 = 13.0;

/// The most lines a block is looked for across. A longer run of comment is shown as written: a wall of
/// `///` that long is not documentation anyone is reading as markdown.
const MAX_BLOCK_LINES: usize = 600;

/// Which kind of doc comment a line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Marker {
    /// `///`, which documents what follows.
    Outer,
    /// `//!`, which documents what holds it.
    Inner,
}

/// What comes before the text of a doc comment line: the indent, then the marker and the one space after
/// it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Prefix {
    marker: Marker,
    /// How many bytes of indent the line has.
    indent: usize,
    /// How many bytes come before the text: the indent, the marker and the space.
    text: usize,
}

/// The kind of doc comment `line` is, and what comes before its text. `None` for any other line,
/// including `////`, which is not a doc comment.
fn prefix_of(line: &str) -> Option<Prefix> {
    let trimmed = line.trim_start_matches([' ', '\t']);
    let indent = line.len() - trimmed.len();
    let (marker, rest) = if let Some(rest) = trimmed.strip_prefix("///") {
        if rest.starts_with('/') {
            return None;
        }
        (Marker::Outer, rest)
    } else {
        (Marker::Inner, trimmed.strip_prefix("//!")?)
    };
    Some(Prefix { marker, indent, text: indent + 3 + usize::from(rest.starts_with(' ')) })
}

/// What a new line starts with when Return is pressed at the end of `before`, the text of a line up to
/// the caret: the same indent and marker as a doc comment line has, and a space, so the comment goes
/// on. `None` for any other line. In a rendered block the marker is not on screen, so without this a
/// new line would be plain text that quietly ends the comment.
pub fn continuation(before: &str) -> Option<String> {
    let prefix = prefix_of(before)?;
    let marker = match prefix.marker {
        Marker::Outer => "///",
        Marker::Inner => "//!",
    };
    Some(format!("{}{marker} ", &before[..prefix.indent]))
}

/// A run of doc-comment lines of one kind, from `first_line` to `last_line`, counted from zero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocBlock {
    pub first_line: usize,
    pub last_line: usize,
}

/// The bytes of line `number`, without its line break.
fn line_bytes(text: &TextSnapshot, number: usize) -> Range<usize> {
    let range = text.line_range(number);
    let mut end = range.end;
    if end > range.start && text.byte_at(end - 1) == Some(b'\n') {
        end -= 1;
        if end > range.start && text.byte_at(end - 1) == Some(b'\r') {
            end -= 1;
        }
    }
    range.start..end
}

fn line_text(text: &TextSnapshot, number: usize) -> Cow<'_, str> {
    text.slice(line_bytes(text, number))
}

fn marker_of(text: &TextSnapshot, number: usize) -> Option<Marker> {
    prefix_of(&line_text(text, number)).map(|prefix| prefix.marker)
}

/// Whether line `number` is the first line of a block.
pub fn is_block_start(text: &TextSnapshot, number: usize) -> bool {
    number < text.line_count()
        && marker_of(text, number).is_some_and(|marker| number == 0 || marker_of(text, number - 1) != Some(marker))
}

/// The block that holds line `number`, if it is a doc comment and its block is not too long.
pub fn block_at(text: &TextSnapshot, number: usize) -> Option<DocBlock> {
    if number >= text.line_count() {
        return None;
    }
    let marker = marker_of(text, number)?;
    let mut first = number;
    while first > 0 && number - first < MAX_BLOCK_LINES && marker_of(text, first - 1) == Some(marker) {
        first -= 1;
    }
    let last_possible = text.line_count() - 1;
    let mut last = number;
    while last < last_possible && last - number < MAX_BLOCK_LINES && marker_of(text, last + 1) == Some(marker) {
        last += 1;
    }
    (last - first < MAX_BLOCK_LINES).then_some(DocBlock { first_line: first, last_line: last })
}

/// The blocks that have a line in `bytes`.
pub fn blocks_in(text: &TextSnapshot, bytes: Range<usize>) -> Vec<DocBlock> {
    if text.len() == 0 {
        return Vec::new();
    }
    let first = text.byte_to_line(bytes.start.min(text.len()));
    let last = text.byte_to_line(bytes.end.max(bytes.start).saturating_sub(1).min(text.len()));
    let mut blocks: Vec<DocBlock> = Vec::new();
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

/// The width in logical pixels of `indent` bytes of indent.
pub fn indent_px(indent: usize) -> f32 {
    indent as f32 * CODE_SIZE * CODE_ADVANCE
}

/// Renders the doc comments of a text, and says which blocks are shown as written. Make one for a
/// document and keep it.
pub struct DocRenderer {
    theme: MarkdownTheme,
    /// The first lines of the blocks that are shown as written.
    as_written: BTreeSet<usize>,
}

impl DocRenderer {
    /// A renderer whose blocks are drawn with `theme`.
    pub fn new(theme: MarkdownTheme) -> Self {
        Self { theme, as_written: BTreeSet::new() }
    }

    /// Whether the block that starts on `first_line` is shown rendered.
    pub fn is_rendered(&self, first_line: usize) -> bool {
        !self.as_written.contains(&first_line)
    }

    /// Shows the block that starts on `first_line` as written if it was rendered, and rendered if it
    /// was written.
    pub fn toggle(&mut self, first_line: usize) {
        if !self.as_written.remove(&first_line) {
            self.as_written.insert(first_line);
        }
    }

    /// Shows as written the rendered block that holds line `number`, if there is one: what typing into
    /// a rendered block does, since there is nothing in it to type into. Returns whether one was opened.
    pub fn open_at(&mut self, text: &TextSnapshot, number: usize) -> bool {
        match block_at(text, number) {
            Some(block) if self.is_rendered(block.first_line) => {
                self.as_written.insert(block.first_line);
                true
            }
            _ => false,
        }
    }

    /// Keeps the blocks shown as written on their lines after an edit that replaced `removed` lines
    /// with `inserted` ones, the first of which is `line`: lines below it moved, and those the edit
    /// took away are forgotten.
    pub fn edited(&mut self, line: usize, removed: usize, inserted: usize) {
        if removed == inserted {
            return;
        }
        self.as_written = std::mem::take(&mut self.as_written)
            .into_iter()
            .filter_map(|first| {
                if first <= line {
                    Some(first)
                } else if first <= line + removed {
                    None
                } else {
                    Some(first + inserted - removed)
                }
            })
            .collect();
    }

    /// Whether line `number` of `text` is the first line of a rendered block, which is the one line of
    /// it that is on screen and so the one that has a bar and a number. The answer is how many bytes of
    /// indent the block has, which is where its bar goes.
    pub fn bar_indent(&self, text: &TextSnapshot, number: usize) -> Option<usize> {
        let block = block_at(text, number)?;
        if self.as_written.contains(&block.first_line) {
            return None;
        }
        prefix_of(&line_text(text, block.first_line)).map(|prefix| prefix.indent)
    }

    /// The embeds of the rendered blocks that have a line in `window`: each stands for the whole block,
    /// from the start of its first line to the end of its last, and draws its markdown. `on_open` is
    /// called with the first line of a block when it is clicked.
    pub fn embeds(&self, text: &TextSnapshot, window: Range<usize>, on_open: &Rc<dyn Fn(usize)>) -> Vec<TextEmbed> {
        let mut seen: HashMap<u64, u64> = HashMap::new();
        let mut embeds = Vec::new();
        for block in blocks_in(text, window) {
            if self.as_written.contains(&block.first_line) {
                continue;
            }
            let (markdown, indent) = markdown_of(text, &block);
            let range = line_bytes(text, block.first_line).start..line_bytes(text, block.last_line).end;
            let hashed = hash_of(&(&markdown, indent, block.first_line));
            let occurrence = seen.entry(hashed).and_modify(|count| *count += 1).or_insert(0);
            let key = hash_of(&(hashed, *occurrence));
            let (theme, on_open, first_line) = (self.theme, Rc::clone(on_open), block.first_line);
            let markdown: Rc<str> = Rc::from(markdown);
            embeds.push(TextEmbed::new(range, key, move || {
                let (markdown, on_open) = (Rc::clone(&markdown), Rc::clone(&on_open));
                let left = indent_px(indent) + GAP;
                // A click on the rendered block opens it as written: the way to edit it.
                Button::new().no_decor().on_press(move || on_open(first_line)).build(move |_| {
                    Container::new()
                        .padding(LayoutSpacing::new().left(left as u32).top(4).bottom(6))
                        .child(crate::markdown::doc_viewer(&markdown, &theme))
                })
            }));
        }
        embeds
    }
}

/// The markdown of a block, which is its lines with the markers taken off, and the bytes of indent the
/// block has (that of its first line).
fn markdown_of(text: &TextSnapshot, block: &DocBlock) -> (String, usize) {
    let mut markdown = String::new();
    let mut indent = 0;
    for number in block.first_line..=block.last_line {
        let line = line_text(text, number);
        let prefix = prefix_of(&line).expect("a line of a block is a doc comment line");
        if number == block.first_line {
            indent = prefix.indent;
        }
        markdown.push_str(&line[prefix.text..]);
        if number < block.last_line {
            markdown.push('\n');
        }
    }
    (markdown, indent)
}

fn hash_of(value: &impl Hash) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests;
