//! The keywords of Rust as items, for where the language server has nothing to say: before it has started, or
//! when it cannot be run.

use crate::lsp::{Edit, Item, ItemKind};

/// The keywords of Rust, strict and reserved.
const RUST_KEYWORDS: &[&str] = &[
    "Self", "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
    "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "if", "impl", "in", "let",
    "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref", "return", "self",
    "static", "struct", "super", "trait", "true", "try", "type", "typeof", "unsafe", "unsized", "use",
    "virtual", "where", "while", "yield",
];

/// The keywords as items. The shorter is the likelier, so the sort text begins with the length.
pub fn rust_keywords() -> Vec<Item> {
    RUST_KEYWORDS
        .iter()
        .map(|keyword| Item {
            label: (*keyword).to_owned(),
            kind: ItemKind::Keyword,
            detail: Some("keyword".to_owned()),
            label_detail: None,
            sort: format!("{:02}{keyword}", keyword.len()),
            filter: (*keyword).to_owned(),
            edit: Edit { range: None, text: (*keyword).to_owned(), extra: Vec::new() },
            resolve: None,
            deprecated: false,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_keyword_is_an_item_that_puts_itself_in() {
        let items = rust_keywords();
        assert_eq!(items.len(), RUST_KEYWORDS.len());
        assert!(items.iter().all(|item| item.edit.text == item.label && item.kind == ItemKind::Keyword && item.edit.range.is_none()));
        assert!(items.iter().any(|item| item.label == "fn"));
    }

    #[test]
    fn a_shorter_keyword_sorts_before_a_longer_one_and_alphabetically_among_those_as_long() {
        let sort_of = |label: &str| rust_keywords().into_iter().find(|item| item.label == label).unwrap().sort;
        assert!(sort_of("fn") < sort_of("for"));
        assert!(sort_of("for") < sort_of("false"));
        assert!(sort_of("false") < sort_of("final"));
    }
}
