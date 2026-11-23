//! Git sidebar panel state and model for Commit and Push.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use aimer::input::TextEditingController;
use aimer::{FocusNode, Venus};

use super::operations::{ChangedFile, PushInfo};
use crate::ui::components::popup::Redraw;

/// An entry in the commit file list with checkbox selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitFileEntry {
    pub file: ChangedFile,
    pub selected: bool,
}

/// The state of the commit section.
pub struct CommitPromptState {
    pub files: RefCell<Vec<CommitFileEntry>>,
    pub message_controller: TextEditingController,
    pub message_focus: FocusNode,
}

/// The state of the push section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PushPromptState {
    pub info: PushInfo,
}

/// What the git prompt was last opened for.
pub enum GitPromptMode {
    Commit(CommitPromptState),
    Push(PushPromptState),
}

/// Snapshot handle for compatibility with modal/prompt lifecycle checks.
pub struct GitPromptAsking {
    pub mode: GitPromptMode,
    pub is_busy: Cell<bool>,
    pub error: RefCell<Option<String>>,
}

/// Holds the Git sidebar panel state (changed files, commit message, unpushed commits, busy/error status)
/// and notifies the sidebar widget on change.
pub struct GitPrompt {
    files: RefCell<Vec<CommitFileEntry>>,
    unpushed: RefCell<Option<PushInfo>>,
    message_controller: TextEditingController,
    message_focus: FocusNode,
    is_busy: Cell<bool>,
    error: RefCell<Option<String>>,
    asking: Rc<RefCell<Option<Rc<GitPromptAsking>>>>,
    redraw: Redraw,
}

impl Default for GitPrompt {
    fn default() -> Self {
        Self {
            files: RefCell::new(Vec::new()),
            unpushed: RefCell::new(None),
            message_controller: TextEditingController::new(),
            message_focus: FocusNode::new(),
            is_busy: Cell::new(false),
            error: RefCell::new(None),
            asking: Rc::new(RefCell::new(None)),
            redraw: Redraw::default(),
        }
    }
}

impl GitPrompt {
    const FOCUS_DELAY: Duration = Duration::from_millis(60);

    /// Updates the changed files list in the sidebar, preserving unchecked state for files that were already listed.
    pub fn set_files(&self, changed: Vec<ChangedFile>) {
        let previously_unselected: HashSet<PathBuf> = self
            .files
            .borrow()
            .iter()
            .filter(|e| !e.selected)
            .map(|e| e.file.path.clone())
            .collect();
        let entries: Vec<CommitFileEntry> = changed
            .into_iter()
            .map(|file| {
                let selected = !previously_unselected.contains(&file.path);
                CommitFileEntry { file, selected }
            })
            .collect();
        *self.files.borrow_mut() = entries;
        self.redraw.fire();
    }

    /// Updates the unpushed commits info in the sidebar.
    pub fn set_unpushed(&self, info: Option<PushInfo>) {
        *self.unpushed.borrow_mut() = info;
        self.redraw.fire();
    }

    /// Updates both changed files and unpushed commits in a single redraw.
    pub fn set_snapshot(&self, changed: Vec<ChangedFile>, unpushed: Option<PushInfo>) {
        let previously_unselected: HashSet<PathBuf> = self
            .files
            .borrow()
            .iter()
            .filter(|e| !e.selected)
            .map(|e| e.file.path.clone())
            .collect();
        let entries: Vec<CommitFileEntry> = changed
            .into_iter()
            .map(|file| {
                let selected = !previously_unselected.contains(&file.path);
                CommitFileEntry { file, selected }
            })
            .collect();
        *self.files.borrow_mut() = entries;
        *self.unpushed.borrow_mut() = unpushed;
        self.redraw.fire();
    }

    /// Returns a snapshot of the commit file entries.
    pub fn files(&self) -> Vec<CommitFileEntry> {
        self.files.borrow().clone()
    }

    /// Returns a snapshot of the unpushed commit info, if available.
    pub fn unpushed(&self) -> Option<PushInfo> {
        self.unpushed.borrow().clone()
    }

    /// The commit message text controller.
    pub fn message_controller(&self) -> TextEditingController {
        self.message_controller.clone()
    }

    /// The focus node for the commit message field.
    pub fn message_focus(&self) -> FocusNode {
        self.message_focus.clone()
    }

    /// Clears the commit message field after a successful commit.
    pub fn clear_message(&self) {
        self.message_controller.clear();
        self.redraw.fire();
    }

    /// Requests keyboard focus on the commit message input field.
    pub fn focus_message(&self) {
        let focus = self.message_focus.clone();
        focus.request_focus();
        if let Some(venus) = Venus::current() {
            let waited = venus.sleep(Self::FOCUS_DELAY);
            venus.spawn(async move {
                let _ = waited.await;
                focus.request_focus();
            });
        }
    }

