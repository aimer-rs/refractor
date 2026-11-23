//! A few lines of a file around what a hit goes to, shown while a key is held, to look before going.
//!
//! Only the lines that are shown are kept: the lines before them are read past and dropped, and a file that is large is not
//! read, so what the preview costs is bounded, and does not grow with the file.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use super::Target;

/// The lines of a file around the one that is gone to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preview {
    pub path: PathBuf,
    /// The number of the first of the lines, from 1.
    pub first: usize,
    pub lines: Vec<String>,
    /// The number of the line that is gone to, from 1, if it is among the lines.
    pub focus: Option<usize>,
}

impl Preview {
    /// How many lines are shown before the one that is gone to, and after it.
    pub const BEFORE: usize = 4;
    pub const AFTER: usize = 7;
    /// A file bigger than this is not read for a preview.
    pub const LARGEST_FILE: u64 = 4 * 1024 * 1024;
    /// How long a line is at most, in bytes: what is longer is cut.
    pub const LINE: usize = 300;

    /// The preview of what `target` goes to, or none for what is not in a file, a file that cannot be read, is empty or is too
    /// large. A line that is past the end of the file shows the end of it, with no line chosen.
    pub fn of(target: &Target) -> Option<Self> {
        let (path, line) = match target {
            Target::File { path, at } => (path, at.map(|(line, _)| line)),
            Target::Place(place) => (&place.path, Some(place.at.line as usize + 1)),
            Target::Action(_) => return None,
        };
        let (first, lines) = window(path, line)?;
        let focus = line.filter(|line| (first..first + lines.len()).contains(line));
        Some(Self {
            path: path.clone(),
            first,
            lines,
            focus,
        })
    }
}

/// Where the reading of a window of lines stops at the least: the lines that are before the one that is gone to, it, and the
/// lines after it.
const SIZE: usize = Preview::BEFORE + 1 + Preview::AFTER;

/// The line as text: without its break, and cut where it is long, on a character.
fn text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
    let mut line = String::from_utf8_lossy(bytes).into_owned();
    if line.len() > Preview::LINE {
        let mut end = Preview::LINE;
        while !line.is_char_boundary(end) {
            end -= 1;
        }
        line.truncate(end);
    }
    line
}

fn reader(path: &Path) -> Option<BufReader<File>> {
    let file = File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    (meta.is_file() && meta.len() <= Preview::LARGEST_FILE).then(|| BufReader::new(file))
}

/// The number of the first line and the lines that are shown for `focus`: the ones around it, or the first of the file when
/// there is none, or the last of the file when it is not there. None for an empty file.
fn window(path: &Path, focus: Option<usize>) -> Option<(usize, Vec<String>)> {
    let start = focus.map_or(1, |line| line.saturating_sub(Preview::BEFORE).max(1));
    let mut input = reader(path)?;
    let mut buffer = Vec::new();
    // The lines before the window are read and not kept.
    let mut number = 0;
    while number + 1 < start {
        buffer.clear();
        if input.read_until(b'\n', &mut buffer).ok()? == 0 {
            return tail(path);
        }
        number += 1;
    }
    let mut lines = Vec::with_capacity(SIZE);
    while lines.len() < SIZE {
        buffer.clear();
        if input.read_until(b'\n', &mut buffer).ok()? == 0 {
            break;
        }
        lines.push(text(&buffer));
    }
    if lines.is_empty() {
        return tail(path);
    }
    Some((start, lines))
}

/// The last lines of the file, for a line that is past its end.
fn tail(path: &Path) -> Option<(usize, Vec<String>)> {
    let mut input = reader(path)?;
    let mut buffer = Vec::new();
    let mut last: VecDeque<String> = VecDeque::with_capacity(SIZE);
    let mut count = 0;
    loop {
        buffer.clear();
        if input.read_until(b'\n', &mut buffer).ok()? == 0 {
            break;
        }
        count += 1;
        if last.len() == SIZE {
            last.pop_front();
        }
        last.push_back(text(&buffer));
    }
    (!last.is_empty()).then(|| (count - last.len() + 1, last.into()))
}

#[cfg(test)]
#[path = "../../tests/services/search/preview.rs"]
mod preview_tests;
