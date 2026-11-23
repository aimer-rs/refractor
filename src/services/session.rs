//! What was open in a folder when it was last used: the files, where the caret was in each, which tab was shown, and which folders of the
//! file tree were open, kept in a file of
//! its own for each folder, so that opening the folder again brings the editor back to where it was left.
//!
//! The caret is kept as a line (from 1) and a byte of it (from 0), which is how [`Workspace::go_to_line`] is told a place. The session is
//! looked at now and then and written only when it is not what was last written, so typing costs one comparison of a few paths and
//! numbers a second, and not a write.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use aimer::{ProviderHandle, Venus};
use serde::{Deserialize, Serialize};

use crate::services::settings::Settings;
use crate::model::workspace::{Workspace, notifier};

/// A file that was open, and where its caret was.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Remembered {
    pub path: String,
    /// The line of the caret, from 1.
    pub line: usize,
    /// The byte of the line the caret was at, from 0.
    pub column: usize,
    /// Whether the tab was pinned.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
}

/// The files of a folder that were open, in the order of their tabs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub files: Vec<Remembered>,
    /// The tab that was shown, as an index of `files`. `None` where the settings were shown, or nothing was open.
    pub active: Option<usize>,
    /// The folders of the file tree that were open, as paths sorted so that the same tree is always the same text.
    pub folders: Vec<String>,
}

impl Session {
    /// How often the session is looked at for a change.
    pub const INTERVAL: Duration = Duration::from_secs(1);
    /// The folder, beside the settings, that the sessions are kept in.
    pub const FOLDER: &'static str = "sessions";

    /// What `space` has open now.
    pub fn of(space: &Workspace) -> Self {
        let files = space
            .documents
            .iter()
            .map(|document| {
                let snapshot = document.controller.snapshot();
                let caret = document
                    .controller
                    .value()
                    .selection()
                    .focus()
                    .min(snapshot.len());
                let line = snapshot.byte_to_line(caret);
                Remembered {
                    path: document.path.to_string_lossy().into_owned(),
                    line: line + 1,
                    column: caret - snapshot.line_to_byte(line),
                    pinned: space.is_pinned(document.id),
                }
            })
            .collect();
        let active = space.active.and_then(|id| {
            space
                .documents
                .iter()
                .position(|document| document.id == id)
        });
        let folders = space
            .expanded_folders()
            .into_iter()
            .map(|folder| folder.to_string_lossy().into_owned())
            .collect();
        Self {
            files,
            active,
            folders,
        }
    }

    /// Opens the folders of the tree again, then the files, each at its caret, and shows the tab that was shown. A file that is gone, or cannot be opened, is
    /// left out. Does nothing when something is open already, so that what the user chose is not replaced. Returns whether any file was
    /// opened.
    pub fn restore(&self, space: &mut Workspace, notify: &Rc<dyn Fn()>) -> bool {
        // The tree is brought back whether or not there are files to bring back, and not over what the user has opened already.
        space.restore_expanded(self.folders.iter().map(PathBuf::from));
        if !space.documents.is_empty() {
            return false;
        }
        for file in &self.files {
            let path = Path::new(&file.path);
            if path.is_file() {
                let _ = space.go_to_line(path, file.line, file.column, 0, Rc::clone(notify));
                if file.pinned
                    && let Some(id) = space
                        .documents
                        .iter()
                        .find(|document| document.path == path)
                        .map(|document| document.id)
                {
                    space.pin_tab(id);
                }
            }
        }
        let shown = self
            .active
            .and_then(|at| self.files.get(at))
            .and_then(|file| {
                space
                    .documents
                    .iter()
                    .find(|document| document.path == Path::new(&file.path))
            })
            .map(|document| document.id);
        if let Some(id) = shown {
            space.activate(id);
        }
        !space.documents.is_empty()
    }

