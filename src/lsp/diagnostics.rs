//! What the server says is wrong with the text, as bytes of the text the server has: what the lines under the code are made from.

use std::path::Path;
use std::rc::Rc;

use aimer::input::input::TextSnapshot;

use super::client::Lsp;
use super::session::Diagnostic;

impl Lsp {
    /// The diagnostics of the open file `path` that are about the text the server has, and that text, which their bytes are of: what
    /// is on the screen is moved from that text to the one it is by the edits made since. None where the file is not open, or the
    /// server has been told of a text that it has not yet said anything of. Nothing published is empty and not none.
    pub fn diagnostics(&self, path: &Path) -> Option<(Vec<Diagnostic>, TextSnapshot)> {
        self.core_diagnostics(path)
    }

    /// Adds what is called when what the server says may have changed: its diagnostics, its status. It says whether what it is for is
    /// still there, and is forgotten when it is not.
    pub fn on_diagnostics(&self, hook: Rc<dyn Fn() -> bool>) {
        self.add_change_hook(hook);
    }
}

#[cfg(test)]
mod tests;
