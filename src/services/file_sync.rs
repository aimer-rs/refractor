//! Keeping the text of an open file as the file is: when something else changes it, a formatter or a `git checkout` or another editor, the
//! text follows, unless the user has changed it too, in which case what the user wrote is kept and the conflict is said.
//!
//! The files are looked at now and then, by when they were changed and how long they are, which costs a look at each open file and not
//! a read of it. A file whose look is not the one the document last saw is read again.

use std::cell::Cell;
use std::rc::{Rc, Weak};
use std::time::Duration;

use aimer::Venus;

use crate::model::document::{Disk, Document};

/// What was found of a file, when it was looked at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Synced {
    /// As the document has it.
    Same,
    /// Changed by something else, and the text is what the file says now.
    Reloaded,
    /// Changed by something else, and the document has changes of its own, which are kept: the file is not read.
    Conflict,
    /// Not there any more. The text is kept, and saving writes the file again.
    Deleted,
    /// Changed by something else, and cannot be read as text. Why.
    Failed(String),
}

/// Looks at the file of `document`, and brings the text to it if it was changed by something else and the document has no changes of
/// its own. What was found is told once: the same change is not found again at the next look.
pub fn sync_document(document: &Document) -> Synced {
    match document.disk() {
        Disk::Same => Synced::Same,
        Disk::Gone => {
            document.acknowledge_disk();
            Synced::Deleted
        }
        Disk::Changed if document.is_dirty() => {
            document.acknowledge_disk();
            Synced::Conflict
        }
        Disk::Changed => match document.reload() {
            // A file that was touched and says what it said: nothing to tell.
            Ok(crate::model::document::Reloaded::Unchanged) => Synced::Same,
            Ok(crate::model::document::Reloaded::Replaced) => Synced::Reloaded,
            Err(error) => {
                document.acknowledge_disk();
                Synced::Failed(error.to_string())
            }
        },
    }
}

/// Looks at each of `documents`, and tells `report` of those that were not as they were.
pub fn look_at(documents: &[Rc<Document>], report: &dyn Fn(&Rc<Document>, Synced)) {
    for document in documents {
        let found = sync_document(document);
        if found != Synced::Same {
            report(document, found);
        }
    }
}

/// Looks at the open files now and then, for as long as it lives.
pub struct FileSync {
    started: Cell<bool>,
}

impl FileSync {
    /// How long between two looks. A look is a stat of each open file.
    pub const INTERVAL: Duration = Duration::from_millis(750);

    pub fn new() -> Rc<Self> {
        Rc::new(Self {
            started: Cell::new(false),
        })
    }

    /// Begins looking at what `documents` says is open, and telling `report` what was found, which is done outside an update of the
    /// workspace. Does nothing outside the runtime, and the second time.
    pub fn start(
        self: &Rc<Self>,
        documents: Rc<dyn Fn() -> Vec<Rc<Document>>>,
        report: Rc<dyn Fn(&Rc<Document>, Synced)>,
    ) {
        let Some(venus) = Venus::current() else {
            return;
        };
        if self.started.replace(true) {
            return;
        }
        let this: Weak<Self> = Rc::downgrade(self);
        let sleeper = venus.clone();
        venus.spawn(async move {
            loop {
                let _ = sleeper.sleep(Self::INTERVAL).await;
                if this.upgrade().is_none() {
                    break;
                }
                look_at(&documents(), &*report);
            }
        });
    }
}

#[cfg(test)]
#[path = "../tests/services/file_sync.rs"]
mod tests;
