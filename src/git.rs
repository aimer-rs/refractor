//! The branch the project is on, read from the repository's own files.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The branch checked out in the repository that holds `dir`, or the start of the commit when
/// there is no branch (a detached head). `None` when `dir` is not in a repository.
///
/// It reads `.git/HEAD` and nothing else, so it is a file read and not a program run.
pub fn branch_of(dir: &Path) -> Option<String> {
    let git = dir
        .ancestors()
        .map(|ancestor| ancestor.join(".git"))
        .find(|git| git.exists())?;
    let head = std::fs::read_to_string(git_directory(&git)?.join("HEAD")).ok()?;
    let head = head.trim();
    match head.strip_prefix("ref: ") {
        Some(reference) => Some(
            reference
                .strip_prefix("refs/heads/")
                .unwrap_or(reference)
                .to_owned(),
        ),
        None if head.len() >= 7 => Some(head[..7].to_owned()),
        None => None,
    }
}

/// Where the repository's files are: `.git` itself, or what a `.git` file points to, as it does
/// in a linked working tree.
fn git_directory(git: &Path) -> Option<PathBuf> {
    if git.is_dir() {
        return Some(git.to_path_buf());
    }
    let pointer = std::fs::read_to_string(git).ok()?;
    let target = Path::new(
        pointer
            .trim()
            .strip_prefix("gitdir: ")?,
    );
    Some(if target.is_absolute() {
        target.to_path_buf()
    } else {
        git.parent()?.join(target)
    })
}

/// How git sees a file that is not as it was committed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Modified,
    Added,
    Deleted,
    Renamed,
    /// Not tracked by git yet.
    Untracked,
    /// Both sides of a merge changed it.
    Conflict,
    /// Listed in a `.gitignore`: not a change, but shown as the quiet thing it is.
    Ignored,
}

impl Change {
    /// The letter next to a file's name, as in most editors: none for an ignored file.
    pub fn letter(self) -> Option<&'static str> {
        match self {
            Self::Modified => Some("M"),
            Self::Added => Some("A"),
            Self::Deleted => Some("D"),
            Self::Renamed => Some("R"),
            Self::Untracked => Some("U"),
            Self::Conflict => Some("!"),
            Self::Ignored => None,
        }
    }
}

/// What git says of the files of a folder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    files: HashMap<PathBuf, Change>,
    /// The folders with a change somewhere inside them (an ignored file is not one).
    folders: HashSet<PathBuf>,
}

impl Status {
    /// How git sees `path`: by its own entry, or by the ignored folder it is inside.
    pub fn change_of(&self, path: &Path) -> Option<Change> {
        if let Some(change) = self.files.get(path) {
            return Some(*change);
        }
        path.ancestors()
            .skip(1)
            .find(|ancestor| self.files.get(*ancestor) == Some(&Change::Ignored))
            .map(|_| Change::Ignored)
    }

    /// Whether something inside the folder `path` is changed.
    pub fn has_changes_inside(&self, path: &Path) -> bool {
        self.folders.contains(path)
    }

    /// Whether anything at all is changed: what a `*` after the branch name says.
    pub fn is_dirty(&self) -> bool {
        !self.folders.is_empty()
            || self
                .files
                .values()
                .any(|change| *change != Change::Ignored)
    }

    /// The status from the output of `git status --porcelain=v1 -z`, whose paths are relative to
    /// `top`, the top of the repository.
    pub fn parse(output: &str, top: &Path) -> Self {
        let mut status = Self::default();
        let mut entries = output.split('\0');
        while let Some(entry) = entries.next() {
            let (Some(codes), Some(path)) = (entry.get(..2), entry.get(3..)) else {
                continue;
            };
            let codes = codes.as_bytes();
            // A rename or a copy is followed by the path it came from, which is not an entry of its own.
            if codes
                .iter()
                .any(|code| matches!(code, b'R' | b'C'))
            {
                entries.next();
            }
            let change = match (codes[0], codes[1]) {
                (b'!', b'!') => Change::Ignored,
                (b'?', b'?') => Change::Untracked,
                (b'U', _) | (_, b'U') | (b'A', b'A') | (b'D', b'D') => Change::Conflict,
                (b'R' | b'C', _) | (_, b'R' | b'C') => Change::Renamed,
                (b'D', _) | (_, b'D') => Change::Deleted,
                (b'A', _) => Change::Added,
                _ => Change::Modified,
            };
            let path = top.join(path.trim_end_matches('/'));
            if change != Change::Ignored {
                for ancestor in path
                    .ancestors()
                    .skip(1)
                    .take_while(|ancestor| ancestor.starts_with(top))
                {
                    status
                        .folders
                        .insert(ancestor.to_path_buf());
                }
            }
            status
                .files
                .insert(path, change);
        }
        status
    }
}

/// Asks git for the status of the repository that holds `dir`. `None` when `dir` is not in one, or
/// git cannot be run: a program is run and waited for, so call it off the UI thread.
pub fn status_of(dir: &Path) -> Option<Status> {
    let top = run(dir, &["rev-parse", "--show-toplevel"])?;
    let top = PathBuf::from(top.trim());
    // Untracked folders are named, not listed, and so are ignored ones: a build folder of thousands
    // of files is one entry.
    let output = run(
        dir,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=normal",
            "--ignored=traditional",
        ],
    )?;
    Some(Status::parse(&output, &top))
}

