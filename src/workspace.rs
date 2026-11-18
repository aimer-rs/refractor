//! What is open: the folder, the files in it that are open, and which one is shown.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use aimer::{ProviderHandle, Venus};

use crate::document::Document;
use crate::git::{self, Change};
use crate::tree;

/// The state of the editor, shared by the file tree, the tabs and the editor. It is cloned cheaply:
/// the documents are shared.
#[derive(Clone)]
pub struct Workspace {
    pub root: PathBuf,
    /// The branch the folder is on, if it is in a repository.
    pub branch: Option<String>,
    pub documents: Vec<Rc<Document>>,
    /// The document being edited, by its identity.
    pub active: Option<u64>,
    /// The folders that are open in the file tree.
    expanded: HashSet<PathBuf>,
    /// What the status bar says about the last thing that went wrong, if anything.
    pub message: Option<String>,
    /// The tab whose close was asked for while it had changes: asking again closes it.
    pub closing: Option<u64>,
    /// The rows of the file tree, made when the folder or what is open in it changes, and not each
    /// time something is drawn.
    rows: Rc<Vec<tree::Row>>,
    /// What git says of the files, read again when the folder is opened and after a save. Nothing
    /// until the first answer, or for a folder that is not in a repository.
    status: Rc<git::Status>,
    next_id: u64,
    /// Counts the times the documents' changes were reported, so what watches the workspace is told
    /// when a document becomes changed or unchanged, which the list of documents does not show.
    pub changes: u64,
}

impl Workspace {
    /// A workspace on the folder `root`, with nothing open.
    pub fn new(root: PathBuf) -> Self {
        let branch = git::branch_of(&root);
        let mut space = Self {
            root,
            branch,
            documents: Vec::new(),
            active: None,
            expanded: HashSet::new(),
            message: None,
            closing: None,
            rows: Rc::new(Vec::new()),
            status: Rc::default(),
            next_id: 1,
            changes: 0,
        };
        space.refresh();
        space
    }

    /// The workspace the app starts with, on the folder [`choose_root`] picks.
    pub fn initial() -> Self {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let first_argument = std::env::args_os()
            .nth(1)
            .map(PathBuf::from);
        Self::new(choose_root(
            first_argument,
            std::env::var_os("REFRACTOR_ROOT").map(PathBuf::from),
            std::env::current_dir().ok(),
            manifest,
            std::env::var_os("HOME").map(PathBuf::from),
        ))
    }

    /// Reads the folders that are open again, for what changed on disk.
    pub fn refresh(&mut self) {
        self.rows = Rc::new(tree::rows(&self.root, &self.expanded));
    }

    /// The name of the folder, for the title.
    pub fn name(&self) -> String {
        self.root
            .file_name()
            .map_or_else(
                || self.root.display().to_string(),
                |name| {
                    name.to_string_lossy()
                        .into_owned()
                },
            )
    }

    /// Makes `root` the folder, and closes nothing: the files stay open.
    pub fn set_root(&mut self, root: PathBuf) {
        self.branch = git::branch_of(&root);
        self.root = root;
        self.expanded.clear();
        self.status = Rc::default();
        self.refresh();
    }

    /// Closes every folder of the tree.
    pub fn collapse_all(&mut self) {
        self.expanded.clear();
        self.refresh();
    }

    /// Takes what git said of `root`, unless the folder has changed since it was asked.
    pub fn set_status(&mut self, root: &Path, status: git::Status) {
        if self.root == root {
            self.status = Rc::new(status);
        }
    }

    /// How git sees the file or folder `path`, if it is not as committed.
    pub fn change_of(&self, path: &Path) -> Option<Change> {
        self.status.change_of(path)
    }

    /// Whether something inside the folder `path` is changed.
    pub fn has_changes_inside(&self, path: &Path) -> bool {
        self.status
            .has_changes_inside(path)
    }

    /// Whether anything in the repository is changed.
    pub fn git_dirty(&self) -> bool {
        self.status.is_dirty()
    }

