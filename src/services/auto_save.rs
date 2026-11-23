//! Saving a changed file without being asked to: a while after the last change, or when the editor loses the focus.
//!
//! The service knows when to save and not how: what saves is given to it, and is what the save key runs, so that a save by itself is
//! the same as a save by hand: the file is written, git is asked again, and the language server is told, which makes it check the
//! project. A save is made from what the user does, and not from inside an update of the workspace.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use aimer::Venus;
use aimer::venus::{Lifecycle, LifecycleSubscription, on_lifecycle};

use crate::model::document::Document;
pub use crate::services::settings::AutoSave as AutoSaveMode;

/// The identity the settings tab has among the tabs, which is not a file.
const SETTINGS_TAB: u64 = 0;

/// When files are saved by themselves, and what saves them.
pub struct AutoSave {
    mode: Cell<AutoSaveMode>,
    delay: Cell<Duration>,
    saver: RefCell<Option<Rc<dyn Fn(&Rc<Document>)>>>,
    /// The latest wait of each document that is waiting, by its identity: a wait that ends and finds it is not the latest was
    /// overtaken by a change, and does nothing.
    waits: RefCell<HashMap<u64, u64>>,
    next_wait: Cell<u64>,
    /// The tab that was shown the last time, which is how another being shown is known.
    shown: Cell<Option<u64>>,
    /// Keeps the app told when the window goes to the background, for as long as there is a service.
    watching: RefCell<Option<LifecycleSubscription>>,
}

impl AutoSave {
    pub fn new() -> Rc<Self> {
        Rc::new(Self {
            mode: Cell::new(AutoSaveMode::Off),
            delay: Cell::new(Duration::from_secs(1)),
            saver: RefCell::new(None),
            waits: RefCell::new(HashMap::new()),
            next_wait: Cell::new(1),
            shown: Cell::new(None),
            watching: RefCell::new(None),
        })
    }

    /// Sets when files are saved, and how long after the last change when it is after a delay. Said again on every build of what
    /// shows the settings, so a change of them is followed.
    pub fn configure(&self, mode: AutoSaveMode, delay: Duration) {
        self.mode.set(mode);
        self.delay.set(delay);
    }

    /// Sets what saves a document.
    pub fn set_saver(&self, saver: Rc<dyn Fn(&Rc<Document>)>) {
        *self.saver.borrow_mut() = Some(saver);
    }

    /// Saves `document`, if it is changed and there is something to save it with.
    fn save(&self, document: &Rc<Document>) {
        if !document.is_dirty() {
            return;
        }
        // Not while the cell is borrowed: what saves touches the workspace, which may say something to the service.
        let saver = self.saver.borrow().clone();
        if let Some(saver) = saver {
            saver(document);
        }
    }

    /// The text of `document` was changed. When files are saved after a delay, begins a wait for the document, which a later change
    /// makes useless, and says which wait it is; and in no other way, nothing. The save is made when the wait ends, by
    /// [`expired`](Self::expired), which a timer calls.
    pub fn edited(self: &Rc<Self>, document: &Rc<Document>) -> Option<u64> {
        if self.mode.get() != AutoSaveMode::Delay {
            return None;
        }
        let wait = self.next_wait.get();
        self.next_wait.set(wait + 1);
        self.waits
            .borrow_mut()
            .insert(document.id, wait);
        let (this, document) = (Rc::downgrade(self), Rc::downgrade(document));
        if let Some(venus) = Venus::current() {
            let waited = venus.sleep(self.delay.get());
            venus.spawn(async move {
                // A runtime that has no timers answers at once.
                let _ = waited.await;
                if let (Some(this), Some(document)) = (this.upgrade(), document.upgrade()) {
                    this.expired(&document, wait);
                }
            });
        }
        Some(wait)
    }

    /// The wait `wait` of `document` is over: it is saved if it is changed and was not overtaken by another change, and the way of
    /// saving is still after a delay.
    pub fn expired(&self, document: &Rc<Document>, wait: u64) {
        if self.mode.get() != AutoSaveMode::Delay {
            return;
        }
        {
            let mut waits = self.waits.borrow_mut();
            if waits.get(&document.id) != Some(&wait) {
                return;
            }
            waits.remove(&document.id);
        }
        self.save(document);
    }

    /// The editor lost the focus: when files are saved for that, every changed one is.
    pub fn focus_lost(&self, documents: &[Rc<Document>]) {
        if self.mode.get() != AutoSaveMode::FocusLost {
            return;
        }
        for document in documents {
            self.save(document);
        }
    }

    /// The tab `shown` is the one that is shown, of the tabs of `documents` and the settings: when it is another than it was, the
    /// ones that were shown and are not now lost the focus.
    pub fn switched_now(&self, shown: Option<u64>, documents: &[Rc<Document>]) {
        let before = self.shown.replace(shown);
        if before == shown || self.mode.get() != AutoSaveMode::FocusLost {
            return;
        }
        for document in documents {
            // The one that is shown is being edited; a tab that is not a file shows no file.
            if before == Some(document.id)
                && shown != Some(document.id)
                && document.id != SETTINGS_TAB
            {
                self.save(document);
            }
        }
    }

    /// [`switched_now`](Self::switched_now), made after what is being built is, since a save touches the workspace and a build is
    /// not the time for that.
    pub fn switched(self: &Rc<Self>, shown: Option<u64>, documents: Vec<Rc<Document>>) {
        if self.shown.get() == shown {
            return;
        }
        let this = Rc::downgrade(self);
        match Venus::current() {
            Some(venus) => {
                venus.spawn(async move {
                    if let Some(this) = this.upgrade() {
                        this.switched_now(shown, &documents);
                    }
                });
            }
            None => self.switched_now(shown, &documents),
        }
    }

    /// Has the changed files saved when the window goes to the background, for as long as there is a service. `documents` says which
    /// files there are, at the time. Does nothing the second time.
    pub fn watch_window(self: &Rc<Self>, documents: Rc<dyn Fn() -> Vec<Rc<Document>>>) {
        let mut watching = self.watching.borrow_mut();
        if watching.is_some() {
            return;
        }
        let this = Rc::downgrade(self);
        *watching = Some(on_lifecycle(move |event| {
            if matches!(event, Lifecycle::Inactive | Lifecycle::Background)
                && let Some(this) = this.upgrade()
            {
                this.focus_lost(&documents());
            }
        }));
    }
}

impl std::fmt::Debug for AutoSave {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AutoSave")
            .field("mode", &self.mode.get())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "../tests/services/auto_save.rs"]
mod tests;
