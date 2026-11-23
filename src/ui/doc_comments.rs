//! Markdown rendered inside the doc comments of Rust code.
//!
//! A run of `///` lines (or `//!`) is a block. Its text, with the markers taken off, is markdown, and a
//! rendered block is drawn as that markdown by the framework's viewer (lists, emphasis, links and code
//! as what they mean), standing in for all the lines of the block, which is why the block has one line
//! number and the height of its rendered text.
//!
//! A rendered block is not edited in place, and its text cannot be changed while it is rendered:
//! [`DocRenderer::protects`] says which edits that refuses. Only the button in the margin shows one as
//! written ([`DocRenderer::toggle`]): nothing else does, not a click on it, nor the caret going into it,
//! nor an edit. A block is told apart by its first line, and the lines are kept on their blocks as the
//! text is edited ([`DocRenderer::edited`]).

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;

use aimer::Container;
use aimer::MarkdownTheme;
use aimer::input::input::{TextEmbed, TextSnapshot};
use aimer::style::LayoutSpacing;

mod links;

pub use links::DocLink;
use links::LinkScanner;

/// What is done with a link of a rendered block that is clicked.
pub type LinkHandler = Rc<dyn Fn(&DocLink)>;

/// How wide, in logical pixels, the gap is between the bar of a rendered block and its text.
pub const GAP: f32 = 24.0;
/// How wide a character of the code font is, as a share of its size: what an indent of spaces is worth
/// in pixels. The bar stands at the block's indent, and its text after it.
pub const CODE_ADVANCE: f32 = 0.6;
/// The size of the code a renderer measures an indent in until it is told another.
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
    Some(Prefix {
        marker,
        indent,
        text: indent + 3 + usize::from(rest.starts_with(' ')),
    })
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
pub(crate) fn line_bytes(text: &TextSnapshot, number: usize) -> Range<usize> {
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
        && marker_of(text, number)
            .is_some_and(|marker| number == 0 || marker_of(text, number - 1) != Some(marker))
}

/// The block that holds line `number`, if it is a doc comment and its block is not too long.
pub fn block_at(text: &TextSnapshot, number: usize) -> Option<DocBlock> {
    block_around(number, text.line_count(), &|line| marker_of(text, line))
}

/// [`block_at`] with the kind of each line given by `marker`, and the number of lines known.
fn block_around(
    number: usize,
    line_count: usize,
    marker: &dyn Fn(usize) -> Option<Marker>,
) -> Option<DocBlock> {
    if number >= line_count {
        return None;
    }
    let kind = marker(number)?;
    let mut first = number;
    while first > 0 && number - first < MAX_BLOCK_LINES && marker(first - 1) == Some(kind) {
        first -= 1;
    }
    let last_possible = line_count - 1;
    let mut last = number;
    while last < last_possible && last - number < MAX_BLOCK_LINES && marker(last + 1) == Some(kind)
    {
        last += 1;
    }
    (last - first < MAX_BLOCK_LINES).then_some(DocBlock {
        first_line: first,
        last_line: last,
    })
}

/// The longest a line is held, in bytes, while looking for its marker across the pieces of the text.
/// The marker is after the indent, so a line with more indent than this is asked about by itself.
const MAX_HELD_LINE: usize = 1024;

/// The kind of doc comment each of the lines `first` to `last` is, in order, found in one pass over
/// their text. Asking for each line on its own looks it up in the tree of the text, which costs far more
/// than reading it.
fn markers_of(text: &TextSnapshot, first: usize, last: usize) -> Vec<Option<Marker>> {
    let count = last + 1 - first;
    let mut markers = Vec::with_capacity(count);
    let bytes = text.line_range(first).start..text.line_range(last).end;
    // The start of the line that a piece of text ends in the middle of, for the next piece to go on.
    let mut held = String::new();
    let mut too_long = false;
    let kind_of = |line: &str| prefix_of(line).map(|prefix| prefix.marker);
    for piece in text.chunks(bytes) {
        let mut rest = piece;
        while let Some(at) = rest.find('\n') {
            let head = &rest[..at];
            let marker = if held.is_empty() && !too_long {
                kind_of(head)
            } else if too_long || held.len() + head.len() > MAX_HELD_LINE {
                marker_of(text, first + markers.len())
            } else {
                held.push_str(head);
                kind_of(&held)
            };
            markers.push(marker);
            held.clear();
            too_long = false;
            rest = &rest[at + 1..];
        }
        if !too_long {
            if held.len() + rest.len() > MAX_HELD_LINE {
                too_long = true;
            } else {
                held.push_str(rest);
            }
        }
    }
    // A last line that is not ended by a line break.
    if markers.len() < count {
        markers.push(if too_long {
            marker_of(text, first + markers.len())
        } else {
            kind_of(&held)
        });
    }
    markers
}

