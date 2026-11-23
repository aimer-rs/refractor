//! The prompt state for file tree operations: asking for a new file name, a new folder name, a renamed name, or confirming deletion.
//!
//! Mirrors [`crate::ui::rename::RenamePrompt`]: this module owns only the prompt's state and redraw notifications; the widget that draws
//! the floating card lives under `screen::editor::tree_prompt_card`.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use aimer::input::TextEditingController;
use aimer::{FocusNode, Venus};

use crate::ui::components::popup::Redraw;
use crate::services::tree_ops::rename_selection;

/// Which operation the open file-tree prompt is asking about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreePromptKind {
    NewFile {
        dir: PathBuf,
    },
    NewFolder {
        dir: PathBuf,
    },
    Rename {
        path: PathBuf,
        old: String,
        is_dir: bool,
    },
    Delete {
        entries: Vec<(PathBuf, String, bool)>,
    },
}

/// The open prompt: what it is asking, its text field (if it takes a name), and its focus handle.
pub struct TreeAsking {
    pub kind: TreePromptKind,
    /// Present for `NewFile`, `NewFolder`, and `Rename`; `None` for `Delete`.
    pub controller: Option<TextEditingController>,
    pub focus: FocusNode,
}

/// Holds the currently open file-tree prompt (if any) and any validation or I/O error message.
#[derive(Default)]
pub struct TreePrompt {
    asking: Rc<RefCell<Option<Rc<TreeAsking>>>>,
    complaint: RefCell<Option<String>>,
    redraw: Redraw,
}

impl TreePrompt {
    /// Delay before focusing the prompt's field, so the context menu that opened it finishes closing first without stealing focus back.
    const FOCUS_DELAY: Duration = Duration::from_millis(80);

    /// Opens a prompt to create a new file inside `dir`.
    pub fn open_new_file(&self, dir: PathBuf) {
        self.open(
            TreePromptKind::NewFile { dir },
            Some(TextEditingController::new()),
        );
    }

    /// Opens a prompt to create a new folder inside `dir`.
    pub fn open_new_folder(&self, dir: PathBuf) {
        self.open(
            TreePromptKind::NewFolder { dir },
            Some(TextEditingController::new()),
        );
    }

    /// Opens a prompt to rename `path` (currently named `old`), pre-selecting the stem for files or the whole name for folders.
    pub fn open_rename(&self, path: PathBuf, old: String, is_dir: bool) {
        let controller = TextEditingController::with_text(old.clone());
        let (start, end) = rename_selection(&old, is_dir);
        controller.select_bytes(start, end);
        self.open(
            TreePromptKind::Rename { path, old, is_dir },
            Some(controller),
        );
    }

    /// Opens a confirmation prompt to permanently delete `path`.
    pub fn open_delete(&self, path: PathBuf, name: String, is_dir: bool) {
        self.open_delete_many(vec![(path, name, is_dir)]);
    }

    /// Opens a confirmation prompt to permanently delete one or more entries.
    pub fn open_delete_many(&self, entries: Vec<(PathBuf, String, bool)>) {
        if entries.is_empty() {
            return;
        }
        self.open(TreePromptKind::Delete { entries }, None);
    }

    fn open(&self, kind: TreePromptKind, controller: Option<TextEditingController>) {
        let asking = Rc::new(TreeAsking {
            kind,
            controller,
            focus: FocusNode::new(),
        });
        *self.asking.borrow_mut() = Some(Rc::clone(&asking));
        *self.complaint.borrow_mut() = None;
        self.redraw.fire();
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

    /// Closes the prompt if open and clears any error message.
    pub fn close(&self) {
        let was_open = self.asking.borrow_mut().take().is_some();
        *self.complaint.borrow_mut() = None;
        if was_open {
            self.redraw.fire();
        }
    }

    /// Closes the prompt if `target` is still the active asking instance.
    ///
    /// Used when a modal dialog unmounts after being dismissed externally (by clicking outside or pressing Escape),
    /// so dismissing an older modal cannot close a newer prompt that has already replaced it.
    pub fn close_if_same(&self, target: &Rc<TreeAsking>) {
        let same = self
            .asking
            .borrow()
            .as_ref()
            .is_some_and(|now| Rc::ptr_eq(now, target));
        if same {
            self.close();
        }
    }

    /// The currently open prompt, if any.
    pub fn asking(&self) -> Option<Rc<TreeAsking>> {
        self.asking.borrow().clone()
    }

    /// Displays an error message beneath the prompt input.
    pub fn complain(&self, message: impl Into<String>) {
        *self.complaint.borrow_mut() = Some(message.into());
        self.redraw.fire();
    }

    /// The current error message, if any.
    pub fn complaint(&self) -> Option<String> {
        self.complaint.borrow().clone()
    }

    /// Clears the current error message when the user edits the input again.
    pub fn typed(&self) {
        if self.complaint.borrow_mut().take().is_some() {
            self.redraw.fire();
        }
    }

    /// Registers a watcher notified whenever the prompt opens, closes, or changes its complaint.
    pub fn on_change(&self, hook: Rc<dyn Fn() -> bool>) {
        self.redraw.add(hook);
    }
}

#[cfg(test)]
#[path = "../tests/ui/tree_prompt.rs"]
mod tests;
