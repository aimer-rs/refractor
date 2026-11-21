//! Syntax highlighting for the text on screen.
//!
//! A [`SyntaxHighlighter`] keeps the tree-sitter parse of a document and makes it again, from the
//! previous tree, after each edit, so a keystroke costs the part of the tree it changed and not the
//! file. The parser reads the text straight out of the editor's rope a chunk at a time, so no copy
//! of the file is made to parse it. The editor asks for the styles of the lines it is about to draw,
//! and only the captures of those lines are looked up.

mod brackets;
mod cache;
mod palette;

use crate::doc_comments::{self, DocRenderer};

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use self::brackets::Brackets;
use self::cache::{HighlightCache, clip};

use aimer::MarkdownTheme;
use aimer::input::input::{
    HighlightContext, SpanStyle, StyledRange, TextEmbedder, TextHighlighter, TextSnapshot,
};
use arborium::tree_sitter::{
    InputEdit, Language, Node, Parser, Point, Query, QueryCursor, StreamingIterator, Tree,
};

pub use self::brackets::Pair;
pub use self::palette::SyntaxPalette;

/// The longest text that is highlighted, in bytes. A file past it is shown plain: parsing it whole
/// the first time would be a pause the user would see, and it is not code anyone edits.
const MAX_HIGHLIGHTED_BYTES: usize = 4 * 1024 * 1024;