/// The blocks that have a line in `bytes`.
pub fn blocks_in(text: &TextSnapshot, bytes: Range<usize>) -> Vec<DocBlock> {
    if text.len() == 0 {
        return Vec::new();
    }
    let first = text.byte_to_line(bytes.start.min(text.len()));
    let last = text.byte_to_line(
        bytes
            .end
            .max(bytes.start)
            .saturating_sub(1)
            .min(text.len()),
    );
    // The lines of the window are read in one pass. A block that reaches out of it is followed to its
    // ends one line at a time, which is a few lines and not the file.
    let window = markers_of(text, first, last);
    let marker = |number: usize| {
        if (first..=last).contains(&number) {
            window[number - first]
        } else {
            marker_of(text, number)
        }
    };
    let line_count = text.line_count();
    let mut blocks: Vec<DocBlock> = Vec::new();
    let mut number = first;
    while number <= last {
        match block_around(number, line_count, &marker) {
            Some(block) => {
                number = block.last_line + 1;
                blocks.push(block);
            }
            None => number += 1,
        }
    }
    blocks
}

/// The width in logical pixels of `indent` bytes of indent in code of `code_size` logical pixels.
pub fn indent_px(indent: usize, code_size: f32) -> f32 {
    indent as f32 * code_size * CODE_ADVANCE
}

/// The width in logical pixels of a rendered block whose text is at most `max_text` wide: its indent and
/// the gap after the bar, then the text. `None` when there is no limit, which leaves the block as wide as
/// the editor offers. A width is clamped to what the editor offers, so a limit wider than the editor
/// changes nothing.
fn block_width(indent: usize, code_size: f32, max_text: Option<f32>) -> Option<f32> {
    max_text.map(|text| indent_px(indent, code_size) + GAP + text)
}

/// Renders the doc comments of a text, and says which blocks are shown as written. Make one for a
/// document and keep it.
pub struct DocRenderer {
    theme: MarkdownTheme,
    /// Counts the themes the renderer was given after the first: part of the key of an embed, since an
    /// embed that is kept is drawn again only when its key changes, and a new theme must redraw it.
    theme_generation: u64,
    /// The size of the code the blocks sit in, which an indent is measured in.
    code_size: f32,
    /// The widest the text of a block is drawn, in logical pixels, or `None` for as wide as the editor.
    max_width: Option<f32>,
    /// The first lines of the blocks that are shown as written.
    as_written: BTreeSet<usize>,
    /// What is done when a link of a rendered block is clicked; shared with the widgets of the blocks.
    on_link: Rc<RefCell<Option<LinkHandler>>>,
    /// The blocks [`bar_indent`](Self::bar_indent) found, for the text it was last asked about.
    bars: RefCell<BarCache>,
}

/// What [`DocRenderer::bar_indent`] has worked out about one text, so that a frame that asks about the same
/// lines again as the text has not changed reads none of them again. It is what makes a line cost a lookup
/// in a map and not a walk over its block: without it a block of `n` lines is read `2n` times each frame.
#[derive(Default)]
struct BarCache {
    /// The text the entries are about. A snapshot is made again by any edit, so the entries never stand
    /// for another text than the one they were worked out from.
    text: Option<TextSnapshot>,
    /// For a line, the indent of the first line of its block, or `None` when it is not in a block.
    lines: HashMap<usize, Option<(DocBlock, usize)>>,
    /// How many lines were worked out from the text rather than found here.
    #[cfg(test)]
    scans: usize,
}

impl BarCache {
    /// The most lines kept for one text. The lines on screen are asked for on every frame, so what is kept
    /// is the window being scrolled through; a text that has been scrolled over end to end starts again
    /// when it reaches this, and the memory stays that of a screenful and a bit.
    const MAX_LINES: usize = 4096;
}

impl DocRenderer {
    /// The width in logical pixels the text of a block is limited to in the editor: about a hundred
    /// characters of prose, past which a line is hard to follow.
    pub const READABLE_WIDTH: f32 = 640.0;

    /// A renderer whose blocks are drawn with `theme`.
    pub fn new(theme: MarkdownTheme) -> Self {
        Self {
            theme,
            theme_generation: 0,
            code_size: CODE_SIZE,
            max_width: None,
            on_link: Rc::default(),
            as_written: BTreeSet::new(),
            bars: RefCell::new(BarCache::default()),
        }
    }

    /// Draws the blocks with `theme` from now on. The embeds it makes next are told apart from those
    /// made before, so what shows them draws them again.
    pub fn set_theme(&mut self, theme: MarkdownTheme) {
        self.theme = theme;
        self.theme_generation += 1;
    }

