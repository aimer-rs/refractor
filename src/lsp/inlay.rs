//! The notes a server has for the lines of a file, such as the type of a name, as bytes of the text.
//!
//! They are asked for the lines that are shown, and not for the file, so that the cost of them is that of the screen.

use std::ops::Range;
use std::path::Path;

use aimer::input::input::TextSnapshot;

use super::client::Lsp;
use super::protocol::{self, HintKind};

/// One note, as the editor draws it: where it goes, what it says and what of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hint {
    /// The byte it is drawn before, in the text it was asked for.
    pub at: usize,
    pub label: String,
    pub kind: HintKind,
    /// Whether it is drawn a little apart from the text before it.
    pub pad_left: bool,
    /// Whether it is drawn a little apart from the text after it.
    pub pad_right: bool,
}

impl Lsp {
    /// Asks the server for the hints of the lines `lines` of the open file `path`, and calls `done` with them, in the order of the
    /// text, when it has said, or with none when it could not be asked, has no hints to give, or could not say. `done` is not
    /// called from inside this unless it is with none, and not at all if the request is [cancelled](Self::cancel). The number
    /// that is returned is the request's.
    pub fn inlay_hints(
        &self,
        path: &Path,
        lines: Range<usize>,
        done: impl FnOnce(Option<Vec<Hint>>) + 'static,
    ) -> Option<i64> {
        let text = self.snapshot(path).filter(|_| self.can_hint());
        let Some(text) = text else {
            done(None);
            return None;
        };
        let lsp = self.clone();
        // The lines are those of the text. The start of the line after the last one is the end of the text, and a range that goes
        // past it is not one every server takes.
        let start = lsp.position_of(&text, line_start(&text, lines.start));
        let end = lsp.position_of(&text, line_start(&text, lines.end));
        let params = protocol::hint_params(path, start, end);
        self.ask(protocol::INLAY_HINT, params, move |reply| {
            done(reply.map(|reply| {
                let mut hints: Vec<Hint> = protocol::parse_hints(&reply)
                    .into_iter()
                    .map(|reported| Hint {
                        at: lsp.byte_of(&text, reported.at),
                        label: reported.label,
                        kind: reported.kind,
                        pad_left: reported.pad_left,
                        pad_right: reported.pad_right,
                    })
                    .collect();
                // Stable: what a server says at one place stays in the order that it said it.
                hints.sort_by_key(|hint| hint.at);
                hints
            }));
        })
    }
}

/// The byte where the line `line` of `text` starts, or the end of the text for a line that it has not.
fn line_start(text: &TextSnapshot, line: usize) -> usize {
    if line >= text.line_count() {
        return text.len();
    }
    text.line_to_byte(line)
}

#[cfg(test)]
mod tests;
