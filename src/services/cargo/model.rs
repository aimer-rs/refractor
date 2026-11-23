//! The manifest of one document as it is asked about: the scan of its text, kept until the text changes, so that what is asked on every move of
//! the pointer costs a lookup and not a read of the file.

use std::cell::RefCell;
use std::rc::Rc;

use aimer::input::input::TextSnapshot;

use super::check::{Hover, Problem, check, hover_at};
use super::scan::Scan;
use super::schema::Schema;

/// The most bytes of a manifest that are read: a file past it is not one that anyone writes by hand.
pub const MAX_BYTES: usize = 512 * 1024;

/// What was read of a text, and the text it is the reading of.
#[derive(Default)]
pub struct Manifest {
    last: RefCell<Option<(TextSnapshot, Rc<Scan>)>>,
}

impl Manifest {
    /// The scan of `text`, which is the one kept where the text is the same; `None` for a text that is too large.
    pub fn scan(&self, text: &TextSnapshot) -> Option<Rc<Scan>> {
        if text.len() > MAX_BYTES {
            return None;
        }
        if let Some((kept, scan)) = self.last.borrow().as_ref()
            && kept.ptr_eq(text)
        {
            return Some(Rc::clone(scan));
        }
        let whole = text.slice(0..text.len());
        let scan = Rc::new(Scan::of(&whole));
        *self.last.borrow_mut() = Some((text.clone(), Rc::clone(&scan)));
        Some(scan)
    }

    /// What is wrong in `text`.
    pub fn problems(&self, text: &TextSnapshot) -> Vec<Problem> {
        self.scan(text)
            .map(|scan| check(Schema::cargo(), &scan.entries))
            .unwrap_or_default()
    }

    /// The card for the key at the byte `at` of `text`.
    pub fn hover(&self, text: &TextSnapshot, at: usize) -> Option<Hover> {
        let scan = self.scan(text)?;
        hover_at(Schema::cargo(), &scan.entries, at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scan_is_kept_for_the_same_text_and_made_again_for_another() {
        let manifest = Manifest::default();
        let text = TextSnapshot::from_text("[package]\nname = \"a\"\n");
        let first = manifest.scan(&text).expect("read");
        assert!(Rc::ptr_eq(&first, &manifest.scan(&text).expect("read")));
        let edited = text.with_replaced(0..0, "# c\n");
        assert!(!Rc::ptr_eq(&first, &manifest.scan(&edited).expect("read")));
    }

    #[test]
    fn problems_and_cards_are_read_from_the_text() {
        let manifest = Manifest::default();
        let text = TextSnapshot::from_text("[package]\nnme = 1\n");
        let problems = manifest.problems(&text);
        assert_eq!(problems.len(), 1);
        assert_eq!(&"[package]\nnme = 1\n"[problems[0].span.clone()], "nme");
        assert!(
            manifest
                .hover(&text, 2)
                .expect("a card")
                .markdown
                .starts_with("**package**")
        );
        assert!(
            manifest.hover(&text, 11).is_none(),
            "an unknown key has no words"
        );
    }

    #[test]
    fn a_text_that_is_too_large_is_not_read() {
        let manifest = Manifest::default();
        let text = TextSnapshot::from_text(&"# x\n".repeat(MAX_BYTES / 4 + 1));
        assert!(manifest.scan(&text).is_none());
        assert!(manifest.problems(&text).is_empty());
    }
}