    /// Measures the indent of a block in code of `size` logical pixels, as the text beside it is set in. A
    /// size that is not the one it has makes the embeds new, as [`set_theme`](Self::set_theme) does.
    pub fn set_code_size(&mut self, size: f32) {
        if self.code_size != size {
            self.code_size = size;
            self.theme_generation += 1;
        }
    }

    /// The widest, in logical pixels, the text of a rendered block is drawn, not counting the indent and
    /// the gap after the bar. `None` is no limit.
    #[cfg(test)]
    pub fn max_width(&self) -> Option<f32> {
        self.max_width
    }

    /// Limits the text of a rendered block to `width` logical pixels, so that a long line of comment
    /// wraps to a readable column and not to the whole editor. `None` removes the limit. A width that is
    /// not a number or is endless is no limit, and a negative one is none at all. A limit that is not the
    /// one it has makes the embeds new, as [`set_theme`](Self::set_theme) does.
    pub fn set_max_width(&mut self, width: Option<f32>) {
        let width = width
            .filter(|width| width.is_finite())
            .map(|width| width.max(0.0));
        if self.max_width != width {
            self.max_width = width;
            self.theme_generation += 1;
        }
    }

    /// Calls `handler` with the link when one in a rendered block is clicked. The link says which line of the text its name is
    /// on and at which byte of the line, which is where to ask what it names. Replaces the handler it had, and
    /// reaches the blocks drawn already.
    pub fn set_on_link(&mut self, handler: impl Fn(&DocLink) + 'static) {
        *self.on_link.borrow_mut() = Some(Rc::new(handler));
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

    /// Whether an edit that removes the bytes `removed` of `text` (or inserts at `removed.start`, when
    /// the range is empty) would change the text of a rendered block, which is immutable while it is
    /// rendered. That is an edit that lands on any of its lines, line breaks included, and one that
    /// removes the line break at its edge so that the code beside it joins its first or last line.
    /// Whole lines beside a block, and the line breaks between them, can still be edited.
    ///
    /// The cost is that of the lines the edit touches, which for a caret is one.
    pub fn protects(&self, text: &TextSnapshot, removed: Range<usize>) -> bool {
        let len = text.len();
        let start = removed.start.min(len);
        let end = removed.end.min(len).max(start);
        if len == 0 {
            return false;
        }
        // The lines the edit lands on. A removal that ends at the start of a line leaves that line
        // alone but takes the line break that ends the line above.
        let first = text.byte_to_line(start);
        let last = text.byte_to_line(if end > start { end - 1 } else { start });
        let mut number = first;
        while number <= last {
            match block_at(text, number) {
                Some(block) => {
                    if self.is_rendered(block.first_line) {
                        return true;
                    }
                    number = block.last_line + 1;
                }
                None => number += 1,
            }
        }
        // A removal that ends where a block begins joins what is left of the line it began on to the
        // first line of the block, unless it took that line from its start.
        if end > start {
            let below = text.byte_to_line(end);
            return text.line_range(below).start == end
                && is_block_start(text, below)
                && self.is_rendered(below)
                && start > text.line_range(text.byte_to_line(start)).start;
        }
        false
    }

    /// Whether Backspace (`backwards`) or Delete, pressed with the selection `selection`, would be an
    /// edit [`Self::protects`] refuses: it removes the selection, or with none, the character before
    /// or after the caret, which is a whole line break when the caret is next to one.
    ///
    /// Typing is refused by making the area read-only while the caret is where an edit would land in
    /// a block. These two keys need more: at the start of a line they remove the line break above,
    /// which is outside any block, and it is the line beside a block that gets joined to it.
    pub fn refuses_deletion(
        &self,
        text: &TextSnapshot,
        selection: Range<usize>,
        backwards: bool,
    ) -> bool {
        let removed = if selection.is_empty() {
            let caret = selection.start;
            if backwards {
                if caret == 0 {
                    return false;
                }
                let crlf = text.byte_at(caret - 1) == Some(b'\n')
                    && caret >= 2
                    && text.byte_at(caret - 2) == Some(b'\r');
                caret - if crlf { 2 } else { 1 }..caret
            } else {
                if caret >= text.len() {
                    return false;
                }
                let crlf =
                    text.byte_at(caret) == Some(b'\r') && text.byte_at(caret + 1) == Some(b'\n');
                caret..caret + if crlf { 2 } else { 1 }
            }
        } else {
            selection
        };
        self.protects(text, removed)
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
        let (block, indent) = self.block_with_indent(text, number)?;
        (!self.as_written.contains(&block.first_line)).then_some(indent)
    }

    /// The block that holds line `number` and the indent of its first line, kept for the text so that
    /// the next ask about any line of the block is a lookup. What is shown as written is not part of it,
    /// so toggling a block needs nothing forgotten.
    fn block_with_indent(&self, text: &TextSnapshot, number: usize) -> Option<(DocBlock, usize)> {
        let mut cache = self.bars.borrow_mut();
        if !cache
            .text
            .as_ref()
            .is_some_and(|cached| cached.ptr_eq(text))
        {
            cache.text = Some(text.clone());
            cache.lines.clear();
        } else if let Some(found) = cache.lines.get(&number) {
            return found.clone();
        }
        if cache.lines.len() >= BarCache::MAX_LINES {
            cache.lines.clear();
        }
        #[cfg(test)]
        {
            cache.scans += 1;
        }
        let found = block_at(text, number).and_then(|block| {
            let indent = prefix_of(&line_text(text, block.first_line))?.indent;
            Some((block, indent))
        });
        // Every line of the block has the same answer, and the lines of a block on screen are asked for
        // one after the other, so the walk that found this one is not made again for the rest.
        if let Some((block, _)) = &found {
            for line in block.first_line..=block.last_line {
                cache.lines.insert(line, found.clone());
            }
        } else {
            cache.lines.insert(number, None);
        }
        found
    }

    /// The embeds of the rendered blocks that have a line in `window`: each stands for the whole block,
    /// from the start of its first line to the end of its last, and draws its markdown. A click on one
    /// does nothing to it: it is shown as written by the button in the margin and by nothing else.
    pub fn embeds(&self, text: &TextSnapshot, window: Range<usize>) -> Vec<TextEmbed> {
        let mut seen: HashMap<u64, u64> = HashMap::new();
        let mut embeds = Vec::new();
        for block in blocks_in(text, window) {
            if self.as_written.contains(&block.first_line) {
                continue;
            }
            let (markdown, indent, links) = markdown_of(text, &block);
            let range =
                line_bytes(text, block.first_line).start..line_bytes(text, block.last_line).end;
            // The links are in the key because the markdown has only their numbers: one that moves along its line is
            // another widget, which is clicked to another place.
            let hashed = hash_of(&(&markdown, indent, block.first_line, &links));
            let occurrence = seen
                .entry(hashed)
                .and_modify(|count| *count += 1)
                .or_insert(0);
            let key = hash_of(&(hashed, *occurrence, self.theme_generation));
            let theme = self.theme;
            let code_size = self.code_size;
            let width = block_width(indent, code_size, self.max_width);
            let markdown: Rc<str> = Rc::from(markdown);
            let links: Rc<[DocLink]> = Rc::from(links);
            let on_link = Rc::clone(&self.on_link);
            embeds.push(TextEmbed::new(range, key, move || {
                let markdown = Rc::clone(&markdown);
                let on_link = Rc::clone(&on_link);
                // The handler is looked up when a link is clicked, so one set after the block was drawn is the one used.
                let click: Rc<dyn Fn(&DocLink)> = Rc::new(move |link| {
                    let handler = on_link.borrow().clone();
                    if let Some(handler) = handler {
                        handler(link);
                    }
                });
                let left = indent_px(indent, code_size) + GAP;
                let container = match width {
                    Some(width) => Container::new().width(width),
                    None => Container::new(),
                };
                container
                    .padding(
                        LayoutSpacing::new()
                            .left(left as u32)
                            .top(4)
                            .bottom(6),
                    )
                    .child(crate::ui::markdown::doc_viewer(
                        &markdown,
                        &theme,
                        Rc::clone(&links),
                        click,
                    ))
            }));
        }
        embeds
    }
}

/// The markdown of a block, which is its lines with the markers taken off and each link ([`DocLink`]) followed by the
/// target that numbers it, the bytes of indent the block has (that of its first line), and the links.
fn markdown_of(text: &TextSnapshot, block: &DocBlock) -> (String, usize, Vec<DocLink>) {
    let mut markdown = String::new();
    let mut indent = 0;
    let mut scanner = LinkScanner::new();
    for number in block.first_line..=block.last_line {
        let line = line_text(text, number);
        let prefix = prefix_of(&line).expect("a line of a block is a doc comment line");
        if number == block.first_line {
            indent = prefix.indent;
        }
        scanner.scan_line(&line[prefix.text..], number, prefix.text, &mut markdown);
        if number < block.last_line {
            markdown.push('\n');
        }
    }
    (markdown, indent, scanner.into_links())
}

fn hash_of(value: &impl Hash) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
#[path = "../tests/ui/doc_comments.rs"]
mod tests;
