//! The language server as the source of the items of one file.

use super::completer::Source;
use crate::lsp::{Answer, Item, Lsp};
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

/// What the server says could be put at the caret of the file `path`.
pub struct LspSource {
    lsp: Lsp,
    path: PathBuf,
}

impl LspSource {
    pub fn new(lsp: Lsp, path: PathBuf) -> Rc<dyn Source> {
        Rc::new(Self { lsp, path })
    }
}

impl Source for LspSource {
    fn request(&self, done: Box<dyn FnOnce(Option<Answer>)>) {
        self.lsp.complete(&self.path, done);
    }

    fn resolve(&self, item: &Item, done: Box<dyn FnOnce(Option<Vec<(Range<usize>, String)>>)>) {
        match &item.resolve {
            Some(payload) => self.lsp.resolve(&self.path, payload, done),
            None => done(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn a_server_that_is_not_there_says_so_at_once_and_does_not_keep_the_request() {
        let lsp = Lsp::with_starter(
            PathBuf::from("/project"),
            Box::new(|_| Err(std::io::Error::other("none"))),
            true,
        );
        let source = LspSource::new(lsp, PathBuf::from("/project/src/main.rs"));
        let said = Rc::new(Cell::new(None));
        let into = Rc::clone(&said);
        source.request(Box::new(move |answer| into.set(Some(answer.is_none()))));
        assert_eq!(said.get(), Some(true));
    }
}