    /// The rows of the file tree.
    pub fn rows(&self) -> Rc<Vec<tree::Row>> {
        Rc::clone(&self.rows)
    }

    /// Opens or closes the folder `path` in the tree.
    pub fn toggle(&mut self, path: &Path) {
        if !self.expanded.remove(path) {
            self.expanded
                .insert(path.to_path_buf());
        }
        self.refresh();
    }

    /// Opens `path` in a tab and shows it, or shows the tab it is already in. `notify` is told when
    /// the new document becomes changed or unchanged. Says why if the file cannot be opened.
    pub fn open(&mut self, path: &Path, notify: Rc<dyn Fn()>) -> io::Result<()> {
        self.message = None;
        if let Some(document) = self
            .documents
            .iter()
            .find(|document| document.path == path)
        {
            self.active = Some(document.id);
            return Ok(());
        }
        let document = match Document::open(self.next_id, path, notify) {
            Ok(document) => document,
            Err(error) => {
                self.message = Some(format!("Could not open {}: {error}", path.display()));
                return Err(error);
            }
        };
        self.next_id += 1;
        self.active = Some(document.id);
        self.documents.push(document);
        Ok(())
    }

    /// Shows the tab `id`.
    pub fn activate(&mut self, id: u64) {
        if self
            .documents
            .iter()
            .any(|document| document.id == id)
        {
            self.active = Some(id);
        }
    }

    /// Asks to close the tab `id`. A tab with no unsaved changes closes at once. One with changes
    /// does not: the status bar says so, and asking again closes it and throws the changes away.
    pub fn request_close(&mut self, id: u64) {
        let Some(document) = self
            .documents
            .iter()
            .find(|document| document.id == id)
        else {
            return;
        };
        if document.is_dirty() && self.closing != Some(id) {
            self.message = Some(format!(
                "{} has unsaved changes: save it, or close it again to discard them",
                document.name
            ));
            self.closing = Some(id);
            return;
        }
        self.close(id);
    }

    /// Closes the tab `id`, and shows the one that was beside it. The changes of a document that is
    /// closed are lost: see [`request_close`](Self::request_close).
    pub fn close(&mut self, id: u64) {
        self.closing = None;
        self.message = None;
        let Some(index) = self
            .documents
            .iter()
            .position(|document| document.id == id)
        else {
            return;
        };
        self.documents.remove(index);
        if self.active == Some(id) {
            self.active = self
                .documents
                .get(index)
                .or_else(|| {
                    index
                        .checked_sub(1)
                        .and_then(|previous| self.documents.get(previous))
                })
                .map(|document| document.id);
        }
    }

    /// The document being edited.
    pub fn active_document(&self) -> Option<&Rc<Document>> {
        self.documents
            .iter()
            .find(|document| Some(document.id) == self.active)
    }

    /// Whether the file `path` is open and has changes that are not saved.
    pub fn is_changed(&self, path: &Path) -> bool {
        self.documents
            .iter()
            .any(|document| document.path == path && document.is_dirty())
    }

    /// Records that a document became changed or unchanged.
    pub fn touch(&mut self) {
        self.changes += 1;
    }

    /// Records how saving `name` went: nothing to say if it went well, and why if it did not.
    ///
    /// Saving is not done here, and must not be done inside an update of the workspace: a document
    /// that becomes unchanged tells the workspace so, which is an update of its own.
    pub fn report_save(&mut self, name: &str, result: io::Result<()>) {
        self.message = result
            .err()
            .map(|error| format!("Could not save {name}: {error}"));
        self.closing = None;
    }

    /// The path of `path` from the folder, for the breadcrumb, or the whole path when it is outside it.
    pub fn relative(&self, path: &Path) -> PathBuf {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_path_buf()
    }
}

/// What a document calls when it becomes changed or unchanged: it touches the workspace, which is how
/// what shows the change (a tab, a row of the tree) is drawn again. It is not called on each keystroke.
pub fn notifier(handle: &ProviderHandle<Workspace>) -> Rc<dyn Fn()> {
    let handle = handle.clone();
    Rc::new(move || handle.update(|space| space.touch()))
}

