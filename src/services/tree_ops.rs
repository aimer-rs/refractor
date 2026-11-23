//! Filesystem operations for the file tree: creating files and folders, renaming, deleting, copying, moving, and revealing in the
//! system's file manager.
//!
//! Pure filesystem logic with no widget or UI dependencies, so every edge case — invalid names, collisions, recursive copies,
//! case-only renames, and moving a folder into itself — is tested directly.

use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

/// One or more files or folders placed on the file tree's clipboard by Cut or Copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileClipboard {
    pub paths: Vec<PathBuf>,
    /// `true` for Cut (move on paste), `false` for Copy (duplicate on paste).
    pub cut: bool,
}

/// What a typed file or folder name is worth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameVerdict {
    /// Unchanged from the current name: nothing to rename.
    Same,
    /// Cannot be used as a file or folder name, and why.
    Invalid(&'static str),
    /// A valid name to create or rename to.
    Valid(String),
}

/// Messages explaining why a name was rejected.
pub struct NameError;

impl NameError {
    pub const EMPTY: &'static str = "Name cannot be empty";
    pub const SEPARATOR: &'static str = "Name cannot contain path separators";
    pub const DOTS: &'static str = "Name cannot be '.' or '..'";
    pub const NUL: &'static str = "Name contains an invalid character";
}

/// Validates a file or folder name typed by the user. When `old` is `Some`, matching it returns [`NameVerdict::Same`].
pub fn validate_name(typed: &str, old: Option<&str>) -> NameVerdict {
    let trimmed = typed.trim();
    if trimmed.is_empty() {
        return NameVerdict::Invalid(NameError::EMPTY);
    }
    if trimmed == "." || trimmed == ".." {
        return NameVerdict::Invalid(NameError::DOTS);
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return NameVerdict::Invalid(NameError::SEPARATOR);
    }
    if trimmed.contains('\0') {
        return NameVerdict::Invalid(NameError::NUL);
    }
    if old == Some(trimmed) {
        return NameVerdict::Same;
    }
    NameVerdict::Valid(trimmed.to_owned())
}

/// The byte range of `name` to select when opening a rename prompt: the stem before the extension for ordinary files, and the whole
/// name for folders and dotfiles without a further extension.
pub fn rename_selection(name: &str, is_dir: bool) -> (usize, usize) {
    if is_dir {
        return (0, name.len());
    }
    // A leading dot (e.g. `.gitignore`) is not an extension separator.
    match name.rfind('.') {
        Some(dot) if dot > 0 => (0, dot),
        _ => (0, name.len()),
    }
}

/// The directory that a New File, New Folder, or Paste action targets: the folder itself when `entry` is a directory, its parent when
/// `entry` is a file, and `root` when nothing is targeted.
pub fn target_dir(root: &Path, entry: Option<(&Path, bool)>) -> PathBuf {
    match entry {
        Some((path, true)) => path.to_path_buf(),
        Some((path, false)) => path.parent().unwrap_or(root).to_path_buf(),
        None => root.to_path_buf(),
    }
}

/// Creates a new empty file named `name` inside `dir`. Fails with [`io::ErrorKind::AlreadyExists`] if an entry already exists there.
pub fn create_file(dir: &Path, name: &str) -> io::Result<PathBuf> {
    let path = dir.join(name);
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    Ok(path)
}

/// Creates a new folder named `name` inside `dir`. Fails with [`io::ErrorKind::AlreadyExists`] if an entry already exists there.
pub fn create_folder(dir: &Path, name: &str) -> io::Result<PathBuf> {
    let path = dir.join(name);
    fs::create_dir(&path)?;
    Ok(path)
}

/// Renames `old` to `new_name` in the same parent directory.
///
/// Refuses to overwrite an existing distinct entry, while still allowing case-only renames (such as `foo.rs` -> `Foo.rs`) on
/// case-insensitive filesystems.
pub fn rename_entry(old: &Path, new_name: &str) -> io::Result<PathBuf> {
    let parent = old
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    let new_path = parent.join(new_name);
    if old == new_path {
        return Ok(new_path);
    }
    let case_only = old
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|old_name| old_name.eq_ignore_ascii_case(new_name));
    if !case_only && new_path.symlink_metadata().is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{new_name} already exists"),
        ));
    }
    fs::rename(old, &new_path)?;
    Ok(new_path)
}