    /// The text of the file that holds the session.
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("paths as text and numbers are TOML")
    }

    /// Reads a session from text; an error says why it is not one.
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// The session in `path`: nothing for a file that is not there or is not a session, which the next write replaces.
    pub fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| Self::from_toml(&text).ok())
            .unwrap_or_default()
    }

    /// Writes the session to `path`, making the folder: whole or not at all, as the settings are.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let mut temporary = path.as_os_str().to_owned();
        temporary.push(".tmp");
        let temporary = PathBuf::from(temporary);
        let written = std::fs::write(&temporary, self.to_toml())
            .and_then(|()| std::fs::rename(&temporary, path));
        if written.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        written
    }

    /// The file for the folder `root`, among the sessions in `config`, which is the folder the settings are in. The name has the name of
    /// the folder, to be read by a person, and a hash of the whole path, to tell two folders of the same name apart.
    pub fn file_in(config: &Path, root: &Path) -> PathBuf {
        // FNV-1a, which is the same on every run and every version, as `DefaultHasher` is not promised to be.
        let hash = root
            .to_string_lossy()
            .bytes()
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
            });
        let name: String = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .take(40)
            .collect();
        config
            .join(Self::FOLDER)
            .join(format!("{name}-{hash:016x}.toml"))
    }

    /// The file of the folder `root` for this user, beside the settings. `None` where there is no place for the settings.
    pub fn file(root: &Path) -> Option<PathBuf> {
        let settings = Settings::file()?;
        Some(Self::file_in(settings.parent()?, root))
    }

    /// Brings back what was open in the folder of `store`, if nothing is open, and returns whether it did.
    pub fn restore_from_disk(store: &ProviderHandle<Workspace>) -> bool {
        let root = store.read().root.clone();
        let Some(file) = Self::file(&root) else {
            return false;
        };
        let session = Self::load_from(&file);
        let notify = notifier(store);
        let mut restored = false;
        store.update(|space| restored = session.restore(space, &notify));
        restored
    }

    /// Looks at what is open now and then, and writes it when it is not what was last written. The write is off the UI thread. Does
    /// nothing outside the runtime.
    pub fn keep(store: ProviderHandle<Workspace>) {
        let Some(venus) = Venus::current() else {
            return;
        };
        let sleeper = venus.clone();
        venus.spawn(async move {
            let mut last: Option<(PathBuf, Session)> = None;
            loop {
                let _ = sleeper.sleep(Self::INTERVAL).await;
                let (root, session) = {
                    let space = store.read();
                    (space.root.clone(), Self::of(&space))
                };
                if last
                    .as_ref()
                    .is_some_and(|(was, kept)| *was == root && *kept == session)
                {
                    continue;
                }
                if let Some(file) = Self::file(&root) {
                    let writing = session.clone();
                    let _ = sleeper
                        .spawn_blocking(move || {
                            if let Err(error) = writing.save_to(&file) {
                                aimer::console::warn!(
                                    "Could not save the session {}: {error}",
                                    file.display()
                                );
                            }
                        })
                        .await;
                }
                last = Some((root, session));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tree::Scratch;

    fn notify() -> Rc<dyn Fn()> {
        Rc::new(|| {})
    }

    fn remembered(path: &Path, line: usize, column: usize) -> Remembered {
        Remembered {
            path: path.to_string_lossy().into_owned(),
            line,
            column,
            pinned: false,
        }
    }

    #[test]
    fn a_session_survives_a_round_trip_and_a_file_of_nothing_is_an_empty_one() {
        let session = Session {
            files: vec![remembered(Path::new("/p/Cargo.toml"), 3, 0)],
            active: Some(0),
            folders: vec!["/p/src".to_owned()],
        };
        assert_eq!(Session::from_toml(&session.to_toml()), Ok(session));
        assert_eq!(Session::from_toml(""), Ok(Session::default()));
        assert!(Session::from_toml("files = 3").is_err());
    }

    #[test]
    fn what_is_open_is_read_with_the_line_and_the_byte_of_the_caret_and_the_tab_shown() {
        let dir = Scratch::new("session-of");
        let cargo = dir.file("Cargo.toml", "[package]\nname = \"é\"\n");
        let main = dir.file("src/main.rs", "fn main() {}\n\nfn other() {}");
        let mut space = Workspace::new(dir.0.clone());
        space.open(&cargo, notify()).expect("opens");
        space.open(&main, notify()).expect("opens");
        // A byte inside the last line, after a two-byte character on the line above it.
        let at = "[package]\nname = \"é".len();
        space.documents[0]
            .controller
            .select_bytes(at, at);
        let last = "fn main() {}\n\nfn ".len();
        space.documents[1]
            .controller
            .select_bytes(last, last);
        space.activate(space.documents[0].id);
        assert_eq!(
            Session::of(&space),
            Session {
                files: vec![
                    remembered(&cargo, 2, "name = \"é".len()),
                    remembered(&main, 3, 3),
                ],
                active: Some(0),
                folders: Vec::new(),
            }
        );
    }

    #[test]
    fn nothing_open_is_an_empty_session_and_the_settings_tab_is_not_a_file() {
        let dir = Scratch::new("session-empty");
        let mut space = Workspace::new(dir.0.clone());
        assert_eq!(Session::of(&space), Session::default());
        space.open_settings();
        assert_eq!(Session::of(&space).active, None);
    }

    #[test]
    fn the_files_are_opened_again_at_their_lines_and_the_tab_that_was_shown_is_shown() {
        let dir = Scratch::new("session-restore");
        let cargo = dir.file("Cargo.toml", "one\ntwo\nthree\n");
        let main = dir.file("src/main.rs", "a\nb\nc\nd\n");
        let gone = dir.0.join("gone.rs");
        let session = Session {
            files: vec![
                remembered(&cargo, 3, 2),
                remembered(&gone, 1, 0),
                remembered(&main, 4, 0),
            ],
            active: Some(0),
            ..Session::default()
        };
        let mut space = Workspace::new(dir.0.clone());
        assert!(session.restore(&mut space, &notify()));
        let opened: Vec<&Path> = space
            .documents
            .iter()
            .map(|document| document.path.as_path())
            .collect();
        assert_eq!(
            opened,
            [cargo.as_path(), main.as_path()],
            "the missing file is left out"
        );
        assert_eq!(space.active_document().expect("shown").path, cargo);
        assert_eq!(
            space.documents[0]
                .controller
                .value()
                .selection()
                .focus(),
            "one\ntwo\nth".len()
        );
        assert_eq!(
            space.documents[1]
                .controller
                .value()
                .selection()
                .focus(),
            "a\nb\nc\n".len()
        );
        assert_eq!(Session::of(&space).active, Some(0));
    }

    #[test]
    fn a_line_or_a_column_past_the_end_is_the_end_and_a_session_does_not_replace_what_is_open() {
        let dir = Scratch::new("session-bounds");
        let a = dir.file("a.txt", "ab\ncd");
        let b = dir.file("b.txt", "");
        let session = Session {
            files: vec![remembered(&a, 99, 99), remembered(&b, 5, 5)],
            active: Some(7),
            ..Session::default()
        };
        let mut space = Workspace::new(dir.0.clone());
        assert!(session.restore(&mut space, &notify()));
        assert_eq!(
            space.documents[0]
                .controller
                .value()
                .selection()
                .focus(),
            5
        );
        assert_eq!(
            space.documents[1]
                .controller
                .value()
                .selection()
                .focus(),
            0
        );
        assert!(!session.restore(&mut space, &notify()), "something is open");
        assert_eq!(space.documents.len(), 2);
    }

    #[test]
    fn a_session_of_no_file_that_exists_opens_nothing() {
        let dir = Scratch::new("session-none");
        let mut space = Workspace::new(dir.0.clone());
        let session = Session {
            files: vec![remembered(&dir.0.join("gone.rs"), 1, 0)],
            active: Some(0),
            ..Session::default()
        };
        assert!(!session.restore(&mut space, &notify()));
        assert!(space.documents.is_empty());
    }

    #[test]
    fn a_session_is_written_whole_and_a_bad_or_missing_file_reads_as_nothing() {
        let dir = Scratch::new("session-file");
        let path = dir.0.join("inside").join("one.toml");
        assert_eq!(Session::load_from(&path), Session::default());
        let session = Session {
            files: vec![remembered(Path::new("/p/a.rs"), 9, 1)],
            active: Some(0),
            ..Session::default()
        };
        session
            .save_to(&path)
            .expect("the folder is made");
        assert_eq!(Session::load_from(&path), session);
        let left: Vec<_> = std::fs::read_dir(path.parent().expect("a folder"))
            .expect("read")
            .map(|entry| entry.expect("an entry").file_name())
            .collect();
        assert_eq!(left, ["one.toml"], "no temporary file is left");
        std::fs::write(&path, "not [ toml").expect("written");
        assert_eq!(Session::load_from(&path), Session::default());
    }

    #[test]
    fn the_open_folders_of_the_tree_are_read_sorted_and_come_back_where_they_still_exist() {
        let dir = Scratch::new("session-folders");
        dir.file("src/deep/mod.rs", "");
        dir.file("docs/a.md", "");
        let mut space = Workspace::new(dir.0.clone());
        space.toggle(&dir.0.join("src"));
        space.toggle(&dir.0.join("src/deep"));
        space.toggle(&dir.0.join("docs"));
        let session = Session::of(&space);
        assert_eq!(
            session.folders,
            [
                dir.0
                    .join("docs")
                    .to_string_lossy()
                    .into_owned(),
                dir.0.join("src").to_string_lossy().into_owned(),
                dir.0
                    .join("src/deep")
                    .to_string_lossy()
                    .into_owned(),
            ],
            "sorted, so the same tree is the same text"
        );
        assert_eq!(session, Session::of(&space));

        let mut again = Workspace::new(dir.0.clone());
        let mut gone = session.clone();
        gone.folders.push(
            dir.0
                .join("removed")
                .to_string_lossy()
                .into_owned(),
        );
        gone.folders.push("/somewhere/else".to_owned());
        assert!(!gone.restore(&mut again, &notify()), "no file was open");
        assert_eq!(Session::of(&again).folders, session.folders);
        let rows = again.rows();
        let names: Vec<&str> = rows
            .iter()
            .map(|row| row.name.as_str())
            .collect();
        assert!(
            names.contains(&"deep") && names.contains(&"a.md"),
            "{names:?}"
        );
    }

    #[test]
    fn a_session_from_before_the_tree_was_kept_has_no_folders_and_the_tree_is_not_changed_by_it() {
        let old = Session::from_toml("active = 0\n").expect("reads");
        assert!(old.folders.is_empty());
        let dir = Scratch::new("session-folders-old");
        dir.file("src/main.rs", "");
        let mut space = Workspace::new(dir.0.clone());
        space.toggle(&dir.0.join("src"));
        old.restore(&mut space, &notify());
        assert_eq!(
            Session::of(&space).folders,
            [dir.0.join("src").to_string_lossy().into_owned()],
            "what the user opened is not closed"
        );
    }

    #[test]
    fn each_folder_has_a_file_of_its_own_that_is_the_same_every_time() {
        let config = Path::new("/c");
        let a = Session::file_in(config, Path::new("/work/a/app"));
        assert_eq!(a, Session::file_in(config, Path::new("/work/a/app")));
        assert_ne!(a, Session::file_in(config, Path::new("/work/b/app")));
        let name = a
            .file_name()
            .expect("a name")
            .to_string_lossy()
            .into_owned();
        assert!(
            name.starts_with("app-") && name.ends_with(".toml"),
            "{name}"
        );
        assert_eq!(a.parent(), Some(Path::new("/c/sessions")));
        let odd = Session::file_in(config, Path::new("/work/my project/ü?"));
        assert!(
            odd.file_name()
                .expect("a name")
                .to_string_lossy()
                .starts_with("ü_-")
        );
        assert!(Session::file_in(config, Path::new("/")).starts_with("/c/sessions"));
    }
}
