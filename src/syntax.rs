//! Syntax highlighting for the text on screen.
//!
//! A [`SyntaxHighlighter`] keeps the tree-sitter parse of a document and makes it again, from the
//! previous tree, after each edit, so a keystroke costs the part of the tree it changed and not the
//! file. The parser reads the text straight out of the editor's rope a chunk at a time, so no copy
//! of the file is made to parse it. The editor asks for the styles of the lines it is about to draw,
//! and only the captures of those lines are looked up.

mod palette;

use crate::doc_comments::{self, DocRenderer};

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use aimer::input::input::{
    HighlightContext, SpanStyle, StyledRange, TextEmbedder, TextHighlighter, TextSnapshot,
};
use arborium::tree_sitter::{
    InputEdit, Language, Node, Parser, Point, Query, QueryCursor, StreamingIterator, Tree,
};

pub use self::palette::SyntaxPalette;

/// The longest text that is highlighted, in bytes. A file past it is shown plain: parsing it whole
/// the first time would be a pause the user would see, and it is not code anyone edits.
const MAX_HIGHLIGHTED_BYTES: usize = 4 * 1024 * 1024;

/// The syntax of one document: its parse, kept current through the edits, and the styles to draw
/// it with. Clones share the parse.
#[derive(Clone)]
pub struct SyntaxHighlighter {
    engine: Rc<RefCell<Engine>>,
}

impl SyntaxHighlighter {
    /// A highlighter for Rust.
    ///
    /// # Panics
    ///
    /// If the grammar or its highlights query is not one tree-sitter accepts, which is a fault in
    /// the build and not in any input.
    pub fn rust(palette: SyntaxPalette) -> Self {
        let language: Language = arborium::lang_rust::language().into();
        let query = Query::new(&language, arborium::lang_rust::HIGHLIGHTS_QUERY)
            .expect("the Rust highlights query is valid");
        let mut parser = Parser::new();
        parser
            .set_language(&language)
            .expect("the Rust grammar is one this tree-sitter reads");
        let styles = query
            .capture_names()
            .iter()
            .map(|name| palette.style_for(name))
            .collect();
        let docs = DocRenderer::new(crate::markdown::doc_theme());
        Self {
            engine: Rc::new(RefCell::new(Engine {
                parser,
                query,
                styles,
                tree: None,
                text: TextSnapshot::new(),
                incremental_parses: 0,
                docs,
            })),
        }
    }

    /// The highlighter to give a text area: it is asked for the styles of the lines being drawn, and
    /// parses what changed since it was last asked.
    pub fn highlighter(&self) -> TextHighlighter {
        let engine = Rc::clone(&self.engine);
        TextHighlighter::windowed(move |text, range, _context: &HighlightContext| {
            engine
                .borrow_mut()
                .ranges(text, range)
        })
    }

    /// Whether line `line` starts a doc comment, and if it does, whether the block is rendered
    /// (`Some(true)`) or shown as written: the state the button in the margin shows.
    pub fn doc_block_at(&self, text: &TextSnapshot, line: usize) -> Option<bool> {
        doc_comments::is_block_start(text, line).then(|| self.engine.borrow().docs.is_rendered(line))
    }

    /// Whether line `line` of `text` is the first line of a rendered doc comment, the one line of it that
    /// is on screen: how many bytes of indent it has, which is where its bar goes. `None` for a line
    /// that is not.
    pub fn doc_bar(&self, text: &TextSnapshot, line: usize) -> Option<usize> {
        self.engine
            .borrow()
            .docs
            .bar_indent(text, line)
    }

    /// The embedder that draws the rendered doc comments: each block as its markdown. `on_open` is
    /// called with the first line of a block when it is clicked.
    pub fn doc_embedder(&self, on_open: Rc<dyn Fn(usize)>) -> TextEmbedder {
        let engine = Rc::clone(&self.engine);
        TextEmbedder::windowed(move |text, range, _context| engine.borrow().docs.embeds(text, range, &on_open))
    }

