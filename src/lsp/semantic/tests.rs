use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

use aimer::input::TextEditingController;
use serde_json::{Value, json};

use super::*;
use crate::lsp::fake::{Fake, installed, lsp_with, settle};

const FILE: &str = "/project/src/main.rs";
const TEXT: &str = "fn main() {\n    let mut é = 1;\n}\n";

fn server(encoding: &str) -> Value {
    json!({
        "textDocumentSync": 2,
        "positionEncoding": encoding,
        "semanticTokensProvider": {
            "legend": {"tokenTypes": ["keyword", "variable", "function"], "tokenModifiers": ["declaration", "mutable"]},
            "full": true,
        },
    })
}

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

type Given = Rc<RefCell<Vec<Option<SemanticTokens>>>>;

fn ask(lsp: &Lsp) -> (Option<i64>, Given) {
    let given: Given = Rc::default();
    let into = Rc::clone(&given);
    let id = lsp.semantic_tokens(Path::new(FILE), move |tokens| into.borrow_mut().push(tokens));
    (id, given)
}

/// `fn`, `main` and, on the next line, `let`, `mut` and `é`.
fn data() -> Value {
    json!({"resultId": "1", "data": [
        0, 0, 2, 0, 0,
        0, 3, 4, 2, 1,
        1, 4, 3, 0, 0,
        0, 4, 3, 0, 0,
        0, 4, 2, 1, 3,
    ]})
}

#[test]
fn the_tokens_of_a_file_are_asked_for_by_its_address_and_given_as_bytes_with_what_they_are() {
    let (venus, fake, lsp) = ready(server("utf-8"));
    let (id, given) = ask(&lsp);
    let told = fake.requests();
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].0, "textDocument/semanticTokens/full");
    assert_eq!(told[0].1, json!({"textDocument": {"uri": "file:///project/src/main.rs"}}));
    assert_eq!(id, told[0].2);
    fake.answer(id.unwrap(), data());
    settle(&venus);

    let given = given.borrow();
    assert_eq!(given.len(), 1, "once");
    let found = given[0].as_ref().expect("the server said");
    // The second line starts at byte 12: `    let mut é` is a character of two bytes at the end, and this server counts bytes.
    let ranges: Vec<_> = found.tokens.iter().map(|token| token.range.clone()).collect();
    assert_eq!(ranges, [0..2, 3..7, 16..19, 20..23, 24..26]);
    assert_eq!(found.legend.type_name(found.tokens[1].kind), Some("function"));
    assert_eq!(found.legend.modifier_names(found.tokens[4].modifiers).collect::<Vec<_>>(), ["declaration", "mutable"]);
    assert_eq!(found.text.len(), TEXT.len(), "and the text they are of is kept, which what moves them over edits is told by");
}

#[test]
fn a_position_counted_in_utf16_is_the_byte_it_is_in_the_text() {
    let (venus, fake, lsp) = ready(server("utf-16"));
    let (id, given) = ask(&lsp);
    let _ = fake.requests();
    // `é` is one unit of two bytes in the line that was told as bytes: with units it starts at 12 and is one long.
    fake.answer(id.unwrap(), json!({"data": [1, 12, 1, 1, 0]}));
    settle(&venus);
    let found = given.borrow()[0].clone().unwrap();
    assert_eq!(found.tokens.iter().map(|token| token.range.clone()).collect::<Vec<_>>(), [24..26]);
}

#[test]
fn a_token_with_no_length_or_that_ends_where_it_starts_is_left_out() {
    let (venus, fake, lsp) = ready(server("utf-8"));
    let (id, given) = ask(&lsp);
    let _ = fake.requests();
    fake.answer(id.unwrap(), json!({"data": [0, 0, 0, 0, 0, 0, 3, 4, 2, 1, 9, 0, 3, 0, 0]}));
    settle(&venus);
    let found = given.borrow()[0].clone().unwrap();
    assert_eq!(found.tokens.len(), 1, "one with no length, and one on a line that is not there");
    assert_eq!(found.tokens[0].range, 3..7);
}

#[test]
fn a_server_without_tokens_is_not_asked_and_has_none() {
    let (_venus, fake, lsp) = ready(json!({"textDocumentSync": 2, "positionEncoding": "utf-8"}));
    let (id, given) = ask(&lsp);
    assert_eq!(id, None);
    assert!(fake.requests().is_empty());
    assert!(matches!(given.borrow().as_slice(), [None]), "said at once");
}

#[test]
fn a_file_that_is_not_open_has_none() {
    let (_venus, fake, lsp) = ready(server("utf-8"));
    let given: Given = Rc::default();
    let into = Rc::clone(&given);
    assert_eq!(lsp.semantic_tokens(Path::new("/project/src/other.rs"), move |tokens| into.borrow_mut().push(tokens)), None);
    assert!(fake.requests().is_empty());
    assert!(matches!(given.borrow().as_slice(), [None]));
}

#[test]
fn an_error_of_the_server_is_none_and_a_cancelled_request_is_not_given() {
    let (venus, fake, lsp) = ready(server("utf-8"));
    let (id, given) = ask(&lsp);
    let _ = fake.requests();
    fake.fail(id.unwrap());
    settle(&venus);
    assert!(matches!(given.borrow().as_slice(), [None]), "the server could not say");

    let (id, given) = ask(&lsp);
    let _ = fake.requests();
    lsp.cancel(id.unwrap());
    fake.answer(id.unwrap(), data());
    settle(&venus);
    assert!(given.borrow().is_empty(), "not wanted");
}

#[test]
fn what_is_asked_of_a_server_that_says_its_tokens_are_not_as_they_were_is_told_to_whoever_asked_to_know() {
    let (venus, fake, lsp) = ready(server("utf-8"));
    let told = Rc::new(Cell::new(0));
    let counted = Rc::clone(&told);
    lsp.on_semantic_refresh(Rc::new(move || {
        counted.set(counted.get() + 1);
        true
    }));
    let gone = Rc::new(Cell::new(0));
    let seen = Rc::clone(&gone);
    lsp.on_semantic_refresh(Rc::new(move || {
        seen.set(seen.get() + 1);
        false
    }));
    fake.request(json!(7), "workspace/semanticTokens/refresh", Value::Null);
    settle(&venus);
    assert_eq!((told.get(), gone.get()), (1, 1));
    // It is answered, and the one that said it was gone is not asked again.
    let sent = fake.sent.borrow().len();
    assert!(sent >= 1, "answered");
    fake.request(json!(8), "workspace/semanticTokens/refresh", Value::Null);
    settle(&venus);
    assert_eq!((told.get(), gone.get()), (2, 1));
}
