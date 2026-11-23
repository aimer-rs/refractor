//! The manifest of a folder as the source of the items that complete a word: what the schema says could be written at the caret.

use std::rc::Rc;

use aimer::input::TextEditingController;

use super::complete::{Candidate, Place, complete};
use super::scan::Scan;
use super::schema::Schema;
use crate::model::completion::Source;
use crate::services::lsp::{Answer, Edit, Item, ItemKind};

/// The items for the caret of one manifest.
pub struct ManifestSource {
    controller: TextEditingController,
}

impl ManifestSource {
    /// The most bytes of a manifest that are read to answer: a file past it is not one that anyone writes by hand.
    pub const MAX_BYTES: usize = 512 * 1024;

    pub fn new(controller: TextEditingController) -> Rc<dyn Source> {
        Rc::new(Self { controller })
    }

    /// What could be put at the caret now; `None` where there is nothing.
    pub fn answer(&self) -> Option<Answer> {
        let snapshot = self.controller.snapshot();
        if snapshot.len() > Self::MAX_BYTES {
            return None;
        }
        let caret = self.controller.value().selection().focus();
        let text = snapshot.slice(0..snapshot.len());
        let found = complete(Schema::cargo(), &text, &Scan::of(&text).entries, caret)?;
        let kind = match found.place {
            Place::Table => ItemKind::Module,
            Place::Key => ItemKind::Property,
            Place::Value => ItemKind::Value,
        };
        let items = found
            .items
            .into_iter()
            .map(|candidate| item(candidate, kind))
            .collect();
        Some(Answer {
            items,
            incomplete: false,
        })
    }
}

fn item(candidate: Candidate, kind: ItemKind) -> Item {
    Item {
        sort: candidate.label.clone(),
        filter: candidate.label.clone(),
        label: candidate.label,
        kind,
        detail: Some(candidate.detail).filter(|detail| !detail.is_empty()),
        label_detail: None,
        edit: Edit {
            range: None,
            text: candidate.insert,
            extra: Vec::new(),
        },
        resolve: None,
        deprecated: false,
    }
}

impl Source for ManifestSource {
    fn request(&self, done: Box<dyn FnOnce(Option<Answer>)>) {
        done(self.answer());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn source(text: &str) -> (TextEditingController, ManifestSource) {
        let controller = TextEditingController::with_str(text);
        (controller.clone(), ManifestSource { controller })
    }

    #[test]
    fn the_items_are_those_of_the_schema_at_the_caret() {
        let (_, source) = source("[package]\nname = \"a\"\ned");
        let answer = source.answer().expect("offered");
        let edition = answer
            .items
            .iter()
            .find(|item| item.label == "edition")
            .expect("edition");
        assert_eq!(edition.kind, ItemKind::Property);
        assert_eq!(edition.edit.text, "edition");
        assert!(
            answer
                .items
                .iter()
                .all(|item| item.label != "name")
        );
        assert!(!answer.incomplete);
    }

    #[test]
    fn the_request_is_answered_at_once_and_a_value_is_put_with_its_quotes() {
        let (_, source) = source("[profile.release]\nopt-level = ");
        let heard = Rc::new(Cell::new(false));
        let seen = Rc::clone(&heard);
        source.request(Box::new(move |answer| {
            let answer = answer.expect("offered");
            let s = answer
                .items
                .iter()
                .find(|item| item.label == "s")
                .expect("s");
            assert_eq!(s.edit.text, "\"s\"");
            seen.set(true);
        }));
        assert!(heard.get());
    }

    #[test]
    fn a_manifest_that_is_too_large_is_not_read() {
        let (_, source) = source(&"# x\n".repeat(ManifestSource::MAX_BYTES / 4 + 1));
        assert!(source.answer().is_none());
    }
}