    /// Loads `files` into the commit list and marks the prompt open.
    pub fn open_commit(&self, files: Vec<ChangedFile>) {
        let entries: Vec<CommitFileEntry> = files
            .into_iter()
            .map(|file| CommitFileEntry {
                file,
                selected: true,
            })
            .collect();
        *self.files.borrow_mut() = entries.clone();
        *self.error.borrow_mut() = None;
        self.is_busy.set(false);

        let asking = Rc::new(GitPromptAsking {
            mode: GitPromptMode::Commit(CommitPromptState {
                files: RefCell::new(entries),
                message_controller: self.message_controller.clone(),
                message_focus: self.message_focus.clone(),
            }),
            is_busy: Cell::new(false),
            error: RefCell::new(None),
        });
        *self.asking.borrow_mut() = Some(asking);
        self.redraw.fire();
        self.focus_message();
    }

    /// Loads `info` into the unpushed info state and marks the prompt open.
    pub fn open_push(&self, info: PushInfo) {
        *self.unpushed.borrow_mut() = Some(info.clone());
        *self.error.borrow_mut() = None;
        self.is_busy.set(false);

        let asking = Rc::new(GitPromptAsking {
            mode: GitPromptMode::Push(PushPromptState { info }),
            is_busy: Cell::new(false),
            error: RefCell::new(None),
        });
        *self.asking.borrow_mut() = Some(asking);
        self.redraw.fire();
    }

    /// Closes the prompt asking handle.
    pub fn close(&self) {
        let was_open = self.asking.borrow_mut().take().is_some();
        *self.error.borrow_mut() = None;
        self.is_busy.set(false);
        if was_open {
            self.redraw.fire();
        }
    }

    /// Closes the prompt only if `target` is still the active asking instance.
    pub fn close_if_same(&self, target: &Rc<GitPromptAsking>) {
        let same = self
            .asking
            .borrow()
            .as_ref()
            .is_some_and(|now| Rc::ptr_eq(now, target));
        if same {
            self.close();
        }
    }

    /// Returns the currently active prompt asking state, if open.
    pub fn asking(&self) -> Option<Rc<GitPromptAsking>> {
        self.asking.borrow().clone()
    }

    /// Whether the prompt is currently open.
    pub fn is_open(&self) -> bool {
        self.asking.borrow().is_some()
    }

    /// Toggles the selection of a file in the commit list.
    pub fn toggle_file(&self, path: &Path) {
        if let Some(entry) = self
            .files
            .borrow_mut()
            .iter_mut()
            .find(|e| e.file.path == path)
        {
            entry.selected = !entry.selected;
        }
        if let Some(asking) = self.asking.borrow().as_ref()
            && let GitPromptMode::Commit(state) = &asking.mode
        {
            if let Some(entry) = state
                .files
                .borrow_mut()
                .iter_mut()
                .find(|e| e.file.path == path)
            {
                entry.selected = !entry.selected;
            }
            *asking.error.borrow_mut() = None;
        }
        *self.error.borrow_mut() = None;
        self.redraw.fire();
    }

    /// Selects or deselects every changed file inside `dir`.
    pub fn set_dir_selected(&self, dir: &Path, select: bool) {
        for entry in self.files.borrow_mut().iter_mut() {
            if entry.file.path.starts_with(dir) {
                entry.selected = select;
            }
        }
        if let Some(asking) = self.asking.borrow().as_ref()
            && let GitPromptMode::Commit(state) = &asking.mode
        {
            for entry in state.files.borrow_mut().iter_mut() {
                if entry.file.path.starts_with(dir) {
                    entry.selected = select;
                }
            }
            *asking.error.borrow_mut() = None;
        }
        *self.error.borrow_mut() = None;
        self.redraw.fire();
    }

    /// Selects or deselects all files in the commit list.
    pub fn select_all(&self, select: bool) {
        for entry in self.files.borrow_mut().iter_mut() {
            entry.selected = select;
        }
        if let Some(asking) = self.asking.borrow().as_ref()
            && let GitPromptMode::Commit(state) = &asking.mode
        {
            for entry in state.files.borrow_mut().iter_mut() {
                entry.selected = select;
            }
            *asking.error.borrow_mut() = None;
        }
        *self.error.borrow_mut() = None;
        self.redraw.fire();
    }

    /// Clears any current error when the user edits the commit message.
    pub fn typed(&self) {
        let mut cleared = self.error.borrow_mut().take().is_some();
        if let Some(asking) = self.asking.borrow().as_ref() {
            cleared |= asking.error.borrow_mut().take().is_some();
        }
        if cleared {
            self.redraw.fire();
        }
    }

    /// Sets whether a git operation is in progress.
    pub fn set_busy(&self, busy: bool) {
        self.is_busy.set(busy);
        if let Some(asking) = self.asking.borrow().as_ref() {
            asking.is_busy.set(busy);
        }
        self.redraw.fire();
    }

    /// Sets an error message to display in the panel.
    pub fn set_error(&self, error: Option<String>) {
        *self.error.borrow_mut() = error.clone();
        if let Some(asking) = self.asking.borrow().as_ref() {
            *asking.error.borrow_mut() = error;
        }
        self.redraw.fire();
    }