/// Asks git for the status of the folder, and gives the answer to the workspace when it comes. Git is
/// a program, so it runs off the UI thread: the tree and the tabs show the answer when it arrives, and
/// are drawn without it until then. Does nothing outside the UI thread.
pub fn refresh_status(handle: &ProviderHandle<Workspace>) {
    let Some(venus) = Venus::current() else {
        return;
    };
    let handle = handle.clone();
    let root = handle.read().root.clone();
    let blocking = venus.clone();
    venus.spawn(async move {
        let asked = root.clone();
        if let Some(status) = blocking
            .spawn_blocking(move || git::status_of(&asked))
            .await
        {
            handle.update(|space| space.set_status(&root, status));
        }
    });
}

/// The folder to start on: the first of what the command line names (a folder, or a file, which
/// gives the folder it is in), the `REFRACTOR_ROOT` variable, the folder the app was started in
/// (unless it is the root of the disk, which is where an app launched from the desktop starts), the
/// folder the app was built from, and the home folder that is a folder.
pub fn choose_root(
    argument: Option<PathBuf>,
    variable: Option<PathBuf>,
    current: Option<PathBuf>,
    built_from: &Path,
    home: Option<PathBuf>,
) -> PathBuf {
    let from_argument = argument.map(|path| {
        if path.is_file() {
            path.parent()
                .map_or(path.clone(), Path::to_path_buf)
        } else {
            path
        }
    });
    let current = current.filter(|path| path.parent().is_some());
    [
        from_argument,
        variable,
        current,
        Some(built_from.to_path_buf()),
        home,
    ]
    .into_iter()
    .flatten()
    .find(|path| path.is_dir())
    .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::tree::Scratch;

    fn notify() -> Rc<dyn Fn()> {
        Rc::new(|| {})
    }

    fn workspace(scratch: &Scratch) -> Workspace {
        Workspace::new(scratch.0.clone())
    }

    #[test]
    fn opening_a_file_shows_it_and_opening_it_again_shows_the_same_tab() {
        let scratch = Scratch::new("open");
        let a = scratch.file("a.rs", "a");
        let b = scratch.file("b.rs", "b");
        let mut space = workspace(&scratch);
        space
            .open(&a, notify())
            .unwrap();
        space
            .open(&b, notify())
            .unwrap();
        assert_eq!(space.documents.len(), 2);
        assert_eq!(
            space
                .active_document()
                .unwrap()
                .name,
            "b.rs"
        );
        space
            .open(&a, notify())
            .unwrap();
        assert_eq!(space.documents.len(), 2, "a is not opened twice");
        assert_eq!(
            space
                .active_document()
                .unwrap()
                .name,
            "a.rs"
        );
        assert_ne!(space.documents[0].id, space.documents[1].id);
    }

    #[test]
    fn a_file_that_cannot_be_opened_is_reported_and_changes_nothing() {
        let scratch = Scratch::new("openfail");
        let mut space = workspace(&scratch);
        let open = scratch.file("a.rs", "a");
        space
            .open(&open, notify())
            .unwrap();
        assert!(
            space
                .open(&scratch.0.join("missing.rs"), notify())
                .is_err()
        );
        assert_eq!(space.documents.len(), 1);
        assert_eq!(
            space
                .active_document()
                .unwrap()
                .name,
            "a.rs"
        );
        assert!(
            space
                .message
                .as_deref()
                .is_some_and(|message| message.contains("missing.rs")),
            "{:?}",
            space.message
        );
        space
            .open(&open, notify())
            .unwrap();
        assert!(space.message.is_none(), "the next thing done clears it");
    }

    #[test]
    fn closing_the_shown_tab_shows_the_one_beside_it_and_closing_the_last_shows_nothing() {
        let scratch = Scratch::new("close");
        let mut space = workspace(&scratch);
        for name in ["a.rs", "b.rs", "c.rs"] {
            let path = scratch.file(name, name);
            space
                .open(&path, notify())
                .unwrap();
        }
        let ids: Vec<u64> = space
            .documents
            .iter()
            .map(|document| document.id)
            .collect();
        space.activate(ids[1]);
        space.close(ids[1]);
        assert_eq!(space.active, Some(ids[2]), "the one that took its place");
        space.close(ids[2]);
        assert_eq!(
            space.active,
            Some(ids[0]),
            "the last tab closed: the one before"
        );
        space.close(ids[0]);
        assert_eq!(space.active, None);
        assert!(
            space
                .active_document()
                .is_none()
        );
        space.close(999);
    }

    #[test]
    fn a_folder_in_the_tree_opens_and_closes() {
        let scratch = Scratch::new("toggle");
        scratch.file("src/main.rs", "");
        let mut space = workspace(&scratch);
        assert_eq!(space.rows().len(), 1);
        space.toggle(&scratch.0.join("src"));
        assert_eq!(space.rows().len(), 2);
        space.toggle(&scratch.0.join("src"));
        assert_eq!(space.rows().len(), 1);
    }

    #[test]
    fn the_workspace_knows_which_open_files_have_unsaved_changes() {
        let scratch = Scratch::new("changed");
        let a = scratch.file("a.rs", "a");
        let b = scratch.file("b.rs", "b");
        let changes = Rc::new(Cell::new(0));
        let counted = Rc::clone(&changes);
        let mut space = workspace(&scratch);
        space
            .open(&a, Rc::new(move || counted.set(counted.get() + 1)))
            .unwrap();
        space
            .open(&b, notify())
            .unwrap();
        assert!(
            !space
                .documents
                .iter()
                .any(|document| document.is_dirty())
        );

        space.documents[0]
            .controller
            .set_text("changed");
        assert!(space.is_changed(&a) && !space.is_changed(&b));
        assert!(
            space
                .documents
                .iter()
                .any(|document| document.is_dirty())
        );
        assert_eq!(changes.get(), 1, "the owner was told once");

        space.documents[0]
            .save()
            .unwrap();
        assert!(
            !space
                .documents
                .iter()
                .any(|document| document.is_dirty())
        );
    }

    #[test]
    fn the_folder_has_a_name_a_branch_and_paths_from_it() {
        let scratch = Scratch::new("names");
        scratch.file(".git/HEAD", "ref: refs/heads/main\n");
        let space = workspace(&scratch);
        assert!(
            space
                .name()
                .starts_with("refractor-names")
        );
        assert_eq!(space.branch.as_deref(), Some("main"));
        assert_eq!(
            space.relative(&scratch.0.join("src/lib.rs")),
            PathBuf::from("src/lib.rs")
        );
        assert_eq!(
            space.relative(Path::new("/elsewhere/x.rs")),
            PathBuf::from("/elsewhere/x.rs")
        );
    }

    #[test]
    fn a_tab_with_unsaved_changes_closes_on_the_second_request_and_one_without_closes_at_once() {
        let scratch = Scratch::new("closing");
        let a = scratch.file("a.rs", "a");
        let b = scratch.file("b.rs", "b");
        let mut space = workspace(&scratch);
        space
            .open(&a, notify())
            .unwrap();
        space
            .open(&b, notify())
            .unwrap();
        let (first, second) = (space.documents[0].id, space.documents[1].id);

        space.request_close(second);
        assert_eq!(space.documents.len(), 1, "no changes: closed at once");

        space.documents[0]
            .controller
            .set_text("changed");
        space.request_close(first);
        assert_eq!(space.documents.len(), 1, "changed: not closed");
        assert!(
            space
                .message
                .as_deref()
                .is_some_and(|message| message.contains("unsaved")),
            "{:?}",
            space.message
        );
        space.request_close(first);
        assert!(
            space.documents.is_empty(),
            "asked again: closed, and the changes are gone"
        );
        assert!(space.message.is_none() && space.closing.is_none());
    }

    #[test]
    fn how_a_save_went_is_a_message_only_when_it_went_wrong() {
        let scratch = Scratch::new("saving");
        let path = scratch.file("a.rs", "a");
        let mut space = workspace(&scratch);
        space
            .open(&path, notify())
            .unwrap();
        space.documents[0]
            .controller
            .set_text("saved text");
        let (name, result) = (space.documents[0].name.clone(), space.documents[0].save());
        space.report_save(&name, result);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "saved text");
        assert!(space.message.is_none());

        space.message = Some("an old message".to_owned());
        space.documents[0]
            .controller
            .set_text("more");
        std::fs::remove_dir_all(&scratch.0).unwrap();
        let result = space.documents[0].save();
        space.report_save(&name, result);
        assert!(
            space
                .message
                .as_deref()
                .is_some_and(|message| message.starts_with("Could not save a.rs")),
            "{:?}",
            space.message
        );
        space.report_save(&name, Ok(()));
        assert!(
            space.message.is_none(),
            "a save that went well clears what the last one said"
        );
    }

    #[test]
    fn a_document_tells_its_owner_it_is_unchanged_after_a_save_outside_any_update_of_the_workspace()
    {
        // The owner is told from inside `save`, and updates the workspace in answer: the save must
        // not be made from inside an update of the workspace, or that is an update inside one.
        let scratch = Scratch::new("nested");
        let path = scratch.file("a.rs", "a");
        let told = Rc::new(Cell::new(0));
        let counted = Rc::clone(&told);
        let mut space = workspace(&scratch);
        space
            .open(&path, Rc::new(move || counted.set(counted.get() + 1)))
            .unwrap();
        space.documents[0]
            .controller
            .set_text("b");
        let document = Rc::clone(&space.documents[0]);
        document.save().unwrap();
        assert_eq!(told.get(), 2, "changed, then unchanged");
    }

    #[test]
    fn the_rows_are_kept_until_something_changes_them() {
        let scratch = Scratch::new("rows");
        scratch.file("a.rs", "");
        let mut space = workspace(&scratch);
        let first = space.rows();
        assert!(
            Rc::ptr_eq(&first, &space.rows()),
            "asking again is not reading again"
        );
        scratch.file("b.rs", "");
        assert_eq!(
            space.rows().len(),
            1,
            "a file made on disk is not seen until the tree is refreshed"
        );
        space.refresh();
        assert_eq!(space.rows().len(), 2);
    }

    #[test]
    fn the_folder_to_start_on_is_the_first_that_is_one() {
        let scratch = Scratch::new("choose");
        let folder = scratch.0.join("project");
        std::fs::create_dir_all(&folder).unwrap();
        let file = scratch.file("project/main.rs", "");
        let built = scratch.0.clone();
        let missing = PathBuf::from("/no/such/folder");
        // A folder named on the command line, or the folder a named file is in, wins.
        assert_eq!(
            choose_root(
                Some(folder.clone()),
                Some(missing.clone()),
                None,
                &built,
                None
            ),
            folder
        );
        assert_eq!(choose_root(Some(file), None, None, &built, None), folder);
        // Then the variable; then where the app was started, unless that is the root of the disk.
        assert_eq!(
            choose_root(
                None,
                Some(folder.clone()),
                Some(built.clone()),
                &built,
                None
            ),
            folder
        );
        assert_eq!(
            choose_root(None, None, Some(folder.clone()), &built, None),
            folder
        );
        assert_eq!(
            choose_root(None, None, Some(PathBuf::from("/")), &folder, None),
            folder,
            "an app started from the desktop starts at /"
        );
        // What is not a folder is passed over, and with nothing left it is the folder it is in.
        assert_eq!(
            choose_root(
                Some(missing.clone()),
                Some(missing.clone()),
                Some(missing),
                &built,
                Some(folder.clone())
            ),
            built
        );
        assert_eq!(
            choose_root(None, None, None, Path::new("/no/such"), None),
            PathBuf::from(".")
        );
    }
}
