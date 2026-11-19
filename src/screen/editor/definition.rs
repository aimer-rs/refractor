//! Going to where a name is defined: a force click on a name in a Rust file asks the language server where it is, and goes
//! there.

use crate::lsp::{Lsp, Place};
use std::path::Path;

/// Asks the server where the name at the byte `offset` of the file `path` is defined, and calls `go` with the first place it
/// says. Nothing is called when it says none, or cannot be asked, or another request is made before it answers.
pub fn find_definition(lsp: &Lsp, path: &Path, offset: usize, go: impl FnOnce(Place) + 'static) {
    lsp.definition(path, offset, move |places| {
        if let Some(place) = places.and_then(|places| places.into_iter().next()) {
            go(place);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::path::PathBuf;
    use std::rc::Rc;

    #[test]
    fn nothing_is_gone_to_where_there_is_no_server_to_ask() {
        let lsp = Lsp::with_starter(PathBuf::from("/project"), Box::new(|_| Err(std::io::Error::other("none"))), true);
        let went = Rc::new(Cell::new(false));
        let marked = Rc::clone(&went);
        find_definition(&lsp, Path::new("/project/src/main.rs"), 3, move |_| marked.set(true));
        assert!(!went.get());
    }
}