/// The most bytes of a text whose highlight is kept: a long file that is scrolled through is held a
/// window at a time and not whole.
const MAX_CACHED_BYTES: usize = 512 * 1024;

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
        let styles = capture_styles(&query, &palette);
        let docs = DocRenderer::new(crate::markdown::doc_theme(crate::settings::ThemeMode::Dark));
        Self {
            engine: Rc::new(RefCell::new(Engine {
                parser,
                query,
                styles,
                palette,
                tree: None,
                text: TextSnapshot::new(),
                incremental_parses: 0,
                cache: None,
                queried_bytes: 0,
                docs,
            })),
        }
    }

    /// Colors the syntax with `palette` from now on. What was highlighted is dropped, so the next draw
    /// asks again; the parse is kept, so nothing is parsed again. The same palette changes nothing.
    pub fn set_palette(&self, palette: SyntaxPalette) {
        let mut engine = self.engine.borrow_mut();
        if engine.palette == palette {
            return;
        }
        engine.styles = capture_styles(&engine.query, &palette);
        engine.palette = palette;
        engine.cache = None;
    }

    /// Measures the indent of the rendered doc comments in code of `size` logical pixels, which is the size the code
    /// beside them is set in.
    pub fn set_code_size(&self, size: f32) {
        self.engine
            .borrow_mut()
            .docs
            .set_code_size(size);
    }

    /// Limits the text of the rendered doc comments to `width` logical pixels, so a long line of comment
    /// wraps to a readable column; `None` lets them take the width of the editor. See
    /// [`DocRenderer::set_max_width`].
    pub fn set_doc_max_width(&self, width: Option<f32>) {
        self.engine
            .borrow_mut()
            .docs
            .set_max_width(width);
    }

    /// Calls `handler` with the link when one in a rendered doc comment is clicked. See [`DocRenderer::set_on_link`].
    pub fn set_doc_on_link(&self, handler: impl Fn(&crate::doc_comments::DocLink) + 'static) {
        self.engine
            .borrow_mut()
            .docs
            .set_on_link(handler);
    }

    /// Draws the rendered doc comments with `theme` from now on.
    pub fn set_doc_theme(&self, theme: MarkdownTheme) {
        self.engine.borrow_mut().docs.set_theme(theme);
    }

    /// The highlighter to give a text area: it is asked for the styles of the lines being drawn, and
    /// parses what changed since it was last asked.
    pub fn highlighter(&self) -> TextHighlighter {
        let engine = Rc::clone(&self.engine);
        TextHighlighter::windowed(move |text, range, _context: &HighlightContext| {
            engine.borrow_mut().ranges(text, range)
        })
    }

    /// [`highlighter`](Self::highlighter) with what the grammar says of a part of the text given to `over`, with the text and the part,
    /// to say what is drawn instead: where something that knows more of the names, a language server, draws over it.
    ///
    /// `over` runs whenever the grammar's runs of a part are asked for, so it is as cheap as it has to be for the lines on screen.
    pub fn highlighter_over(
        &self,
        over: impl Fn(Vec<StyledRange>, &TextSnapshot, Range<usize>) -> Vec<StyledRange> + 'static,
    ) -> TextHighlighter {
        let engine = Rc::clone(&self.engine);
        TextHighlighter::windowed(move |text, range, _context: &HighlightContext| {
            let runs = engine.borrow_mut().ranges(text, range.clone());
            over(runs, text, range)
        })
    }

    /// Whether line `line` starts a doc comment, and if it does, whether the block is rendered
    /// (`Some(true)`) or shown as written: the state the button in the margin shows.
    pub fn doc_block_at(&self, text: &TextSnapshot, line: usize) -> Option<bool> {
        doc_comments::is_block_start(text, line)
            .then(|| self.engine.borrow().docs.is_rendered(line))
    }

    /// Whether line `line` of `text` is the first line of a rendered doc comment, the one line of it that
    /// is on screen: how many bytes of indent it has, which is where its bar goes. `None` for a line
    /// that is not.
    pub fn doc_bar(&self, text: &TextSnapshot, line: usize) -> Option<usize> {
        self.engine.borrow().docs.bar_indent(text, line)
    }

    /// The embedder that draws the rendered doc comments: each block as its markdown.
    pub fn doc_embedder(&self) -> TextEmbedder {
        let engine = Rc::clone(&self.engine);
        TextEmbedder::windowed(move |text, range, _context| {
            engine.borrow().docs.embeds(text, range)
        })
    }

    /// Whether an edit that removes the bytes `removed` of `text` (inserts at its start, when it is
    /// empty) would change a rendered doc comment, which cannot be edited while it is rendered.
    pub fn protects(&self, text: &TextSnapshot, removed: Range<usize>) -> bool {
        self.engine
            .borrow()
            .docs
            .protects(text, removed)
    }

    /// Whether Backspace (`backwards`) or Delete with the selection `selection` would change a
    /// rendered doc comment; see [`DocRenderer::refuses_deletion`].
    pub fn refuses_deletion(
        &self,
        text: &TextSnapshot,
        selection: Range<usize>,
        backwards: bool,
    ) -> bool {
        self.engine
            .borrow()
            .docs
            .refuses_deletion(text, selection, backwards)
    }

    /// Shows the doc comment that starts on `line` as written if it is rendered, and rendered if it is
    /// written.
    pub fn toggle_doc_block(&self, line: usize) {
        self.engine.borrow_mut().docs.toggle(line);
    }

    /// The name of the item the byte `at` is inside of, for a breadcrumb: the innermost function,
    /// type, trait, module or `impl` that holds it, as `impl Trait for Type`, `fn name`, and so on.
    /// `None` outside every item.
    pub fn symbol_at(&self, text: &TextSnapshot, at: usize) -> Option<String> {
        self.engine.borrow_mut().symbol_at(text, at)
    }

    /// The bytes of the name the byte `at` is in, when it is a name that something is defined as and used by: an
    /// identifier, a type, a field. `None` for a comment, a string, a raw string, a keyword, a number, and for what is
    /// beside the name. It is what the jump key shows as a link.
    pub fn link_at(&self, text: &TextSnapshot, at: usize) -> Option<Range<usize>> {
        self.engine.borrow_mut().link_at(text, at)
    }

    /// The pair of brackets that the caret at `at` is beside, as the parse of `text` has it: a bracket before it, or else
    /// one after it. `None` where it is beside none, or where the text is not parsed.
    pub fn pair_beside(&self, text: &TextSnapshot, at: usize) -> Option<Pair> {
        self.engine
            .borrow_mut()
            .with_tree(text, |tree| Brackets::adjacent(tree, at.min(text.len())))
    }

    /// The innermost pair of brackets that holds the caret at `at`.
    pub fn pair_around(&self, text: &TextSnapshot, at: usize) -> Option<Pair> {
        self.engine
            .borrow_mut()
            .with_tree(text, |tree| Brackets::enclosing(tree, at.min(text.len())))
    }

    /// The styled ranges of a line of code that is shown on its own, such as one of the lines that use a name. It is parsed as it
    /// is, and so is a fragment where the line is: the colors are those of what the parse makes of it. Use a highlighter of its
    /// own for this and not a document's, whose parse each line would replace.
    pub fn snippet(&self, text: &str) -> Vec<StyledRange> {
        let text = TextSnapshot::from_text(text);
        let len = text.len();
        self.engine.borrow_mut().ranges(&text, 0..len)
    }

    /// The styled ranges of `range` of `text`, with nothing focused.
    #[cfg(test)]
    pub fn ranges(&self, text: &TextSnapshot, range: Range<usize>) -> Vec<StyledRange> {
        self.engine.borrow_mut().ranges(text, range)
    }

    /// The styled ranges of `range` as a query of it gives them, without the kept highlight.
    #[cfg(test)]
    pub fn uncached_ranges(&self, text: &TextSnapshot, range: Range<usize>) -> Vec<StyledRange> {
        let mut engine = self.engine.borrow_mut();
        engine.sync(text);
        let range = range.start.min(text.len())..range.end.min(text.len());
        let found = engine.query(text, range.clone());
        engine.rainbowed(clip(found, &range), range)
    }

    /// How many bytes of the text tree-sitter was asked about.
    #[cfg(test)]
    fn queried_bytes(&self) -> usize {
        self.engine.borrow().queried_bytes
    }

    /// How many times the tree was made again from the one before it, and not from scratch.
    #[cfg(test)]
    fn incremental_parses(&self) -> usize {
        self.engine.borrow().incremental_parses
    }
}

