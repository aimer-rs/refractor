//! The question that a rename asks: what is the name to be called. It is open beside the name that was chosen, with the name in it and all of
//! it selected, and is answered with Return or given up with Escape.
//!
//! This is only the state of the question and the rules for an answer; the widget that draws it is the card of the editor, and the asking of
//! the language server is the pane's, which has it.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use aimer::input::TextEditingController;
use aimer::input::input::FieldSpot;
use aimer::{FocusNode, Venus};

use crate::ui::components::popup::Redraw;

/// What is asked: which name, where the question hangs from, and what is typed in answer.
pub struct Asking {
    /// The byte where the name starts, in the text it was chosen in.
    pub at: usize,
    pub old: String,
    pub spot: FieldSpot,
    /// What is typed: the name, to begin with.
    pub controller: TextEditingController,
    /// The focus of the field that takes what is typed.
    pub focus: FocusNode,
}

/// What an answer is worth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// It is the name that there is already: there is nothing to rename.
    Same,
    /// It cannot be a name, and why.
    Invalid(&'static str),
    /// A name to rename to.
    Name(String),
}

/// Whether `typed` is a name to rename `old` to. The blanks round it are not part of it. A name is an identifier, as Rust writes them, and
/// a raw one, `r#type`, is: whether it is one that the language takes where it is put is for the language server to say.
pub fn judge(old: &str, typed: &str) -> Verdict {
    let typed = typed.trim();
    if typed.is_empty() {
        return Verdict::Invalid(RenamePrompt::EMPTY);
    }
    if typed == old {
        return Verdict::Same;
    }
    let word = typed.strip_prefix("r#").unwrap_or(typed);
    let mut characters = word.chars();
    let first = characters
        .next()
        .is_some_and(|first| first == '_' || first.is_alphabetic());
    // Letters, digits, marks of letters and the line: a mark is what a letter of many scripts is written with.
    if first
        && characters
            .all(|character| character == '_' || character.is_alphanumeric() || is_mark(character))
    {
        Verdict::Name(typed.to_owned())
    } else {
        Verdict::Invalid(RenamePrompt::INVALID)
    }
}

/// Whether `character` is a combining mark, which an identifier of many scripts has among its letters.
fn is_mark(character: char) -> bool {
    matches!(character, '\u{300}'..='\u{36f}' | '\u{900}'..='\u{903}' | '\u{93a}'..='\u{94f}' | '\u{1ab0}'..='\u{1aff}' | '\u{20d0}'..='\u{20ff}')
}

/// The question, whether it is open, and what is said of the answer.
#[derive(Default)]
pub struct RenamePrompt {
    asking: Rc<RefCell<Option<Rc<Asking>>>>,
    complaint: RefCell<Option<&'static str>>,
    redraw: Redraw,
}

impl RenamePrompt {
    /// How long after the question opens its field takes the focus: the menu that it was asked from closes in the next frame, and gives the
    /// focus back to the text, so a field that took it at once would lose it.
    const FOCUS_DELAY: Duration = Duration::from_millis(80);

    /// What is said of an answer that is nothing.
    pub const EMPTY: &'static str = "A name cannot be empty";
    /// What is said of an answer that is not a name.
    pub const INVALID: &'static str = "That is not a valid name";

    /// Asks what the name `old`, which starts at the byte `at`, is to be called, hanging the question from `spot`.
    pub fn open(&self, at: usize, old: String, spot: FieldSpot) {
        let controller = TextEditingController::with_text(old.clone());
        controller.select_bytes(0, old.len());
        let asking = Rc::new(Asking {
            at,
            old,
            spot,
            controller,
            focus: FocusNode::new(),
        });
        *self.asking.borrow_mut() = Some(Rc::clone(&asking));
        *self.complaint.borrow_mut() = None;
        self.redraw.fire();
        // The field takes the focus once the menu is gone, if this is still the question that is open then. Where there is no runtime, as
        // in a test, nothing waits.
        if let Some(venus) = Venus::current() {
            let (waited, open) = (venus.sleep(Self::FOCUS_DELAY), Rc::clone(&self.asking));
            venus.spawn(async move {
                let _ = waited.await;
                if open
                    .borrow()
                    .as_ref()
                    .is_some_and(|now| Rc::ptr_eq(now, &asking))
                {
                    asking.focus.request_focus();
                }
            });
        }
    }

    /// Drops the question, if there is one.
    pub fn close(&self) {
        let was_open = self.asking.borrow_mut().take().is_some();
        *self.complaint.borrow_mut() = None;
        if was_open {
            self.redraw.fire();
        }
    }

    /// The question that is open.
    pub fn asking(&self) -> Option<Rc<Asking>> {
        self.asking.borrow().clone()
    }

    pub fn is_open(&self) -> bool {
        self.asking.borrow().is_some()
    }

    /// Says what is wrong with the answer, under the field.
    pub fn complain(&self, complaint: &'static str) {
        *self.complaint.borrow_mut() = Some(complaint);
        self.redraw.fire();
    }

    /// What was said of the answer, if it is still said.
    pub fn complaint(&self) -> Option<&'static str> {
        *self.complaint.borrow()
    }

    /// The answer was typed in again: what was said of the one before is not said of this.
    pub fn typed(&self) {
        if self.complaint.borrow_mut().take().is_some() {
            self.redraw.fire();
        }
    }

    /// Adds what is called to draw the card again, which says whether the card is still there.
    pub fn on_change(&self, hook: Rc<dyn Fn() -> bool>) {
        self.redraw.add(hook);
    }
}

#[cfg(test)]
#[path = "../tests/ui/rename.rs"]
mod tests;
