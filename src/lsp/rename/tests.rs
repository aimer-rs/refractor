use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use aimer::input::TextEditingController;
use serde_json::{Value, json};

use super::*;
use crate::lsp::fake::{Fake, installed, lsp_with, settle};

const FILE: &str = "/project/src/main.rs";
const TEXT: &str = "struct State;\nfn main() { let s = State; }\n";

fn ready() -> (Rc<aimer::Venus>, Fake, Lsp, TextEditingController) {
    let venus = installed();
    let fake = Fake::new();
    let lsp = lsp_with(&fake);
    let controller = TextEditingController::with_text(TEXT);
    lsp.open(Path::new(FILE), &controller, Rc::new(|| {}));
    settle(&venus);
    let _ = fake.requests();
    fake.answer(
        1,
        json!({"capabilities": {"textDocumentSync": 2, "positionEncoding": "utf-8"}}),
    );
    settle(&venus);
    let _ = fake.requests();
    (venus, fake, lsp, controller)
}

type Renamed = Rc<RefCell<Vec<Result<Rename, String>>>>;

fn rename(lsp: &Lsp, at: usize, name: &str) -> Renamed {
    let given: Renamed = Rc::default();
    let into = Rc::clone(&given);
    lsp.rename(Path::new(FILE), at, name, move |result| {
        into.borrow_mut().push(result)
    });
    given
}

fn edit(line: u32, from: u32, to: u32, text: &str) -> Value {
    json!({"range": {"start": {"line": line, "character": from}, "end": {"line": line, "character": to}}, "newText": text})
}

#[test]
fn a_rename_asks_the_server_for_the_name_at_the_place_in_its_own_terms_and_says_the_edits_when_they_come()
 {
    let (venus, fake, lsp, _controller) = ready();
    // The name starts at byte 7, on the first line.
    let given = rename(&lsp, 7, "Fresh");
    let told = fake.requests();
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].0, "textDocument/rename");
    assert_eq!(told[0].1["position"], json!({"line": 0, "character": 7}));
    assert_eq!(told[0].1["newName"], "Fresh");
    assert!(given.borrow().is_empty(), "not said from inside the ask");

    fake.answer(
        told[0].2.expect("a request"),
        json!({"changes": {"file:///project/src/main.rs": [edit(0, 7, 12, "Fresh"), edit(1, 24, 29, "Fresh")]}}),
    );
    settle(&venus);

    let given = given.borrow();
    assert_eq!(given.len(), 1);
    let renamed = given[0].as_ref().expect("the edits");
    assert_eq!(renamed.edit.places(), 2);
    assert_eq!(renamed.of.0, Path::new(FILE), "the file it was asked in");
    assert_eq!(
        renamed.of.1.slice(0..13),
        "struct State;",
        "and the text the answer is of, which the user may type over while the server thinks"
    );
}

#[test]
fn a_server_that_says_nothing_or_an_error_gives_a_reason_and_not_an_edit() {
    let (venus, fake, lsp, _controller) = ready();
    let nothing = rename(&lsp, 7, "Fresh");
    let id = fake.requests()[0].2.expect("a request");
    fake.answer(id, Value::Null);
    settle(&venus);
    assert!(
        nothing.borrow()[0]
            .as_ref()
            .unwrap_err()
            .contains("nothing to rename")
    );

    let failed = rename(&lsp, 7, "Fresh");
    let id = fake.requests()[0].2.expect("a request");
    fake.fail(id);
    settle(&venus);
    assert!(
        failed.borrow()[0].is_err(),
        "an error of the server is an error, with a reason to say"
    );
}

#[test]
fn a_rename_that_cannot_be_asked_says_so() {
    let venus = installed();
    let lsp = Lsp::with_starter(
        std::path::PathBuf::from("/project"),
        Box::new(|_| Err(std::io::Error::other("none"))),
        true,
    );
    let given = rename(&lsp, 7, "Fresh");
    settle(&venus);
    assert!(
        given.borrow()[0]
            .as_ref()
            .unwrap_err()
            .contains("language server"),
        "{:?}",
        given.borrow()
    );
}

#[test]
fn the_places_that_use_a_name_are_asked_for_without_the_definition_and_are_given_as_places() {
    let (venus, fake, lsp, _controller) = ready();
    let given: Rc<RefCell<Vec<Option<Vec<Place>>>>> = Rc::default();
    let into = Rc::clone(&given);
    lsp.references(Path::new(FILE), 7, move |places| {
        into.borrow_mut().push(places)
    });
    let told = fake.requests();
    assert_eq!(told[0].0, "textDocument/references");
    assert_eq!(told[0].1["context"]["includeDeclaration"], false);
    fake.answer(
        told[0].2.expect("a request"),
        json!([{"uri": "file:///project/src/main.rs", "range": {"start": {"line": 1, "character": 24}, "end": {"line": 1, "character": 29}}}]),
    );
    settle(&venus);
    let given = given.borrow();
    let places = given[0].as_ref().expect("the places");
    assert_eq!(places.len(), 1);
    assert_eq!((places[0].at.line, places[0].at.character), (1, 24));
}

#[test]
fn no_places_is_an_empty_list_and_an_error_is_none() {
    let (venus, fake, lsp, _controller) = ready();
    let given: Rc<RefCell<Vec<Option<Vec<Place>>>>> = Rc::default();
    for answer in [Some(Value::Null), None] {
        let into = Rc::clone(&given);
        lsp.references(Path::new(FILE), 7, move |places| {
            into.borrow_mut().push(places)
        });
        let id = fake.requests()[0].2.expect("a request");
        match answer {
            Some(value) => fake.answer(id, value),
            None => fake.fail(id),
        }
        settle(&venus);
    }
    assert_eq!(*given.borrow(), vec![Some(Vec::new()), None]);
}