/// The style `palette` gives each capture of `query`, by the index of the capture.
fn capture_styles(query: &Query, palette: &SyntaxPalette) -> Vec<Option<SpanStyle>> {
    query
        .capture_names()
        .iter()
        .map(|name| palette.style_for(name))
        .collect()
}

struct Engine {
    parser: Parser,
    query: Query,
    /// The style of each capture of the query, by its index; `None` leaves the text plain.
    styles: Vec<Option<SpanStyle>>,
    /// The palette `styles` were made from.
    palette: SyntaxPalette,
    tree: Option<Tree>,
    /// The text `tree` is the parse of.
    text: TextSnapshot,
    incremental_parses: usize,
    /// The highlight of the part of the text that was asked for, moved over the edits since.
    cache: Option<HighlightCache>,
    /// How many bytes of the text were asked tree-sitter about, which is what the cache is for.
    queried_bytes: usize,
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
    /// The styled ranges of `range`: the runs that have a byte in it, from what is kept of the highlight
    /// and asked of tree-sitter only for the part that is not kept.
    fn ranges(&mut self, text: &TextSnapshot, range: Range<usize>) -> Vec<StyledRange> {
        if text.len() > MAX_HIGHLIGHTED_BYTES {
            self.tree = None;
            self.cache = None;
            return Vec::new();
        }
        self.sync(text);
        if self.tree.is_none() {
            return Vec::new();
        }
        let range = range.start.min(text.len())..range.end.min(text.len());
        if range.is_empty() {
            return Vec::new();
        }
        self.cover(text, &range);
        let kept = self
            .cache
            .as_ref()
            .map_or_else(Vec::new, |cache| cache.ranges_in(range.clone()));
        self.rainbowed(kept, range)
    }

    /// `ranges`, the runs of `region`, with the brackets in it in the colors of their depth.
    ///
    /// How deep a bracket is depends on the brackets around it, which an edit far from it can change, so the colors are not
    /// kept with the runs: they are made from the tree as it is, for the part that is asked about.
    fn rainbowed(&self, ranges: Vec<StyledRange>, region: Range<usize>) -> Vec<StyledRange> {
        let Some(tree) = &self.tree else {
            return ranges;
        };
        // A run that reaches out of `region` may be several brackets, which are one run while they are the same color: they
        // are told apart over the whole of it, and the pieces that are not in `region` are dropped after.
        let reach = ranges
            .first()
            .map_or(region.start, |run| run.range.start.min(region.start))
            ..ranges
                .last()
                .map_or(region.end, |run| run.range.end.max(region.end));
        let brackets: Vec<(Range<usize>, SpanStyle)> = Brackets::depths(tree, reach)
            .into_iter()
            .map(|(range, depth)| (range, SpanStyle::new().color(self.palette.rainbow(depth))))
            .collect();
        let mut ranges = with_brackets(ranges, brackets);
        ranges.retain(|run| run.range.start < region.end && run.range.end > region.start);
        ranges
    }