    /// Shows as written the rendered doc comment that holds line `line`, if there is one: what typing
    /// into a rendered block does. Returns whether one was opened.
    pub fn open_doc_block_at(&self, text: &TextSnapshot, line: usize) -> bool {
        self.engine
            .borrow_mut()
            .docs
            .open_at(text, line)
    }

    /// Shows the doc comment that starts on `line` as written if it is rendered, and rendered if it is
    /// written.
    pub fn toggle_doc_block(&self, line: usize) {
        self.engine
            .borrow_mut()
            .docs
            .toggle(line);
    }

    /// The name of the item the byte `at` is inside of, for a breadcrumb: the innermost function,
    /// type, trait, module or `impl` that holds it, as `impl Trait for Type`, `fn name`, and so on.
    /// `None` outside every item.
    pub fn symbol_at(&self, text: &TextSnapshot, at: usize) -> Option<String> {
        self.engine
            .borrow_mut()
            .symbol_at(text, at)
    }

    /// The styled ranges of `range` of `text`, with nothing focused.
    #[cfg(test)]
    pub fn ranges(&self, text: &TextSnapshot, range: Range<usize>) -> Vec<StyledRange> {
        self.engine
            .borrow_mut()
            .ranges(text, range)
    }

    /// How many times the tree was made again from the one before it, and not from scratch.
    #[cfg(test)]
    fn incremental_parses(&self) -> usize {
        self.engine
            .borrow()
            .incremental_parses
    }
}

struct Engine {
    parser: Parser,
    query: Query,
    /// The style of each capture of the query, by its index; `None` leaves the text plain.
    styles: Vec<Option<SpanStyle>>,
    tree: Option<Tree>,
    /// The text `tree` is the parse of.
    text: TextSnapshot,
    incremental_parses: usize,
    /// The doc comments rendered as markdown, and which are shown as written.
    docs: DocRenderer,
}

/// A capture that was found: the bytes, its style and the pattern it came from. A later pattern
/// of the query wins where captures overlap, which is how a query makes the specific beat the
/// general.
struct Capture {
    range: Range<usize>,
    style: SpanStyle,
    pattern: usize,
}

impl Engine {
    fn ranges(&mut self, text: &TextSnapshot, range: Range<usize>) -> Vec<StyledRange> {
        if text.len() > MAX_HIGHLIGHTED_BYTES {
            self.tree = None;
            return Vec::new();
        }
        self.sync(text);
        let Some(tree) = &self.tree else {
            return Vec::new();
        };
        let range = range.start.min(text.len())..range.end.min(text.len());
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(range.clone());
        let mut found = Vec::new();
        let mut captures = cursor.captures(&self.query, tree.root_node(), |node: Node| {
            text.chunks(node.byte_range())
        });
        while let Some((matched, index)) = captures.next() {
            let capture = matched.captures[*index];
            if let Some(Some(style)) = self
                .styles
                .get(capture.index as usize)
            {
                found.push(Capture {
                    range: capture.node.start_byte()..capture.node.end_byte(),
                    style: style.clone(),
                    pattern: matched.pattern_index,
                });
            }
        }
        flatten(found)
    }

    fn symbol_at(&mut self, text: &TextSnapshot, at: usize) -> Option<String> {
        if text.len() > MAX_HIGHLIGHTED_BYTES {
            return None;
        }
        self.sync(text);
        let tree = self.tree.as_ref()?;
        let at = at.min(text.len());
        let mut node = tree
            .root_node()
            .descendant_for_byte_range(at, at)?;
        loop {
            if let Some(label) = symbol_label(&node, text) {
                return Some(label);
            }
            node = node.parent()?;
        }
    }

