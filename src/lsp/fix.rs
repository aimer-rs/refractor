//! What the server offers to do about a problem: the edits that fix it, as bytes of the text, and the doing of them.

use std::ops::Range;
use std::path::Path;

use aimer::input::TextEditingController;
use aimer::input::input::TextSnapshot;

use super::client::Lsp;
use super::position::Position;
use super::protocol;

/// A fix: what it is called and the edits that it makes.
#[derive(Clone)]
pub struct CodeFix {
    pub title: String,
    /// Whether the server says it is the one to do.
    pub preferred: bool,
    /// The bytes to replace and what to replace them with, in the text that the fix was asked for.
    pub edits: Vec<(Range<usize>, String)>,
    /// The text the places are of: a fix is done only to that text.
    pub text: TextSnapshot,
}

impl CodeFix {
    /// Makes the edits in the text of `controller`, as one edit that one undo takes back, and keeps the caret and the selection with the
    /// text they were in: what is put in above it moves it, and what is replaced under it puts it at the start of what replaced it. False
    /// where nothing was done: the text is not the one the fix was made for, since the user typed after it was asked for, or the edits
    /// overlap.
    pub fn apply(&self, controller: &TextEditingController) -> bool {
        if controller.snapshot().diff(&self.text).is_some() {
            return false;
        }
        let selection = controller.value().selection().range();
        let edits: Vec<(Range<usize>, &str)> = self.edits.iter().map(|(range, text)| (range.clone(), text.as_str())).collect();
        if !controller.replace_ranges(&edits, 0) {
            return false;
        }
        let (anchor, focus) = (self.moved(selection.start), self.moved(selection.end));
        controller.select_bytes(anchor, focus);
        true
    }

    /// Where the byte `at` of the text the fix is of is in the text it makes: moved by each edit that ends at or before it, and at the
    /// start of what an edit replaced if it is in the middle of it.
    fn moved(&self, at: usize) -> usize {
        let point = self.edits.iter().find(|(range, _)| range.start < at && at < range.end).map_or(at, |(range, _)| range.start);
        let shift: isize = self
            .edits
            .iter()
            .filter(|(range, _)| range.end <= point)
            .map(|(range, text)| text.len() as isize - (range.end - range.start) as isize)
            .sum();
        point.checked_add_signed(shift).expect("a caret is not before the start of the text")
    }
}

impl Lsp {
    /// Asks the server what it offers to do about the range `range` of the open file `path`, and calls `done` with the fixes that change
    /// only this file, in the order the server said them, when it has said: with none where it could not be asked or could not say. `done`
    /// is not called from inside this unless it is with none, and not at all if the request is [cancelled](Self::cancel). The number that
    /// is returned is the request's.
    pub fn code_actions(&self, path: &Path, range: Range<usize>, done: impl FnOnce(Option<Vec<CodeFix>>) + 'static) -> Option<i64> {
        let Some(text) = self.snapshot(path) else {
            done(None);
            return None;
        };
        let place = |byte: usize| -> Position { self.position_of(&text, byte) };
        let params = protocol::action_params(path, (place(range.start), place(range.end)));
        let (lsp, path) = (self.clone(), path.to_path_buf());
        self.ask(protocol::CODE_ACTION, params, move |reply| {
            done(reply.map(|reply| {
                protocol::parse_actions(&reply, &path)
                    .into_iter()
                    .map(|action| CodeFix {
                        title: action.title,
                        preferred: action.preferred,
                        edits: action
                            .edits
                            .into_iter()
                            .map(|((from, to), new_text)| {
                                let start = lsp.byte_of(&text, from);
                                (start..lsp.byte_of(&text, to).max(start), new_text)
                            })
                            .collect(),
                        text: text.clone(),
                    })
                    .collect()
            }));
        })
    }
}

#[cfg(test)]
mod tests;
