//! Formatting a file: asking the language server for the edits that format it, or running an external formatter (`rustfmt`)
//! on its text, and putting the edits in as one undo step while keeping the caret with its text.

use std::io::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use aimer::input::TextEditingController;
use aimer::input::input::TextSnapshot;

use crate::model::document::Language;

use super::client::Lsp;
use super::locate;
use super::protocol;

/// The external formatter run for Rust when `REFRACTOR_FORMATTER` does not name another.
const DEFAULT_RUST_FORMATTER: &str = "rustfmt";

/// The edits that format a text, as bytes of the text they were made for.
#[derive(Clone, Debug)]
pub struct FormatEdit {
    /// The bytes to replace and what to replace them with, in the text that the formatting was asked for.
    pub edits: Vec<(Range<usize>, String)>,
    /// The text the places are of: the edits are made only to that text.
    pub text: TextSnapshot,
}

impl FormatEdit {
    /// Makes a [`FormatEdit`] from the whole formatted text that an external formatter produced for `text`.
    /// Only the span that differs between `text` and `formatted` is kept as an edit; when the two are the same,
    /// there are no edits to make.
    pub fn from_formatted(text: TextSnapshot, formatted: &str) -> Self {
        let target = TextSnapshot::from_text(formatted);
        let edits = match text.diff(&target) {
            Some((old, new)) => vec![(old, formatted[new].to_owned())],
            None => Vec::new(),
        };
        Self { edits, text }
    }

    /// Makes the edits in the text of `controller`, as one edit that one undo takes back, and keeps the caret and the selection
    /// with the text they were in. When the file is already formatted, nothing is changed and no undo step is added. False where
    /// the text changed since the formatting was asked for, or where the edits overlap.
    pub fn apply(&self, controller: &TextEditingController) -> bool {
        if controller.snapshot().diff(&self.text).is_some() {
            return false;
        }
        let len = self.text.len();
        let mut sorted: Vec<(Range<usize>, &str)> = self
            .edits
            .iter()
            .map(|(range, text)| {
                let start = range.start.min(len);
                let end = range.end.min(len).max(start);
                (start..end, text.as_str())
            })
            .collect();
        sorted.sort_by_key(|(range, _)| (range.start, range.end));
        if sorted
            .windows(2)
            .any(|pair| pair[0].0.end > pair[1].0.start)
        {
            return false;
        }
        let effective: Vec<(Range<usize>, &str)> = sorted
            .into_iter()
            .filter(|(range, new_text)| self.text.slice(range.clone()).as_ref() != *new_text)
            .collect();
        if effective.is_empty() {
            return true;
        }
        let selection = controller.value().selection().range();
        if !controller.replace_ranges(&effective, 0) {
            return false;
        }
        let (anchor, focus) = (
            moved(&effective, selection.start),
            moved(&effective, selection.end),
        );
        controller.select_bytes(anchor, focus);
        true
    }
}

/// Where the byte `at` of the text before `edits` is after they are made: shifted by every edit that ends at or before it,
/// and clamped to a character boundary within an edit that spans across it.
fn moved(edits: &[(Range<usize>, &str)], at: usize) -> usize {
    let point = edits
        .iter()
        .find(|(range, _)| range.start < at && at < range.end)
        .map_or(at, |(range, text)| {
            range.start + floor_char_boundary(text, (at - range.start).min(text.len()))
        });
    let shift: isize = edits
        .iter()
        .filter(|(range, _)| range.end <= at)
        .map(|(range, text)| text.len() as isize - (range.end - range.start) as isize)
        .sum();
    point
        .checked_add_signed(shift)
        .expect("a caret is not before the start of the text")
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

impl Lsp {
    /// Asks the server for the edits that format the open file `path` with `tab_spaces` spaces per indent, and calls `done` with
    /// them when the server has said, or with `None` where it could not be asked or could not format the file.
    pub fn format(
        &self,
        path: &Path,
        tab_spaces: usize,
        done: impl FnOnce(Option<FormatEdit>) + 'static,
    ) -> Option<i64> {
        let Some(text) = self.snapshot(path) else {
            done(None);
            return None;
        };
        let params = protocol::formatting_params(path, tab_spaces);
        let lsp = self.clone();
        self.ask(protocol::FORMATTING, params, move |reply| {
            done(reply.and_then(|reply| {
                let edits = protocol::formatting_edits(&reply)?
                    .into_iter()
                    .map(|((from, to), new_text)| {
                        let start = lsp.byte_of(&text, from);
                        (start..lsp.byte_of(&text, to).max(start), new_text)
                    })
                    .collect();
                Some(FormatEdit { edits, text })
            }));
        })
    }
}

/// Runs the external formatter for `language` on `text` (for a file at `path`, indented by `tab_spaces`), using the program
/// `REFRACTOR_FORMATTER` names or `rustfmt`.
pub fn format_external(
    language: Language,
    path: &Path,
    text: TextSnapshot,
    tab_spaces: usize,
) -> Result<FormatEdit, String> {
    format_external_with(language, path, text, tab_spaces, None)
}

/// Runs the external formatter with an optional explicit `program` override.
pub fn format_external_with(
    language: Language,
    path: &Path,
    text: TextSnapshot,
    tab_spaces: usize,
    program: Option<&Path>,
) -> Result<FormatEdit, String> {
    if language != Language::Rust {
        return Err(format!(
            "No formatter is available for {}",
            language.label()
        ));
    }
    let resolved = match program {
        Some(program) => program.to_path_buf(),
        None => {
            let named = std::env::var("REFRACTOR_FORMATTER")
                .ok()
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| DEFAULT_RUST_FORMATTER.to_owned());
            let home = std::env::var_os("HOME").map(PathBuf::from);
            locate::resolve(&named, std::env::var_os("PATH"), home.as_deref())
        }
    };
    let mut command = Command::new(&resolved);
    command
        .arg("--edition")
        .arg("2024")
        .arg("--config")
        .arg(format!("tab_spaces={tab_spaces}"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(folder) = path.parent().filter(|folder| folder.is_dir()) {
        command.current_dir(folder);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not run {}: {error}", resolved.display()))?;
    if let Some(mut stdin) = child.stdin.take() {
        for chunk in text.chunks(0..text.len()) {
            stdin
                .write_all(chunk.as_bytes())
                .map_err(|error| {
                    format!("Could not send text to {}: {error}", resolved.display())
                })?;
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("Could not wait for {}: {error}", resolved.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("formatter exited with an error");
        return Err(detail.trim().to_owned());
    }
    let formatted = String::from_utf8(output.stdout)
        .map_err(|_| "The formatter output was not valid UTF-8".to_owned())?;
    Ok(FormatEdit::from_formatted(text, &formatted))
}

#[cfg(test)]
#[path = "../../tests/services/lsp/format.rs"]
mod tests;
