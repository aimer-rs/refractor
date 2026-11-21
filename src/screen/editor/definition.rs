//! Going to where a name is defined: a force click on a name in a Rust file asks the language server where it is, and goes
//! there.

use crate::lsp::{Lsp, Place};
use aimer::input::input::TextSnapshot;
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

/// Whether `place`, which the server gave as where the name at the byte `offset` of the file `path` is defined, is that very name:
/// the click was on a definition, which has nowhere to go but where it is. It is the byte where the name starts if so.
pub fn is_the_name_itself(
    lsp: &Lsp,
    text: &TextSnapshot,
    path: &Path,
    offset: usize,
    place: &Place,
) -> Option<usize> {
    if place.path != path {
        return None;
    }
    let (start, end) = (lsp.byte_of(text, place.at), lsp.byte_of(text, place.end));
    (start..=end).contains(&offset).then_some(start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::Position;
    use std::cell::Cell;
    use std::path::PathBuf;
    use std::rc::Rc;

    fn silent() -> Lsp {
        Lsp::with_starter(
            PathBuf::from("/project"),
            Box::new(|_| Err(std::io::Error::other("none"))),
            true,
        )
    }

    #[test]
    fn a_place_is_the_name_itself_when_it_is_in_the_same_file_and_the_click_was_on_it() {
        let lsp = silent();
        let file = Path::new("/project/src/main.rs");
        let text = TextSnapshot::from_text("pub struct Rules;\nfn f() { Rules }\n");
        let place = |path: &str, line: u32, character: u32| Place {
            path: PathBuf::from(path),
            at: Position { line, character },
            end: Position { line, character: character + 5 },
        };
        // The definition is at byte 11, and the click is anywhere on the name: the first character, the middle, and just after it.
        for offset in [11, 13, 16] {
            assert_eq!(
                is_the_name_itself(&lsp, &text, file, offset, &place("/project/src/main.rs", 0, 11)),
                Some(11),
                "offset {offset}"
            );
        }
        // A click on a use of the name, which goes to the definition above it.
        let used = text.slice(0..text.len()).find("Rules }").unwrap();
        assert_eq!(is_the_name_itself(&lsp, &text, file, used, &place("/project/src/main.rs", 0, 11)), None);
        // The same name in another file is another place.
        assert_eq!(is_the_name_itself(&lsp, &text, file, 12, &place("/project/src/other.rs", 0, 11)), None);
    }

    #[test]
    fn nothing_is_gone_to_where_there_is_no_server_to_ask() {
        let lsp = Lsp::with_starter(
            PathBuf::from("/project"),
            Box::new(|_| Err(std::io::Error::other("none"))),
            true,
        );
        let went = Rc::new(Cell::new(false));
        let marked = Rc::clone(&went);
        find_definition(&lsp, Path::new("/project/src/main.rs"), 3, move |_| {
            marked.set(true)
        });
        assert!(!went.get());
    }
}