/// Permanently deletes `path`: removes a file or symlink with `remove_file`, and a directory recursively with `remove_dir_all`.
pub fn delete_entry(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Chooses a non-colliding destination path inside `dest_dir` for copying an entry named `src_name`.
///
/// Returns `dest_dir/src_name` if free, then `"{stem} copy{ext}"`, `"{stem} copy 2{ext}"`, and so on.
pub fn unique_copy_path(dest_dir: &Path, src_name: &str, is_dir: bool) -> PathBuf {
    let direct = dest_dir.join(src_name);
    if direct.symlink_metadata().is_err() {
        return direct;
    }
    let (stem, ext) = if is_dir {
        (src_name, "")
    } else {
        match src_name.rfind('.') {
            Some(dot) if dot > 0 => (&src_name[..dot], &src_name[dot..]),
            _ => (src_name, ""),
        }
    };
    let first = dest_dir.join(format!("{stem} copy{ext}"));
    if first.symlink_metadata().is_err() {
        return first;
    }
    let mut n = 2u32;
    loop {
        let candidate = dest_dir.join(format!("{stem} copy {n}{ext}"));
        if candidate.symlink_metadata().is_err() {
            return candidate;
        }
        n += 1;
    }
}

/// Whether `dest` is `src` itself or a path inside `src`.
fn is_same_or_inside(src: &Path, dest: &Path) -> bool {
    dest == src || dest.starts_with(src)
}

/// Copies `src` (a file or directory) into `dest_dir`, generating a unique name on collision.
///
/// Refuses to copy a directory into itself or any of its descendants.
pub fn copy_entry(src: &Path, dest_dir: &Path) -> io::Result<PathBuf> {
    let meta = fs::symlink_metadata(src)?;
    let is_dir = meta.file_type().is_dir();
    if is_dir && is_same_or_inside(src, dest_dir) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot copy a folder into itself",
        ));
    }
    let name = src
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid source name"))?;
    let dest = unique_copy_path(dest_dir, name, is_dir);
    if is_dir {
        copy_dir_recursive(src, &dest)?;
    } else {
        fs::copy(src, &dest)?;
    }
    Ok(dest)
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> io::Result<()> {
    fs::create_dir(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let child_src = entry.path();
        let child_dest = dest.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir_recursive(&child_src, &child_dest)?;
        } else {
            fs::copy(&child_src, &child_dest)?;
        }
    }
    Ok(())
}

/// Whether `src` can be moved into `dest_dir` by drag and drop: rejects moving an entry into its own current parent, moving a folder
/// into itself or a descendant, or overwriting an existing entry in `dest_dir`.
pub fn can_drop(src: &Path, dest_dir: &Path) -> bool {
    if is_same_or_inside(src, dest_dir) {
        return false;
    }
    if src.parent() == Some(dest_dir) {
        return false;
    }
    let Some(name) = src.file_name() else {
        return false;
    };
    dest_dir.join(name).symlink_metadata().is_err()
}

/// Removes any path that is a descendant of another path in `paths` (and deduplicates exact matches), preserving order.
///
/// Used before bulk move, copy, or delete so operating on both a folder and a file inside it acts on the folder once instead of
/// failing on the already-moved or already-deleted child.
pub fn prune_descendants(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut unique: Vec<PathBuf> = Vec::with_capacity(paths.len());
    for path in paths {
        if !unique.contains(path) {
            unique.push(path.clone());
        }
    }
    unique
        .iter()
        .filter(|candidate| {
            !unique
                .iter()
                .any(|other| other != *candidate && candidate.starts_with(other))
        })
        .cloned()
        .collect()
}

/// Whether all entries in `sources` can be moved into `dest_dir` by drag and drop.
pub fn can_drop_all(sources: &[PathBuf], dest_dir: &Path) -> bool {
    !sources.is_empty()
        && sources
            .iter()
            .all(|src| can_drop(src, dest_dir))
}

/// Moves `src` into `dest_dir`, keeping its filename.
///
/// Refuses to move a directory into itself or any of its descendants, and refuses to overwrite an existing destination.
pub fn move_entry(src: &Path, dest_dir: &Path) -> io::Result<PathBuf> {
    let meta = fs::symlink_metadata(src)?;
    if meta.file_type().is_dir() && is_same_or_inside(src, dest_dir) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot move a folder into itself",
        ));
    }
    let name = src
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid source name"))?;
    let dest = dest_dir.join(name);
    if dest == src {
        return Ok(dest);
    }
    if dest.symlink_metadata().is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", name.to_string_lossy()),
        ));
    }
    fs::rename(src, &dest)?;
    Ok(dest)
}

/// Remaps `path` if it equals `old` or lies inside `old`, replacing that prefix with `new`.
pub fn remap_path(path: &Path, old: &Path, new: &Path) -> Option<PathBuf> {
    path.strip_prefix(old).ok().map(|tail| {
        if tail.as_os_str().is_empty() {
            new.to_path_buf()
        } else {
            new.join(tail)
        }
    })
}

/// Reveals `path` in the system's file manager (Finder on macOS, Explorer on Windows, default file manager on Unix).
///
/// When `open_folder_directly` is `true` and `path` is a directory (e.g. the workspace root), opens that folder directly instead of
/// selecting it in its parent.
pub fn reveal_in_file_manager(path: &Path, open_folder_directly: bool) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = std::process::Command::new("open");
        if !(open_folder_directly && path.is_dir()) {
            cmd.arg("-R");
        }
        cmd.arg(path).spawn()?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        let mut cmd = std::process::Command::new("explorer");
        if open_folder_directly && path.is_dir() {
            cmd.arg(path);
        } else {
            cmd.arg(format!("/select,{}", path.display()));
        }
        cmd.spawn()?;
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let target = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        let _ = open_folder_directly;
        std::process::Command::new("xdg-open")
            .arg(target)
            .spawn()?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/services/tree_ops.rs"]
mod tests;
