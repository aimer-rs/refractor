//! The text in the files of the folder, found with the engine of ripgrep.
//!
//! The folder is walked as `rg` walks it (what `.gitignore` and hidden files leave out is left out) and each file is read
//! line by line by the searcher, which gives up on a binary file at its first NUL byte and never holds a whole file. The
//! work is bounded: it stops at the limit of hits, and the caller can cancel it, which is checked at each line that
//! matches and at each file.

use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use grep_matcher::Matcher;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkMatch};
use ignore::WalkBuilder;

/// One line of a file that has the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextHit {
    pub path: PathBuf,
    /// The number of the line, from 1.
    pub line: usize,
    /// How many bytes into the line the text is.
    pub column: usize,
    /// The line, without its blanks at the start and its line break, and cut short around the text when it is very long.
    pub preview: String,
    /// Where in the preview the text is: on character boundaries.
    pub marked: Range<usize>,
}

/// What a search found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    pub hits: Vec<TextHit>,
    /// Whether the search stopped at its limit, with more to find.
    pub truncated: bool,
}

/// A search for some text: literal, and with the case ignored unless the text has a capital letter.
pub struct TextSearch {
    /// None for a query that is blank, or that the engine could not make a matcher of.
    matcher: Option<RegexMatcher>,
}

impl TextSearch {
    /// How long a preview is at most, in bytes: what is longer is cut around the text.
    pub const PREVIEW: usize = 160;
    /// How much of the preview is kept before the text, when a line is cut.
    const BEFORE: usize = 40;
    /// The files that are bigger than this are not read: they are not code a person goes to.
    const LARGEST_FILE: u64 = 2 * 1024 * 1024;

    pub fn new(query: &str) -> Self {
        let matcher = (!query.trim().is_empty())
            .then(|| {
                RegexMatcherBuilder::new()
                    .case_smart(true)
                    .fixed_strings(true)
                    .build(query)
                    .ok()
            })
            .flatten();
        Self { matcher }
    }

    /// Searches the files under `root` for the text, and gives at most `limit` lines, in the order of the folder's files. It
    /// stops, with what it has, when `cancel` is set.
    pub fn run(&self, root: &Path, limit: usize, cancel: &AtomicBool) -> Found {
        let mut found = Found::default();
        let Some(matcher) = &self.matcher else {
            return found;
        };
        let mut searcher = SearcherBuilder::new()
            .binary_detection(BinaryDetection::quit(b'\x00'))
            .line_number(true)
            .build();
        let walk = WalkBuilder::new(root)
            // The ignore files count in a folder that is not a repository too: the folder is what is asked of.
            .require_git(false)
            .max_filesize(Some(Self::LARGEST_FILE))
            .sort_by_file_name(|a, b| a.cmp(b))
            .build();
        for entry in walk {
            if cancel.load(Ordering::Relaxed) || found.truncated {
                break;
            }
            // What cannot be read is passed over: a search is not stopped by a folder it has no right to.
            let Ok(entry) = entry else { continue };
            if !entry
                .file_type()
                .is_some_and(|kind| kind.is_file())
            {
                continue;
            }
            let mut sink = Collect {
                matcher,
                path: entry.path(),
                found: &mut found,
                limit,
                cancel,
            };
            let _ = searcher.search_path(matcher, entry.path(), &mut sink);
        }
        found
    }
}

/// Takes the lines that match from the searcher.
struct Collect<'a> {
    matcher: &'a RegexMatcher,
    path: &'a Path,
    found: &'a mut Found,
    limit: usize,
    cancel: &'a AtomicBool,
}

impl Sink for Collect<'_> {
    type Error = io::Error;

    fn matched(
        &mut self,
        _searcher: &Searcher,
        matched: &SinkMatch<'_>,
    ) -> Result<bool, io::Error> {
        if self.cancel.load(Ordering::Relaxed) {
            return Ok(false);
        }
        // Text that is not UTF-8 is not what the editor opens, and its offsets would not be those of a string.
        let Ok(line) = std::str::from_utf8(matched.bytes()) else {
            return Ok(true);
        };
        let Ok(Some(at)) = self.matcher.find(matched.bytes()) else {
            return Ok(true);
        };
        if self.found.hits.len() >= self.limit {
            self.found.truncated = true;
            return Ok(false);
        }
        let line = line.trim_end_matches(['\n', '\r']);
        let (preview, marked) = excerpt(line, at.start()..at.end().min(line.len()));
        self.found.hits.push(TextHit {
            path: self.path.to_path_buf(),
            line: matched
                .line_number()
                .map_or(0, |number| number as usize),
            column: at.start(),
            preview,
            marked,
        });
        Ok(true)
    }
}

/// The part of `line` to show for the match at `at`: without the blanks that start it, and when it is longer than a preview
/// is, the part around the match. And where the match is in it.
fn excerpt(line: &str, at: Range<usize>) -> (String, Range<usize>) {
    let blanks = line.len() - line.trim_start().len();
    let mut start = blanks.min(at.start);
    let mut end = line.len();
    if end - start > TextSearch::PREVIEW {
        start = start.max(at.start.saturating_sub(TextSearch::BEFORE));
        end = (start + TextSearch::PREVIEW)
            .max(at.end)
            .min(line.len());
        start = floor_boundary(line, start);
        end = ceil_boundary(line, end);
    }
    (
        line[start..end].to_owned(),
        at.start - start..at.end - start,
    )
}

fn floor_boundary(text: &str, mut at: usize) -> usize {
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

fn ceil_boundary(text: &str, mut at: usize) -> usize {
    while !text.is_char_boundary(at) {
        at += 1;
    }
    at
}

#[cfg(test)]
#[path = "../../tests/services/search/text.rs"]
mod text_tests;