    /// Makes the kept highlight cover `range`: asks about the parts of it that it does not.
    fn cover(&mut self, text: &TextSnapshot, range: &Range<usize>) {
        let Some(mut cache) = self.cache.take() else {
            self.cache = Some(self.fresh_cache(text, range));
            return;
        };
        let covered = cache.covered();
        if covered.start <= range.start && range.end <= covered.end {
            self.cache = Some(cache);
            return;
        }
        let touches = range.start <= covered.end && range.end >= covered.start;
        let hull = covered.start.min(range.start)..covered.end.max(range.end);
        if !touches || hull.len() > MAX_CACHED_BYTES {
            // A window far from the last one, or too much to keep: a new start.
            self.cache = Some(self.fresh_cache(text, range));
            return;
        }
        if range.start < covered.start {
            let part = range.start..covered.start;
            let fresh = self.query_clipped(text, &part);
            cache.extend(part, fresh);
        }
        if range.end > covered.end {
            let part = covered.end..range.end;
            let fresh = self.query_clipped(text, &part);
            cache.extend(part, fresh);
        }
        self.cache = Some(cache);
    }

    fn fresh_cache(&mut self, text: &TextSnapshot, range: &Range<usize>) -> HighlightCache {
        let fresh = self.query_clipped(text, range);
        HighlightCache::new(range.clone(), fresh)
    }

    /// The runs of `region`, from tree-sitter, cut at its edges.
    fn query_clipped(&mut self, text: &TextSnapshot, region: &Range<usize>) -> Vec<StyledRange> {
        let found = self.query(text, region.clone());
        clip(found, region)
    }

    /// The runs of the captures that have a byte in `region`, as tree-sitter gives them: the whole of a
    /// run that reaches out of `region` is in them.
    fn query(&mut self, text: &TextSnapshot, region: Range<usize>) -> Vec<StyledRange> {
        let Some(tree) = &self.tree else {
            return Vec::new();
        };
        self.queried_bytes += region.len();
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(region);
        let mut found = Vec::new();
        let mut captures = cursor.captures(&self.query, tree.root_node(), |node: Node| {
            text.chunks(node.byte_range())
        });
        while let Some((matched, index)) = captures.next() {
            let capture = matched.captures[*index];
            if let Some(Some(style)) = self.styles.get(capture.index as usize) {
                found.push(Capture {
                    range: capture.node.start_byte()..capture.node.end_byte(),
                    style: style.clone(),
                    pattern: matched.pattern_index,
                });
            }
        }
        flatten(found)
    }

    /// Moves the kept highlight over the edit that made `text` of the text before it, `old` bytes of the
    /// one replaced by `new` of this, and asks again about what the edit can have changed: the lines it
    /// touched and the next to them, and the parts of the tree that changed shape, which an edit that
    /// opens a string or a comment reaches far from itself.
    fn keep_cache_over(
        &mut self,
        text: &TextSnapshot,
        old: Range<usize>,
        new: Range<usize>,
        changed: Vec<Range<usize>>,
    ) {
        let Some(mut cache) = self.cache.take() else {
            return;
        };
        cache.edited(old, new.len());
        let covered = cache.covered();
        if covered.is_empty() || text.len() == 0 {
            return;
        }
        let last_line = text.line_count() - 1;
        let mut regions: Vec<Range<usize>> = Vec::new();
        for dirty in changed.into_iter().chain([new]) {
            let start = dirty.start.min(text.len());
            let end = dirty.end.min(text.len()).max(start);
            let first = text.byte_to_line(start).saturating_sub(1);
            let last =
                (text.byte_to_line(if end > start { end - 1 } else { start }) + 1).min(last_line);
            let region = text.line_range(first).start.max(covered.start)
                ..text.line_range(last).end.min(covered.end);
            if region.start < region.end {
                regions.push(region);
            }
        }
        regions.sort_by_key(|region| region.start);
        let mut merged: Vec<Range<usize>> = Vec::new();
        for region in regions {
            match merged.last_mut() {
                Some(last) if region.start <= last.end => last.end = last.end.max(region.end),
                _ => merged.push(region),
            }
        }
        for region in merged {
            let fresh = self.query_clipped(text, &region);
            cache.replace(region, fresh);
        }
        self.cache = Some(cache);
    }

