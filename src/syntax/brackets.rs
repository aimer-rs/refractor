//! Brackets, read from the parse tree: how deep each one is, which one goes with the one the caret is next to, and which
//! pair holds the caret.
//!
//! The tree is what tells a bracket from a character that looks like one: what is in a string, a comment or a character
//! literal is not a bracket and is not here. Brackets are the tokens `( ) [ ] { }` that are children of a node, and a
//! bracket is as deep as the openers among the siblings before it and before its ancestors, which is all it takes in a
//! grammar where a group is a node, such as a block, an argument list, a token tree of a macro or an attribute.

use std::ops::Range;

use arborium::tree_sitter::{Node, Tree};

/// A bracket and the one that goes with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pair {
    pub open: Range<usize>,
    pub close: Range<usize>,
    /// How many brackets hold the pair: the pair itself is at this depth, and what is inside it is one deeper.
    pub depth: usize,
}

/// What a token is, if it is a bracket.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Open,
    Close,
}

/// Where the brackets are, and how deep.
pub struct Brackets;

impl Brackets {
    /// How deep a tree is followed. Code nested further than this is not code anyone is reading, and a tree that deep is
    /// a tree of something that is not.
    const MAX_LEVELS: usize = 256;

    /// The brackets that have a byte in `region`, each with its range and its depth, in order down the text.
    pub fn depths(tree: &Tree, region: Range<usize>) -> Vec<(Range<usize>, usize)> {
        let mut found = Vec::new();
        if !region.is_empty() {
            Self::collect(tree.root_node(), &region, 0, 0, &mut found);
        }
        found
    }

    fn collect(
        node: Node,
        region: &Range<usize>,
        depth: usize,
        level: usize,
        found: &mut Vec<(Range<usize>, usize)>,
    ) {
        if level >= Self::MAX_LEVELS {
            return;
        }
        // How many of this node's own brackets are open at the child being looked at.
        let mut open = 0usize;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            let within = child.start_byte() < region.end && child.end_byte() > region.start;
            match Self::kind_of(child) {
                Some(Kind::Open) => {
                    if within {
                        found.push((child.start_byte()..child.end_byte(), depth + open));
                    }
                    open += 1;
                }
                Some(Kind::Close) => {
                    open = open.saturating_sub(1);
                    if within {
                        found.push((child.start_byte()..child.end_byte(), depth + open));
                    }
                }
                None if within && child.child_count() > 0 => {
                    Self::collect(child, region, depth + open, level + 1, found)
                }
                None => {}
            }
        }
    }

    /// The pair that the caret at `caret` is beside: a bracket before it, or else one after it. `None` where it is not beside
    /// a bracket, or beside one that has no other half.
    pub fn adjacent(tree: &Tree, caret: usize) -> Option<Pair> {
        let before = caret
            .checked_sub(1)
            .and_then(|at| Self::token_at(tree, at));
        let token = before.or_else(|| Self::token_at(tree, caret))?;
        Self::pair_of(token)
    }

    /// The innermost pair that holds the caret, which may be right after its opener or right before its closer.
    pub fn enclosing(tree: &Tree, caret: usize) -> Option<Pair> {
        let mut node = tree
            .root_node()
            .descendant_for_byte_range(caret, caret)?;
        loop {
            if let Some(pair) = Self::innermost_in(node, caret) {
                return Some(pair);
            }
            node = node.parent()?;
        }
    }

    /// The innermost of the pairs among the children of `node` that hold `caret`.
    fn innermost_in(node: Node, caret: usize) -> Option<Pair> {
        let mut cursor = node.walk();
        let mut stack: Vec<Node> = Vec::new();
        let mut best: Option<(Node, Node)> = None;
        for child in node.children(&mut cursor) {
            match Self::kind_of(child) {
                Some(Kind::Open) => stack.push(child),
                Some(Kind::Close) => {
                    let Some(open) = stack.pop() else { continue };
                    if open.end_byte() <= caret && caret <= child.start_byte() {
                        // A pair that is inside another is found first: the innermost is the one that begins last.
                        if best.is_none_or(|(held, _)| open.start_byte() >= held.start_byte()) {
                            best = Some((open, child));
                        }
                    }
                }
                None => {}
            }
        }
        let (open, close) = best?;
        Some(Pair {
            open: open.start_byte()..open.end_byte(),
            close: close.start_byte()..close.end_byte(),
            depth: Self::depth_of(open),
        })
    }

    /// The bracket token that is the byte at `at`, if that is one.
    fn token_at(tree: &Tree, at: usize) -> Option<Node<'_>> {
        let node = tree
            .root_node()
            .descendant_for_byte_range(at, at + 1)?;
        (node.start_byte() == at && node.end_byte() == at + 1 && Self::kind_of(node).is_some())
            .then_some(node)
    }

    /// The pair that `token` is half of: the bracket among its siblings that closes it, or that it closes, with the ones between
    /// them counted.
    fn pair_of(token: Node) -> Option<Pair> {
        let parent = token.parent()?;
        let kind = Self::kind_of(token)?;
        let mut cursor = parent.walk();
        let siblings: Vec<Node> = parent.children(&mut cursor).collect();
        let index = siblings
            .iter()
            .position(|sibling| sibling.id() == token.id())?;
        let other = match kind {
            Kind::Open => {
                let mut open = 0usize;
                siblings[index + 1..]
                    .iter()
                    .find(|sibling| match Self::kind_of(**sibling) {
                        Some(Kind::Open) => {
                            open += 1;
                            false
                        }
                        Some(Kind::Close) if open == 0 => true,
                        Some(Kind::Close) => {
                            open -= 1;
                            false
                        }
                        None => false,
                    })?
            }
            Kind::Close => {
                let mut closed = 0usize;
                siblings[..index]
                    .iter()
                    .rev()
                    .find(|sibling| match Self::kind_of(**sibling) {
                        Some(Kind::Close) => {
                            closed += 1;
                            false
                        }
                        Some(Kind::Open) if closed == 0 => true,
                        Some(Kind::Open) => {
                            closed -= 1;
                            false
                        }
                        None => false,
                    })?
            }
        };
        let (open, close) = if kind == Kind::Open {
            (token, *other)
        } else {
            (*other, token)
        };
        Some(Pair {
            open: open.start_byte()..open.end_byte(),
            close: close.start_byte()..close.end_byte(),
            depth: Self::depth_of(open),
        })
    }

    /// How deep the opener `open` is: the openers that are open at it among the siblings before it, and before each of its
    /// ancestors, which is what [`Self::depths`] says of it.
    fn depth_of(open: Node) -> usize {
        let mut depth = 0usize;
        let mut node = open;
        while let Some(parent) = node.parent() {
            let mut cursor = parent.walk();
            let mut count = 0usize;
            for sibling in parent.children(&mut cursor) {
                if sibling.id() == node.id() {
                    break;
                }
                match Self::kind_of(sibling) {
                    Some(Kind::Open) => count += 1,
                    Some(Kind::Close) => count = count.saturating_sub(1),
                    None => {}
                }
            }
            depth += count;
            node = parent;
        }
        depth
    }

    /// Whether `node` is a bracket token, and which half.
    fn kind_of(node: Node) -> Option<Kind> {
        if node.is_named() || node.child_count() > 0 {
            return None;
        }
        match node.kind() {
            "(" | "[" | "{" => Some(Kind::Open),
            ")" | "]" | "}" => Some(Kind::Close),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