fn run(dir: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(arguments)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Scratch;

    fn parse(output: &str) -> Status {
        Status::parse(output, Path::new("/repo"))
    }

    #[test]
    fn each_kind_of_entry_is_a_change() {
        let status = parse(
            " M src/a.rs\0M  src/b.rs\0A  new.rs\0 D gone.rs\0?? notes.txt\0UU merge.rs\0!! target/\0",
        );
        assert_eq!(
            status.change_of(Path::new("/repo/src/a.rs")),
            Some(Change::Modified)
        );
        assert_eq!(
            status.change_of(Path::new("/repo/src/b.rs")),
            Some(Change::Modified),
            "staged or not, it is modified"
        );
        assert_eq!(
            status.change_of(Path::new("/repo/new.rs")),
            Some(Change::Added)
        );
        assert_eq!(
            status.change_of(Path::new("/repo/gone.rs")),
            Some(Change::Deleted)
        );
        assert_eq!(
            status.change_of(Path::new("/repo/notes.txt")),
            Some(Change::Untracked)
        );
        assert_eq!(
            status.change_of(Path::new("/repo/merge.rs")),
            Some(Change::Conflict)
        );
        assert_eq!(
            status.change_of(Path::new("/repo/target")),
            Some(Change::Ignored)
        );
        assert_eq!(status.change_of(Path::new("/repo/clean.rs")), None);
    }

    #[test]
    fn what_is_inside_an_ignored_folder_is_ignored() {
        let status = parse("!! target/\0");
        assert_eq!(
            status.change_of(Path::new("/repo/target/debug/x")),
            Some(Change::Ignored)
        );
        assert_eq!(
            status.change_of(Path::new("/repo/targets/x")),
            None,
            "a folder with the same start is another folder"
        );
    }

    #[test]
    fn a_folder_has_changes_inside_when_a_file_in_it_changed_but_not_when_it_is_only_ignored() {
        let status = parse(" M src/screen/a.rs\0!! target/\0");
        assert!(status.has_changes_inside(Path::new("/repo/src")));
        assert!(status.has_changes_inside(Path::new("/repo/src/screen")));
        assert!(!status.has_changes_inside(Path::new("/repo/target")));
        assert!(!status.has_changes_inside(Path::new("/repo/other")));
        assert!(status.is_dirty());
        assert!(
            !parse("!! target/\0").is_dirty(),
            "ignored files are not a change"
        );
        assert!(!parse("").is_dirty());
    }

    #[test]
    fn a_rename_is_one_entry_and_its_old_name_is_not_another() {
        let status = parse("R  new.rs\0old.rs\0 M other.rs\0");
        assert_eq!(
            status.change_of(Path::new("/repo/new.rs")),
            Some(Change::Renamed)
        );
        assert_eq!(status.change_of(Path::new("/repo/old.rs")), None);
        assert_eq!(
            status.change_of(Path::new("/repo/other.rs")),
            Some(Change::Modified),
            "the entry after it is read"
        );
    }

    #[test]
    fn names_with_spaces_and_odd_output_are_handled() {
        let status = parse(" M with space.rs\0\0x\0 M\0 M é.rs\0");
        assert_eq!(
            status.change_of(Path::new("/repo/with space.rs")),
            Some(Change::Modified)
        );
        assert_eq!(
            status.change_of(Path::new("/repo/é.rs")),
            Some(Change::Modified)
        );
    }

    #[test]
    fn a_folder_that_is_not_a_repository_has_no_status() {
        let scratch = Scratch::new("nostatus");
        assert_eq!(status_of(&scratch.0.join("missing")), None);
    }

    #[test]
    fn the_branch_is_what_head_points_to() {
        let scratch = Scratch::new("branch");
        scratch.file(".git/HEAD", "ref: refs/heads/main\n");
        assert_eq!(branch_of(&scratch.0).as_deref(), Some("main"));
        scratch.file(".git/HEAD", "ref: refs/heads/feature/editor\n");
        assert_eq!(
            branch_of(&scratch.0).as_deref(),
            Some("feature/editor"),
            "a branch name may have slashes"
        );
    }

    #[test]
    fn a_directory_inside_the_repository_is_on_its_branch_and_one_outside_is_on_none() {
        let scratch = Scratch::new("inside");
        scratch.file(".git/HEAD", "ref: refs/heads/dev\n");
        let inner = scratch.file("src/deep/file.rs", "");
        assert_eq!(branch_of(inner.parent().unwrap()).as_deref(), Some("dev"));
        assert_eq!(branch_of(Path::new("/")), None);
    }

    #[test]
    fn a_detached_head_is_the_start_of_the_commit() {
        let scratch = Scratch::new("detached");
        scratch.file(".git/HEAD", "0123456789abcdef0123456789abcdef01234567\n");
        assert_eq!(branch_of(&scratch.0).as_deref(), Some("0123456"));
    }

    #[test]
    fn a_linked_working_tree_names_its_git_directory_in_a_file() {
        let scratch = Scratch::new("linked");
        scratch.file("real/HEAD", "ref: refs/heads/linked\n");
        scratch.file(
            "tree/.git",
            &format!("gitdir: {}\n", scratch.0.join("real").display()),
        );
        assert_eq!(
            branch_of(&scratch.0.join("tree")).as_deref(),
            Some("linked")
        );
        scratch.file("tree2/.git", "gitdir: ../real\n");
        assert_eq!(
            branch_of(&scratch.0.join("tree2")).as_deref(),
            Some("linked"),
            "a relative pointer is from the file"
        );
    }

    #[test]
    fn an_unreadable_head_is_no_branch() {
        let scratch = Scratch::new("broken");
        scratch.file(".git/HEAD", "x");
        assert_eq!(branch_of(&scratch.0), None);
    }
}
