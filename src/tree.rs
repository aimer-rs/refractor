//! The folder as a list of rows: what the file tree shows.
//!
//! A directory is read when it is opened, and not before, so a folder of a million files that is
//! never opened costs nothing. The rows are the entries of the open directories, one level deeper
//! under each, directories first.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

/// What is skipped: the repository's own storage and the files a file manager leaves behind.
const HIDDEN: [&str; 2] = [".git", ".DS_Store"];

/// One entry of a directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
}

/// One row of the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub path: PathBuf,
    pub name: String,
    /// How many directories it is inside of, counting from the root's children at zero.
    pub depth: usize,
    pub is_dir: bool,
    /// Whether it is a directory that is open.
    pub expanded: bool,
}

/// The entries of `dir`: directories first, then files, each group in alphabetical order that does
/// not mind case. A name that is not valid text is shown as it best can be.
pub fn read_dir(dir: &Path) -> io::Result<Vec<Entry>> {
    let mut entries: Vec<Entry> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry
                .file_name()
                .to_string_lossy()
                .into_owned();
            if HIDDEN.contains(&name.as_str()) {
                return None;
            }
            // A link to a directory is a directory; one that points nowhere is a file.
            let is_dir = entry.path().is_dir();
            Some(Entry {
                path: entry.path(),
                name,
                is_dir,
            })
        })
        .collect();
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| {
                a.name
                    .to_lowercase()
                    .cmp(&b.name.to_lowercase())
            })
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(entries)
}

/// The rows under `root`, with the directories in `expanded` open. A directory that cannot be read
/// is open and empty.
pub fn rows(root: &Path, expanded: &HashSet<PathBuf>) -> Vec<Row> {
    let mut rows = Vec::new();
    push_rows(root, 0, expanded, &mut rows);
    rows
}

fn push_rows(dir: &Path, depth: usize, expanded: &HashSet<PathBuf>, rows: &mut Vec<Row>) {
    let Ok(entries) = read_dir(dir) else {
        return;
    };
    for entry in entries {
        let open = entry.is_dir && expanded.contains(&entry.path);
        rows.push(Row {
            path: entry.path.clone(),
            name: entry.name,
            depth,
            is_dir: entry.is_dir,
            expanded: open,
        });
        if open {
            push_rows(&entry.path, depth + 1, expanded, rows);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder in the temporary directory that is removed when it is dropped.
    pub struct Scratch(pub PathBuf);

    impl Scratch {
        pub fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("refractor-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("a temporary directory");
            Self(dir)
        }

        pub fn file(&self, path: &str, text: &str) -> PathBuf {
            let path = self.0.join(path);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
            std::fs::write(&path, text).expect("written");
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn names(rows: &[Row]) -> Vec<(usize, &str)> {
        rows.iter()
            .map(|row| (row.depth, row.name.as_str()))
            .collect()
    }

    #[test]
    fn a_directory_lists_its_folders_first_then_its_files_without_minding_case() {
        let scratch = Scratch::new("order");
        scratch.file("b.rs", "");
        scratch.file("A.rs", "");
        scratch.file("zeta/inner.rs", "");
        scratch.file("Alpha/inner.rs", "");
        scratch.file(".git/HEAD", "");
        scratch.file(".gitignore", "");
        scratch.file(".DS_Store", "");
        let entries = read_dir(&scratch.0).unwrap();
        let listed: Vec<&str> = entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(
            listed,
            ["Alpha", "zeta", ".gitignore", "A.rs", "b.rs"],
            "folders, then files; .git and .DS_Store are hidden"
        );
        assert!(entries[0].is_dir && !entries[4].is_dir);
    }

    #[test]
    fn only_the_open_directories_are_read_and_their_rows_are_one_level_deeper() {
        let scratch = Scratch::new("rows");
        scratch.file("src/main.rs", "");
        scratch.file("src/deep/mod.rs", "");
        scratch.file("README.md", "");
        let mut expanded = HashSet::new();
        assert_eq!(
            names(&rows(&scratch.0, &expanded)),
            [(0, "src"), (0, "README.md")]
        );

        expanded.insert(scratch.0.join("src"));
        let open = rows(&scratch.0, &expanded);
        assert_eq!(
            names(&open),
            [(0, "src"), (1, "deep"), (1, "main.rs"), (0, "README.md")]
        );
        assert!(open[0].expanded && !open[1].expanded);

        expanded.insert(scratch.0.join("src/deep"));
        assert_eq!(
            names(&rows(&scratch.0, &expanded)),
            [
                (0, "src"),
                (1, "deep"),
                (2, "mod.rs"),
                (1, "main.rs"),
                (0, "README.md")
            ]
        );
        // Opening something that is not a directory does nothing.
        expanded.insert(scratch.0.join("README.md"));
        assert_eq!(rows(&scratch.0, &expanded).len(), 5);
    }

    #[test]
    fn a_missing_directory_has_no_rows_and_reading_it_is_an_error() {
        let missing = std::env::temp_dir().join("refractor-no-such-directory");
        assert!(read_dir(&missing).is_err());
        assert!(rows(&missing, &HashSet::new()).is_empty());
    }
}

#[cfg(test)]
pub use tests::Scratch;
