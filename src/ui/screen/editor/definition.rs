//! Going to where a name is defined: a force click on a name in a Rust file asks the language server where it is, and goes
//! there.

use crate::services::lsp::{Lsp, Place};
use crate::ui::usages::Usages;
use aimer::input::input::{FieldSpot, TextSnapshot};
use std::path::Path;
use std::rc::Rc;

/// Asks the server where the name at the byte `offset` of the file `path` is defined, and calls `go` with the first place it
/// says. Nothing is called when it says none, or cannot be asked, or another request is made before it answers.
pub fn find_definition(lsp: &Lsp, path: &Path, offset: usize, go: impl FnOnce(Place) + 'static) {
    lsp.definition(path, offset, move |places| {
        if let Some(place) = places.and_then(|places| places.into_iter().next()) {
            go(place);
        }
    });
}

/// Goes to where the name at the byte `offset` of `path` is defined, or lists who uses it when the click is on the definition
/// itself.
pub fn go_to_definition_or_usages(
    lsp: &Lsp,
    usages: &Rc<Usages>,
    text: TextSnapshot,
    path: &Path,
    offset: usize,
    spot: FieldSpot,
    go: impl FnOnce(Place) + 'static,
) {
    let asked = lsp.clone();
    let usages = Rc::clone(usages);
    let own = path.to_path_buf();
    find_definition(lsp, path, offset, move |place| {
        if let Some(start) = is_the_name_itself(&asked, &text, &own, offset, &place) {
            usages.find_usages_at(start, spot);
            return;
        }
        go(place);
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
    use crate::services::lsp::Position;
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
            end: Position {
                line,
                character: character + 5,
            },
        };
        // The definition is at byte 11, and the click is anywhere on the name: the first character, the middle, and just after it.
        for offset in [11, 13, 16] {
            assert_eq!(
                is_the_name_itself(
                    &lsp,
                    &text,
                    file,
                    offset,
                    &place("/project/src/main.rs", 0, 11)
                ),
                Some(11),
                "offset {offset}"
            );
        }
        // A click on a use of the name, which goes to the definition above it.
        let used = text
            .slice(0..text.len())
            .find("Rules }")
            .unwrap();
        assert_eq!(
            is_the_name_itself(
                &lsp,
                &text,
                file,
                used,
                &place("/project/src/main.rs", 0, 11)
            ),
            None
        );
        // The same name in another file is another place.
        assert_eq!(
            is_the_name_itself(
                &lsp,
                &text,
                file,
                12,
                &place("/project/src/other.rs", 0, 11)
            ),
            None
        );
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

    #[test]
    fn clicking_a_variable_or_associated_const_definition_lists_its_usages_instead_of_jumping() {
        use crate::services::lsp::fake::{Fake, installed, lsp_with, settle};
        use crate::services::settings::ThemeMode;
        use crate::ui::usages::LinkColors;
        use aimer::Color;
        use aimer::input::TextEditingController;
        use serde_json::json;

        const FILE: &str = "/project/src/main.rs";
        const SOURCE: &str = "struct S;\nimpl S {\n    pub const LIMIT: usize = 1;\n    fn f() -> usize {\n        let count = Self::LIMIT;\n        count + Self::LIMIT\n    }\n}\n";

        let venus = installed();
        let fake = Fake::new();
        let lsp = lsp_with(&fake);
        let controller = TextEditingController::with_text(SOURCE);
        lsp.open(Path::new(FILE), &controller, Rc::new(|| {}));
        settle(&venus);
        let _ = fake.requests();
        fake.answer(
            1,
            json!({"capabilities": {"textDocumentSync": 2, "positionEncoding": "utf-8", "definitionProvider": true, "referencesProvider": true}}),
        );
        settle(&venus);
        let _ = fake.requests();

        let usages = Rc::new(Usages::default());
        usages.attach(
            lsp.clone(),
            PathBuf::from(FILE),
            PathBuf::from("/project"),
            controller.clone(),
            LinkColors {
                rest: Color::Rgba(120, 124, 135, 255),
                hover: Color::Rgba(250, 250, 250, 255),
            },
            ThemeMode::Dark,
            Rc::new(|_| None),
        );
        let spot = FieldSpot {
            x: 10.0,
            y: 20.0,
            width: 40.0,
            height: 18.0,
            extent: Some((400.0, 200.0)),
        };

        // 1) Associated const `LIMIT` on line 2, columns 14..19.
        let const_offset = SOURCE.find("LIMIT: usize").unwrap();
        let went = Rc::new(Cell::new(false));
        let marked = Rc::clone(&went);
        go_to_definition_or_usages(
            &lsp,
            &usages,
            controller.snapshot(),
            Path::new(FILE),
            const_offset + 2,
            spot.clone(),
            move |_| marked.set(true),
        );
        settle(&venus);
        let asked = fake.requests();
        assert_eq!(asked[0].0, "textDocument/definition");
        fake.answer(
            asked[0].2.expect("request id"),
            json!([{"uri": "file:///project/src/main.rs", "range": {"start": {"line": 2, "character": 14}, "end": {"line": 2, "character": 19}}}]),
        );
        settle(&venus);
        assert!(!went.get(), "a definition has nowhere to jump to");
        let refs = fake.requests();
        assert_eq!(
            refs.first().map(|(m, _, _)| m.as_str()),
            Some("textDocument/references"),
            "an associated const has no lens, so references are asked for"
        );
        fake.answer(
            refs[0].2.expect("request id"),
            json!([
                {"uri": "file:///project/src/main.rs", "range": {"start": {"line": 4, "character": 26}, "end": {"line": 4, "character": 31}}},
                {"uri": "file:///project/src/main.rs", "range": {"start": {"line": 5, "character": 22}, "end": {"line": 5, "character": 27}}}
            ]),
        );
        settle(&venus);
        let list = usages
            .list()
            .expect("usages list opened for associated const");
        assert_eq!(list.heading.name, "LIMIT");
        assert_eq!(list.heading.count_text(), "2 usages");
        usages.close();

        // 2) Local variable `count` on line 4, columns 12..17.
        let var_offset = SOURCE.find("count =").unwrap();
        let went = Rc::new(Cell::new(false));
        let marked = Rc::clone(&went);
        go_to_definition_or_usages(
            &lsp,
            &usages,
            controller.snapshot(),
            Path::new(FILE),
            var_offset + 1,
            spot,
            move |_| marked.set(true),
        );
        settle(&venus);
        let asked = fake.requests();
        assert_eq!(asked[0].0, "textDocument/definition");
        fake.answer(
            asked[0].2.expect("request id"),
            json!([{"uri": "file:///project/src/main.rs", "range": {"start": {"line": 4, "character": 12}, "end": {"line": 4, "character": 17}}}]),
        );
        settle(&venus);
        assert!(!went.get(), "a variable definition has nowhere to jump to");
        let refs = fake.requests();
        assert_eq!(
            refs.first().map(|(m, _, _)| m.as_str()),
            Some("textDocument/references"),
            "a variable has no lens, so references are asked for"
        );
        fake.answer(
            refs[0].2.expect("request id"),
            json!([
                {"uri": "file:///project/src/main.rs", "range": {"start": {"line": 5, "character": 8}, "end": {"line": 5, "character": 13}}}
            ]),
        );
        settle(&venus);
        let list = usages
            .list()
            .expect("usages list opened for local variable");
        assert_eq!(list.heading.name, "count");
        assert_eq!(list.heading.count_text(), "1 usage");
    }

    #[test]
    fn clicking_a_use_of_a_name_goes_to_its_definition() {
        use crate::services::lsp::fake::{Fake, installed, lsp_with, settle};
        use aimer::input::TextEditingController;
        use serde_json::json;

        const FILE: &str = "/project/src/main.rs";
        const SOURCE: &str = "fn f() -> usize {\n    let count = 1;\n    count\n}\n";

        let venus = installed();
        let fake = Fake::new();
        let lsp = lsp_with(&fake);
        let controller = TextEditingController::with_text(SOURCE);
        lsp.open(Path::new(FILE), &controller, Rc::new(|| {}));
        settle(&venus);
        let _ = fake.requests();
        fake.answer(
            1,
            json!({"capabilities": {"textDocumentSync": 2, "positionEncoding": "utf-8", "definitionProvider": true}}),
        );
        settle(&venus);
        let _ = fake.requests();

        let usages = Rc::new(Usages::default());
        let use_offset = SOURCE.rfind("count").unwrap();
        let went = Rc::new(Cell::new(false));
        let marked = Rc::clone(&went);
        go_to_definition_or_usages(
            &lsp,
            &usages,
            controller.snapshot(),
            Path::new(FILE),
            use_offset,
            FieldSpot {
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 18.0,
                extent: Some((400.0, 200.0)),
            },
            move |place| {
                assert_eq!(
                    place.at,
                    Position {
                        line: 1,
                        character: 8
                    }
                );
                marked.set(true);
            },
        );
        settle(&venus);
        let asked = fake.requests();
        assert_eq!(asked[0].0, "textDocument/definition");
        fake.answer(
            asked[0].2.expect("request id"),
            json!([{"uri": "file:///project/src/main.rs", "range": {"start": {"line": 1, "character": 8}, "end": {"line": 1, "character": 13}}}]),
        );
        settle(&venus);
        assert!(went.get(), "jumped to the variable definition");
        assert!(fake.requests().is_empty(), "did not ask for references");
    }
}
