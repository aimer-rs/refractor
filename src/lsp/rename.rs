//! Renaming a name everywhere it is used, and finding where it is used: the two questions of a menu over a name.

use std::path::{Path, PathBuf};

use aimer::input::input::TextSnapshot;

use super::client::Lsp;
use super::protocol::{self, Place, WorkspaceEdit};

/// What a rename comes to: the edits of every file that has the name, and the text of the file it was asked in as it was when it was asked.
#[derive(Clone, Debug)]
pub struct Rename {
    pub edit: WorkspaceEdit,
    /// The file the rename was asked in and its text then: the edits are of that text, and are not made to another.
    pub of: (PathBuf, TextSnapshot),
}

impl Lsp {
    /// Asks the server for the edits that rename the name at the byte `at` of the open file `path` to `new_name`, and calls `done` with them, or
    /// with why there are none: the server could not be asked, the name cannot be renamed there, or the answer is more than edits of text.
    /// `done` is not called from inside this unless it is with the reason that the server could not be asked.
    pub fn rename(
        &self,
        path: &Path,
        at: usize,
        new_name: &str,
        done: impl FnOnce(Result<Rename, String>) + 'static,
    ) {
        let Some(text) = self.snapshot(path) else {
            done(Err(
                "The language server is not running for this file".to_owned()
            ));
            return;
        };
        let params = protocol::rename_params(path, self.position_of(&text, at), new_name);
        let of = (path.to_path_buf(), text);
        let asked = self.ask(protocol::RENAME, params, move |reply| {
            done(match reply {
                Some(reply) => protocol::workspace_edit(&reply).map(|edit| Rename { edit, of }),
                None => Err("The language server could not rename this".to_owned()),
            });
        });
        let _ = asked;
    }

    /// Asks the server where the name at the byte `at` of the open file `path` is used, not counting where it is defined, and calls `done`
    /// with the places, none where there are none, or none where it could not be asked or could not say. `done` is not called from inside
    /// this unless it is with none.
    pub fn references(
        &self,
        path: &Path,
        at: usize,
        done: impl FnOnce(Option<Vec<Place>>) + 'static,
    ) {
        let Some(text) = self.snapshot(path) else {
            done(None);
            return;
        };
        let params = protocol::references_params(path, self.position_of(&text, at));
        self.ask(protocol::REFERENCES, params, move |reply| {
            done(reply.and_then(|reply| protocol::definition_result(reply).ok()));
        });
    }
}

#[cfg(test)]
mod tests;
