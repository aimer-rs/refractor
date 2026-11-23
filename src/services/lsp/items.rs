//! The items that the server offers to complete a word, as the editor takes them: what each is, what it puts
//! in the text, and where, with places as bytes of the text the request was made for.

use std::ops::Range;

use aimer::input::input::TextSnapshot;

use super::position::{self, Encoding};
use super::protocol::{Completions, ItemKind};
use serde_json::Value;

/// What the server says of a name: what it is and what is written of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hover {
    /// The words, as markdown: usually the signature in a fence, then the documentation.
    pub markdown: String,
    /// The part of the text they are about, as bytes of the text the request was made for, where the server says.
    pub range: Option<Range<usize>>,
}

/// What putting an item in the text does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    /// What the text replaces, as bytes of the text the request was made for. None for an item that does not
    /// say, which replaces the word before the caret.
    pub range: Option<Range<usize>>,
    pub text: String,
    /// The other changes it asks for, such as the line that imports it, in the same terms.
    pub extra: Vec<(Range<usize>, String)>,
}

/// One thing that the word being typed could be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    pub kind: ItemKind,
    /// What the server says of it: for a function, its signature.
    pub detail: Option<String>,
    /// What it says beside the label, such as where an item that is not in scope is imported from.
    pub label_detail: Option<String>,
    /// What the items are ordered by, where they are the same to the word typed: the server's own word for it.
    pub sort: String,
    /// What the word typed is compared with.
    pub filter: String,
    pub edit: Edit,
    /// What is sent to the server to have it give the other edits of the item, which it has still to say; none for
    /// an item that has nothing to resolve.
    pub resolve: Option<Value>,
    pub deprecated: bool,
}

/// The items of an answer, and whether more of the word would give others.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    pub items: Vec<Item>,
    pub incomplete: bool,
}

/// The items of `found`, with the places in them as bytes of `text` counted as `encoding` says.
pub fn answer_of(found: Completions, text: &TextSnapshot, encoding: Encoding) -> Answer {
    let bytes = |(from, to): (position::Position, position::Position)| {
        let start = position::byte_of(text, from, encoding);
        start..position::byte_of(text, to, encoding).max(start)
    };
    let items = found
        .items
        .into_iter()
        .map(|item| Item {
            sort: item.sort.unwrap_or_else(|| item.label.clone()),
            filter: item
                .filter
                .unwrap_or_else(|| item.label.clone()),
            label: item.label,
            kind: item.kind,
            detail: item.detail,
            label_detail: item.label_detail,
            edit: Edit {
                range: item.range.map(bytes),
                text: item.text,
                extra: item
                    .extra
                    .into_iter()
                    .map(|(range, text)| (bytes(range), text))
                    .collect(),
            },
            resolve: item.resolve,
            deprecated: item.deprecated,
        })
        .collect();
    Answer {
        items,
        incomplete: found.incomplete,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::lsp::position::Position;
    use crate::services::lsp::protocol::RawItem;

    fn raw(label: &str) -> RawItem {
        RawItem {
            label: label.to_owned(),
            kind: ItemKind::Function,
            detail: None,
            label_detail: None,
            sort: None,
            filter: None,
            text: label.to_owned(),
            range: None,
            extra: Vec::new(),
            resolve: None,
            deprecated: false,
        }
    }

    fn at(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    #[test]
    fn an_item_that_says_no_sort_or_filter_is_sorted_and_filtered_by_its_label() {
        let found = Completions {
            items: vec![
                raw("push"),
                RawItem {
                    sort: Some("09".into()),
                    filter: Some("pu".into()),
                    ..raw("pull")
                },
            ],
            incomplete: true,
        };
        let answer = answer_of(found, &TextSnapshot::from_text(""), Encoding::Utf8);
        assert!(answer.incomplete);
        assert_eq!(
            (
                answer.items[0].sort.as_str(),
                answer.items[0].filter.as_str()
            ),
            ("push", "push")
        );
        assert_eq!(
            (
                answer.items[1].sort.as_str(),
                answer.items[1].filter.as_str()
            ),
            ("09", "pu")
        );
        assert_eq!(
            answer.items[0].edit,
            Edit {
                range: None,
                text: "push".into(),
                extra: vec![]
            }
        );
    }

    #[test]
    fn the_places_of_an_item_are_bytes_of_the_text_in_utf_8_and_in_utf_16() {
        // The accent is two bytes and one unit, the emoji four bytes and two units.
        let text = TextSnapshot::from_text("a\u{e9}\u{1f600}bc\nsecond\n");
        let found = || Completions {
            items: vec![RawItem {
                range: Some((at(0, 4), at(0, 5))),
                extra: vec![((at(1, 0), at(1, 0)), "use x;\n".into())],
                ..raw("it")
            }],
            incomplete: false,
        };
        // The second line begins after the nine bytes of the first and its line break.
        let second = 10;
        let utf8 = answer_of(found(), &text, Encoding::Utf8);
        // In bytes, characters 4 and 5 are inside the emoji, which is bytes 3 to 7: each is at the start of it.
        assert_eq!(utf8.items[0].edit.range, Some(3..3));
        assert_eq!(
            utf8.items[0].edit.extra,
            vec![(second..second, "use x;\n".to_owned())]
        );
        let utf16 = answer_of(found(), &text, Encoding::Utf16);
        // In units, the same numbers are after a, the accent and the emoji (1 + 1 + 2): at b, and at c.
        assert_eq!(utf16.items[0].edit.range, Some(7..8));
        assert_eq!(
            utf16.items[0].edit.extra,
            vec![(second..second, "use x;\n".to_owned())]
        );
    }

    #[test]
    fn a_place_past_the_end_of_the_text_is_its_end() {
        let text = TextSnapshot::from_text("ab");
        let found = Completions {
            items: vec![RawItem {
                range: Some((at(9, 0), at(9, 9))),
                ..raw("x")
            }],
            incomplete: false,
        };
        let answer = answer_of(found, &text, Encoding::Utf8);
        assert_eq!(answer.items[0].edit.range, Some(2..2));
    }

    #[test]
    fn the_other_details_of_an_item_are_kept() {
        let found = Completions {
            items: vec![RawItem {
                kind: ItemKind::Struct,
                detail: Some("std::vec".into()),
                deprecated: true,
                ..raw("Vec")
            }],
            incomplete: false,
        };
        let item = answer_of(found, &TextSnapshot::from_text(""), Encoding::Utf16)
            .items
            .remove(0);
        assert_eq!(
            (
                item.kind,
                item.detail.as_deref(),
                item.deprecated,
                item.label.as_str()
            ),
            (ItemKind::Struct, Some("std::vec"), true, "Vec")
        );
    }
}