    /// Returns the current error message, if any.
    pub fn error(&self) -> Option<String> {
        self.error.borrow().clone()
    }

    /// Returns whether a git operation is currently running.
    pub fn is_busy(&self) -> bool {
        self.is_busy.get()
    }

    /// Registers a callback to be invoked whenever state changes.
    pub fn on_change(&self, hook: Rc<dyn Fn() -> bool>) {
        self.redraw.add(hook);
    }

    /// Gets the paths of files selected for commit.
    pub fn selected_files(&self) -> Vec<PathBuf> {
        self.files
            .borrow()
            .iter()
            .filter(|e| e.selected)
            .map(|e| e.file.path.clone())
            .collect()
    }

    /// Gets the current commit message.
    pub fn commit_message(&self) -> String {
        self.message_controller
            .value()
            .text()
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::git::Change;
    use std::cell::Cell;

    #[test]
    fn open_commit_initializes_selected_files_and_preserves_instance_across_toggles() {
        let prompt = GitPrompt::default();
        let notified = Rc::new(Cell::new(0));
        let notified_clone = Rc::clone(&notified);
        prompt.on_change(Rc::new(move || {
            notified_clone.set(notified_clone.get() + 1);
            true
        }));

        let files = vec![
            ChangedFile {
                path: PathBuf::from("a.txt"),
                absolute_path: PathBuf::from("/repo/a.txt"),
                change: Change::Modified,
                staged: false,
            },
            ChangedFile {
                path: PathBuf::from("b.txt"),
                absolute_path: PathBuf::from("/repo/b.txt"),
                change: Change::Untracked,
                staged: false,
            },
        ];

        prompt.open_commit(files);
        assert!(prompt.is_open());
        let first = prompt.asking().expect("open");
        assert_eq!(prompt.selected_files().len(), 2);
        assert_eq!(notified.get(), 1);

        prompt.toggle_file(Path::new("a.txt"));
        assert!(Rc::ptr_eq(&first, &prompt.asking().expect("still open")));
        assert_eq!(prompt.selected_files(), vec![PathBuf::from("b.txt")]);

        prompt.select_all(false);
        assert!(prompt.selected_files().is_empty());

        prompt.select_all(true);
        assert_eq!(prompt.selected_files().len(), 2);

        prompt.close();
        assert!(!prompt.is_open());
    }

    #[test]
    fn set_dir_selected_toggles_only_files_inside_that_directory() {
        let prompt = GitPrompt::default();
        let files = vec![
            ChangedFile {
                path: PathBuf::from("src/a.rs"),
                absolute_path: PathBuf::from("/repo/src/a.rs"),
                change: Change::Modified,
                staged: false,
            },
            ChangedFile {
                path: PathBuf::from("src/nested/b.rs"),
                absolute_path: PathBuf::from("/repo/src/nested/b.rs"),
                change: Change::Added,
                staged: false,
            },
            ChangedFile {
                path: PathBuf::from("src_other/c.rs"),
                absolute_path: PathBuf::from("/repo/src_other/c.rs"),
                change: Change::Modified,
                staged: false,
            },
            ChangedFile {
                path: PathBuf::from("Cargo.toml"),
                absolute_path: PathBuf::from("/repo/Cargo.toml"),
                change: Change::Modified,
                staged: false,
            },
        ];
        prompt.set_files(files);
        assert_eq!(prompt.selected_files().len(), 4);

        prompt.set_dir_selected(Path::new("src"), false);
        assert_eq!(
            prompt.selected_files(),
            vec![PathBuf::from("src_other/c.rs"), PathBuf::from("Cargo.toml")],
            "only descendants of `src` are deselected; sibling prefix `src_other` is untouched"
        );

        prompt.set_dir_selected(Path::new("src/nested"), true);
        assert_eq!(
            prompt.selected_files(),
            vec![
                PathBuf::from("src/nested/b.rs"),
                PathBuf::from("src_other/c.rs"),
                PathBuf::from("Cargo.toml"),
            ]
        );
    }

    #[test]
    fn open_push_error_handling_and_close_if_same() {
        let prompt = GitPrompt::default();
        let info = PushInfo {
            branch: "feature".to_string(),
            remote: "origin".to_string(),
            remote_branch: "origin/feature".to_string(),
            commits: vec![],
        };

        prompt.open_push(info.clone());
        let first = prompt.asking().expect("push open");
        assert!(prompt.is_open());

        prompt.set_busy(true);
        assert!(prompt.is_busy());

        prompt.set_error(Some("Authentication failed".to_string()));
        assert_eq!(prompt.error().as_deref(), Some("Authentication failed"));

        // Replace with a new push prompt; close_if_same on old instance must not close the new one.
        prompt.open_push(info);
        prompt.close_if_same(&first);
        assert!(prompt.is_open());

        let second = prompt.asking().expect("second open");
        prompt.close_if_same(&second);
        assert!(!prompt.is_open());
    }
}