    /// Reads the parse of `text`, made current first: `None` for a text that is not parsed.
    fn with_tree<T>(
        &mut self,
        text: &TextSnapshot,
        read: impl FnOnce(&Tree) -> Option<T>,
    ) -> Option<T> {
        if text.len() > MAX_HIGHLIGHTED_BYTES {
            return None;
        }
        self.sync(text);
        read(self.tree.as_ref()?)
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

    fn link_at(&mut self, text: &TextSnapshot, at: usize) -> Option<Range<usize>> {
        if at >= text.len() || text.len() > MAX_HIGHLIGHTED_BYTES {
            return None;
        }
        self.sync(text);
        let node = self
            .tree
            .as_ref()?
            .root_node()
            .descendant_for_byte_range(at, at + 1)?;
        // The leaf the byte is in, and only the kinds of leaf that name something. A string and a comment are leaves of
        // their own kinds, and a keyword has no named kind at all, so none of them is in this list.
        matches!(
            node.kind(),
            "identifier" | "type_identifier" | "field_identifier" | "shorthand_field_identifier"
        )
        .then(|| node.byte_range())
    }

    /// Makes the tree the parse of `text`: from the tree of the text before it, told what changed,
    /// when there is one.
    fn sync(&mut self, text: &TextSnapshot) {
        if self.text.ptr_eq(text) && self.tree.is_some() {
            return;
        }
        let mut reuse = false;
        let mut edit = None;
        match (self.text.diff(text), self.tree.as_mut()) {
            (None, Some(_)) => {
                self.text = text.clone();
                return;
            }
            (Some((old, new)), Some(tree)) => {
                // The blocks shown as written stay on their lines: lines were added or taken out.
                let line = self.text.byte_to_line(old.start);
                let removed = self
                    .text
                    .slice(old.clone())
                    .matches('\n')
                    .count();
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
                edit = Some((old, new));
            }
            _ => {}
        }
        let previous = if reuse { self.tree.as_ref() } else { None };
        let parsed = self.parser.parse_with_options(
            &mut |byte, _| text.bytes_from(byte).unwrap_or(&[]),
            previous,
            None,
        );
        // Where the shape of the tree is other than the one the edit was made to.
        let changed: Vec<Range<usize>> = match (&parsed, previous) {
            (Some(tree), Some(before)) => tree
                .changed_ranges(before)
                .map(|range| range.start_byte..range.end_byte)
                .collect(),
            _ => Vec::new(),
        };
        if reuse && parsed.is_some() {
            self.incremental_parses += 1;
        }
        self.tree = parsed;
        self.text = text.clone();
        match edit {
            Some((old, new)) if reuse && self.tree.is_some() => {
                self.keep_cache_over(text, old, new, changed)
            }
            // A tree made from nothing says nothing of what the old one held.
            _ => self.cache = None,
        }
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

/// `ranges`, which are in order and do not overlap, with each of `brackets`, which are too, as a run of its own: a run that
/// has one in it is cut round it, and one that is in no run is a run by itself.
fn with_brackets(
    ranges: Vec<StyledRange>,
    brackets: Vec<(Range<usize>, SpanStyle)>,
) -> Vec<StyledRange> {
    if brackets.is_empty() {
        return ranges;
    }
    let mut out: Vec<StyledRange> = Vec::with_capacity(ranges.len() + brackets.len() * 2);
    let mut next = 0;
    for run in ranges {
        let mut start = run.range.start;
        // Brackets that lie before the run are in no run.
        while next < brackets.len() && brackets[next].0.end <= start {
            out.push(StyledRange::new(
                brackets[next].0.clone(),
                brackets[next].1.clone(),
            ));
            next += 1;
        }
        while next < brackets.len() && brackets[next].0.start < run.range.end {
            let (bracket, style) = &brackets[next];
            if bracket.start > start {
                out.push(StyledRange::new(start..bracket.start, run.style.clone()));
            }
            out.push(StyledRange::new(bracket.clone(), style.clone()));
            start = bracket.end.min(run.range.end);
            next += 1;
        }
        if start < run.range.end {
            out.push(StyledRange::new(start..run.range.end, run.style.clone()));
        }
    }
    for (bracket, style) in &brackets[next..] {
        out.push(StyledRange::new(bracket.clone(), style.clone()));
    }
    out
}

#[cfg(test)]
mod tests;