    /// Makes the tree the parse of `text`: from the tree of the text before it, told what changed,
    /// when there is one.
    fn sync(&mut self, text: &TextSnapshot) {
        if self.text.ptr_eq(text) && self.tree.is_some() {
            return;
        }
        let mut reuse = false;
        match (self.text.diff(text), self.tree.as_mut()) {
            (None, Some(_)) => {
                self.text = text.clone();
                return;
            }
            (Some((old, new)), Some(tree)) => {
                // The blocks shown as written stay on their lines: lines were added or taken out.
                let line = self.text.byte_to_line(old.start);
                let removed = self.text.slice(old.clone()).matches('\n').count();
                let inserted = text.slice(new.clone()).matches('\n').count();
                self.docs.edited(line, removed, inserted);
                tree.edit(&InputEdit {
                    start_byte: old.start,
                    old_end_byte: old.end,
                    new_end_byte: new.end,
                    start_position: point_at(&self.text, old.start),
                    old_end_position: point_at(&self.text, old.end),
                    new_end_position: point_at(text, new.end),
                });
                reuse = true;
            }
            _ => {}
        }
        let previous = if reuse { self.tree.as_ref() } else { None };
        let parsed = self.parser.parse_with_options(
            &mut |byte, _| {
                text.bytes_from(byte)
                    .unwrap_or(&[])
            },
            previous,
            None,
        );
        if reuse && parsed.is_some() {
            self.incremental_parses += 1;
        }
        self.tree = parsed;
        self.text = text.clone();
    }
}

/// What to call `node` in a breadcrumb, if it is an item that has a name.
fn symbol_label(node: &Node, text: &TextSnapshot) -> Option<String> {
    let (keyword, field) = match node.kind() {
        "function_item" => ("fn", "name"),
        "struct_item" => ("struct", "name"),
        "enum_item" => ("enum", "name"),
        "trait_item" => ("trait", "name"),
        "mod_item" => ("mod", "name"),
        "type_item" => ("type", "name"),
        "const_item" => ("const", "name"),
        "static_item" => ("static", "name"),
        "macro_definition" => ("macro", "name"),
        "impl_item" => ("impl", "type"),
        _ => return None,
    };
    let name = node.child_by_field_name(field)?;
    let name = text.slice(name.start_byte()..name.end_byte());
    if keyword != "impl" {
        return Some(format!("{keyword} {name}"));
    }
    // `impl Trait for Type` names both; `impl Type` the one.
    match node.child_by_field_name("trait") {
        Some(trait_node) => Some(format!(
            "impl {} for {name}",
            text.slice(trait_node.start_byte()..trait_node.end_byte())
        )),
        None => Some(format!("impl {name}")),
    }
}

/// Where the byte `at` is, as tree-sitter says it: the line, and the byte from the line's start.
fn point_at(text: &TextSnapshot, at: usize) -> Point {
    let row = text.byte_to_line(at);
    Point {
        row,
        column: at - text.line_range(row).start,
    }
}

/// Cuts overlapping captures into ranges that do not overlap, each with the style of the capture
/// that wins there (the one of the latest pattern, and of the innermost where patterns tie).
fn flatten(mut captures: Vec<Capture>) -> Vec<StyledRange> {
    if captures.is_empty() {
        return Vec::new();
    }
    captures.sort_by_key(|capture| (capture.range.start, std::cmp::Reverse(capture.range.end)));
    let mut edges: Vec<usize> = captures
        .iter()
        .flat_map(|capture| [capture.range.start, capture.range.end])
        .collect();
    edges.sort_unstable();
    edges.dedup();

    let mut out: Vec<StyledRange> = Vec::with_capacity(edges.len());
    let mut active: Vec<usize> = Vec::new();
    let mut next = 0;
    for pair in edges.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        while next < captures.len() && captures[next].range.start <= start {
            active.push(next);
            next += 1;
        }
        active.retain(|&index| captures[index].range.end > start);
        // Later patterns win; between captures of one pattern, the one that starts later is inside.
        let Some(&winner) = active
            .iter()
            .max_by_key(|&&index| (captures[index].pattern, captures[index].range.start))
        else {
            continue;
        };
        let style = &captures[winner].style;
        // A run of the same capture is one range.
        match out.last_mut() {
            Some(last) if last.range.end == start && last.style == *style => last.range.end = end,
            _ => out.push(StyledRange::new(start..end, style.clone())),
        }
    }
    out
}

#[cfg(test)]
mod tests;
