use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use aimer::input::TextEditingController;
use serde_json::{Value, json};

use super::*;
use crate::lsp::fake::{Fake, installed, lsp_with, settle};

const FILE: &str = "/project/src/main.rs";
const TEXT: &str = "fn main() {\n    let x = 1;\n    let é = 2;\n}\n";

/// A server that is ready, with the file open, that says what it can through `capabilities`.
fn ready(capabilities: Value) -> (Rc<aimer::Venus>, Fake, Lsp) {
    let venus = installed();
    let fake = Fake::new();
    let lsp = lsp_with(&fake);
    lsp.open(Path::new(FILE), &TextEditingController::with_text(TEXT), Rc::new(|| {}));
    settle(&venus);
    let _ = fake.requests();
    fake.answer(1, json!({"capabilities": capabilities}));
    settle(&venus);
    let _ = fake.requests();
    (venus, fake, lsp)
}

fn utf8_server() -> Value {
    json!({"textDocumentSync": 2, "positionEncoding": "utf-8", "inlayHintProvider": true})
}

type Given = Rc<RefCell<Vec<Option<Vec<Hint>>>>>;

fn ask(lsp: &Lsp, lines: std::ops::Range<usize>) -> (Option<i64>, Given) {
    let given: Given = Rc::default();
    let into = Rc::clone(&given);
    let id = lsp.inlay_hints(Path::new(FILE), lines, move |hints| into.borrow_mut().push(hints));
    (id, given)
}

fn type_hint(line: u32, character: u32, label: &str) -> Value {
    json!({"position": {"line": line, "character": character}, "label": label, "kind": 1})
}

#[test]
fn the_hints_of_some_lines_are_asked_for_and_given_as_bytes_in_the_order_of_the_text() {
    let (venus, fake, lsp) = ready(utf8_server());
    let (id, given) = ask(&lsp, 1..3);
    let told = fake.requests();
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].0, "textDocument/inlayHint");
    assert_eq!(
        told[0].1,
        json!({"textDocument": {"uri": "file:///project/src/main.rs"},
               "range": {"start": {"line": 1, "character": 0}, "end": {"line": 3, "character": 0}}})
    );
    assert_eq!(id, told[0].2);
    // The server may say them in any order.
    fake.answer(id.unwrap(), json!([type_hint(2, 10, ": i32"), type_hint(1, 9, ": i32")]));
    settle(&venus);
    let given = given.borrow();
    assert_eq!(given.len(), 1, "once");
    let hints = given[0].as_ref().expect("the server said");
    // The second line starts at byte 12 and the third at 27; this server counts bytes, and the é is two.
    assert_eq!(hints.iter().map(|hint| hint.at).collect::<Vec<_>>(), [21, 37]);
    assert_eq!((hints[0].label.as_str(), hints[0].kind), (": i32", HintKind::Type));
}

#[test]
fn a_range_never_ends_past_the_text_because_a_server_may_refuse_a_position_that_is_not_in_it() {
    // rust-analyzer answers an error to a range that ends on a line the text does not have, which the protocol says is the end.
    let (_venus, fake, lsp) = ready(utf8_server());
    let _ = ask(&lsp, 0..99);
    // The text ends in a line break, so what follows it is a line of its own, which is empty: the range ends at its start.
    assert_eq!(
        fake.requests()[0].1["range"],
        json!({"start": {"line": 0, "character": 0}, "end": {"line": 4, "character": 0}})
    );
    let _ = ask(&lsp, 2..4);
    assert_eq!(fake.requests()[0].1["range"]["end"], json!({"line": 4, "character": 0}), "the line that is the end is in the text");
}

#[test]
fn a_range_of_a_text_that_has_no_line_break_at_its_end_ends_at_the_end_of_its_last_line() {
    let venus = installed();
    let fake = Fake::new();
    let lsp = lsp_with(&fake);
    lsp.open(Path::new(FILE), &TextEditingController::with_text("fn a() {}\nfn é() {}"), Rc::new(|| {}));
    settle(&venus);
    let _ = fake.requests();
    fake.answer(1, json!({"capabilities": utf8_server()}));
    settle(&venus);
    let _ = fake.requests();
    let _ = ask(&lsp, 0..50);
    // The second line is nine characters of ten bytes, and this server counts bytes.
    assert_eq!(fake.requests()[0].1["range"]["end"], json!({"line": 1, "character": 10}));
}

#[test]
fn a_position_counted_in_utf16_is_the_byte_it_is_in_the_text() {
    let (venus, fake, lsp) = ready(json!({"textDocumentSync": 2, "inlayHintProvider": true}));
    let (id, given) = ask(&lsp, 2..3);
    let _ = fake.requests();
    // `    let é` is nine characters, and ten bytes: the hint is after it.
    fake.answer(id.unwrap(), json!([type_hint(2, 9, ": i32")]));
    settle(&venus);
    assert_eq!(given.borrow()[0].as_ref().unwrap()[0].at, 27 + 10);
}

#[test]
fn a_server_that_has_no_hints_is_not_asked_and_has_none() {
    let (_venus, fake, lsp) = ready(json!({"textDocumentSync": 2, "positionEncoding": "utf-8"}));
    let (id, given) = ask(&lsp, 0..4);
    assert_eq!(id, None);
    assert!(fake.requests().is_empty(), "nothing is sent");
    assert_eq!(*given.borrow(), vec![None], "and it is said at once");
}

#[test]
fn a_file_that_is_not_open_has_none() {
    let (_venus, fake, lsp) = ready(utf8_server());
    let given: Given = Rc::default();
    let into = Rc::clone(&given);
    assert_eq!(lsp.inlay_hints(Path::new("/project/src/other.rs"), 0..4, move |hints| into.borrow_mut().push(hints)), None);
    assert!(fake.requests().is_empty());
    assert_eq!(*given.borrow(), vec![None]);
}

#[test]
fn an_error_of_the_server_is_none_and_a_cancelled_request_is_not_given() {
    let (venus, fake, lsp) = ready(utf8_server());
    let (id, given) = ask(&lsp, 0..4);
    let _ = fake.requests();
    fake.fail(id.unwrap());
    settle(&venus);
    assert_eq!(*given.borrow(), vec![None], "the server could not say");

    let (id, given) = ask(&lsp, 0..4);
    let _ = fake.requests();
    lsp.cancel(id.unwrap());
    fake.answer(id.unwrap(), json!([type_hint(1, 9, ": i32")]));
    settle(&venus);
    assert!(given.borrow().is_empty(), "not wanted");
}

#[test]
fn a_hint_past_the_end_of_its_line_or_of_the_text_is_at_the_end() {
    let (venus, fake, lsp) = ready(utf8_server());
    let (id, given) = ask(&lsp, 0..4);
    let _ = fake.requests();
    fake.answer(id.unwrap(), json!([type_hint(0, 500, "a"), type_hint(90, 0, "b")]));
    settle(&venus);
    let given = given.borrow();
    let hints = given[0].as_ref().unwrap();
    assert_eq!(hints[0].at, 11, "the end of the first line, before its line break");
    assert_eq!(hints[1].at, TEXT.len());
}
