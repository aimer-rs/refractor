//! The files of the folder, found by their names.
//!
//! The folder is walked once, off the UI thread, into an index of the paths; what is typed after that only goes through
//! the index, which is the cost of the names and not of the disk.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use ignore::WalkBuilder;

use super::Pattern;

/// A file that fits what was typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileHit {
    pub path: PathBuf,
    /// The name of the file.
    pub name: String,
    /// The folder it is in, relative to the folder that was searched, written with `/`: empty for a file at its root.
    pub directory: String,
    /// The runs of the name that are the letters typed.
    pub marked: Vec<Range<usize>>,
    /// How well it fits, the best being the greatest: what lets it be put in a list with what is not a file.
    pub score: i32,
}

/// Splits a trailing `:line` or `:line:column` off what was typed, as in `src/main.rs:109:5`. The line and the column are
/// written from 1; the position returned is the line from 1 and the column from 0, which is how a file is gone to. A column
/// of 0 is taken as the start of the line. What has no such suffix, or nothing before it, is returned as it is.
pub fn split_position(query: &str) -> (&str, Option<(usize, usize)>) {
    let number = |text: &str| -> Option<usize> {
        (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
            .then(|| text.parse().ok())
            .flatten()
    };
    let trimmed = query.trim_end();
    let Some((rest, last)) = trimmed.rsplit_once(':') else {
        return (query, None);
    };
    let Some(last) = number(last) else {
        return (query, None);
    };
    if let Some((path, line)) = rest.rsplit_once(':')
        && let Some(line) = number(line)
        && !path.is_empty()
    {
        return (path, Some((line.max(1), last.saturating_sub(1))));
    }
    if rest.is_empty() {
        return (query, None);
    }
    (rest, Some((last.max(1), 0)))
}

/// A file of the index.
#[derive(Debug)]
struct Entry {
    /// The path relative to the folder, with `/` between its parts.
    relative: String,
    /// Where the name of the file starts in `relative`.
    name_start: usize,
}

impl Entry {
    fn name(&self) -> &str {
        &self.relative[self.name_start..]
    }
}

/// The files under a folder, with what the ignore files leave out left out.
#[derive(Debug, Default)]
pub struct FileIndex {
    root: PathBuf,
    entries: Vec<Entry>,
    truncated: bool,
}

impl FileIndex {
    /// How many files an index keeps at most, so that a folder as big as a disk does not fill the memory.
    pub const LIMIT: usize = 200_000;

    /// Walks `root` and lists its files, up to `limit` of them. Stops, with an index of nothing, when `cancel` is set.
    pub fn build(root: &Path, limit: usize, cancel: &AtomicBool) -> Self {
        let mut index = Self {
            root: root.to_path_buf(),
            entries: Vec::new(),
            truncated: false,
        };
        let walk = WalkBuilder::new(root)
            .require_git(false)
            .sort_by_file_name(|a, b| a.cmp(b))
            .build();
        for entry in walk {
            if cancel.load(Ordering::Relaxed) {
                index.entries.clear();
                return index;
            }
            let Ok(entry) = entry else { continue };
            if !entry
                .file_type()
                .is_some_and(|kind| kind.is_file())
            {
                continue;
            }
            let Ok(relative) = entry.path().strip_prefix(root) else {
                continue;
            };
            if index.entries.len() >= limit {
                index.truncated = true;
                break;
            }
            let relative = relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            let name_start = relative.rfind('/').map_or(0, |slash| slash + 1);
            index.entries.push(Entry {
                relative,
                name_start,
            });
        }
        index
    }

    /// How many files there are.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the folder had more files than the index keeps.
    #[cfg(test)]
    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    /// The files that fit `pattern`, the best first, at most `limit`. The letters are those of the name of the file, or
    /// of the whole path when the pattern has a `/`, which is how a folder is named. Nothing is found for no letters.
    pub fn find(&self, pattern: &Pattern, limit: usize) -> Vec<FileHit> {
        if pattern.is_empty() {
            return Vec::new();
        }
        let by_path = pattern.has('/');
        let mut fits: Vec<(i32, usize, Vec<Range<usize>>)> = Vec::new();
        for (at, entry) in self.entries.iter().enumerate() {
            let text = if by_path {
                entry.relative.as_str()
            } else {
                entry.name()
            };
            if let Some(found) = pattern.matches(text) {
                fits.push((found.score, at, found.ranges));
            }
        }
        // The better score first; then the shorter path, which is the nearer to the root; then the order of the folder.
        fits.sort_by(|(a, a_at, _), (b, b_at, _)| {
            b.cmp(a)
                .then_with(|| {
                    self.entries[*a_at]
                        .relative
                        .len()
                        .cmp(&self.entries[*b_at].relative.len())
                })
                .then_with(|| a_at.cmp(b_at))
        });
        fits.truncate(limit);
        fits.into_iter()
            .map(|(score, at, ranges)| {
                let entry = &self.entries[at];
                // A mark in the folder part of a path is not shown: the name is what is drawn marked.
                let shift = if by_path { entry.name_start } else { 0 };
                let marked = ranges
                    .into_iter()
                    .filter(|range| range.start >= shift)
                    .map(|range| range.start - shift..range.end - shift)
                    .collect();
                FileHit {
                    path: self.root.join(&entry.relative),
                    name: entry.name().to_owned(),
                    directory: entry.relative[..entry.name_start]
                        .trim_end_matches('/')
                        .to_owned(),
                    marked,
                    score,
                }
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "files_tests.rs"]
mod files_tests;
