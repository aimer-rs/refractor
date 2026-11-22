use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use aimer::input::TextEditingController;
use serde_json::{Value, json};

use super::*;
use crate::lsp::fake::{Fake, installed, lsp_with, settle};

const FILE: &str = "/project/src/main.rs";
const TEXT: &str = "fn main() {\n    let x: Rc<u8> = make();\n}\n";

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

type Given = Rc<RefCell<Vec<Option<Vec<CodeFix>>>>>;

fn ask(lsp: &Lsp, range: std::ops::Range<usize>) -> (Option<i64>, Given) {
    let given: Given = Rc::default();
    let into = Rc::clone(&given);
    let id = lsp.code_actions(Path::new(FILE), range, move |fixes| {
        into.borrow_mut().push(fixes)
    });
    (id, given)
}

fn import_action() -> Value {
    json!([{
        "title": "Import `std::rc::Rc`",
        "isPreferred": true,
        "edit": {"changes": {"file:///project/src/main.rs": [
            {"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}, "newText": "use std::rc::Rc;\n"},
        ]}},
    }])
}

#[test]
fn the_fixes_of_a_range_are_asked_for_and_given_as_bytes_with_their_titles() {
    let (venus, fake, lsp, _controller) = ready();
    // `Rc` of the second line: the line starts at byte 12 and it is at 12 + 11.
    let (id, given) = ask(&lsp, 23..25);
    let told = fake.requests();
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].0, "textDocument/codeAction");
    assert_eq!(
        told[0].1["range"],
        json!({"start": {"line": 1, "character": 11}, "end": {"line": 1, "character": 13}})
    );
    assert_eq!(told[0].1["context"]["only"], json!(["quickfix"]));
    assert_eq!(id, told[0].2);
    fake.answer(id.unwrap(), import_action());
    settle(&venus);
    let given = given.borrow();
    assert_eq!(given.len(), 1, "once");
    let fixes = given[0].as_ref().expect("the server said");
    assert_eq!(fixes.len(), 1);
    assert_eq!(
        (fixes[0].title.as_str(), fixes[0].preferred),
        ("Import `std::rc::Rc`", true)
    );
    assert_eq!(
        fixes[0].edits,
        vec![(0..0, "use std::rc::Rc;\n".to_owned())]
    );
}

#[test]
fn a_server_that_has_none_to_say_gives_none_and_an_error_or_a_cancelled_request_gives_nothing() {
    let (venus, fake, lsp, _controller) = ready();
    let (id, given) = ask(&lsp, 23..25);
    let _ = fake.requests();
    fake.answer(id.unwrap(), Value::Null);
    settle(&venus);
    assert_eq!(
        given.borrow()[0].as_ref().map(Vec::len),
        Some(0),
        "nothing to offer is an empty list, which is an answer"
    );
    let (id, given) = ask(&lsp, 23..25);
    let _ = fake.requests();
    fake.fail(id.unwrap());
    settle(&venus);
    assert!(given.borrow()[0].is_none(), "the server could not say");
    let (id, given) = ask(&lsp, 23..25);
    let _ = fake.requests();
    lsp.cancel(id.unwrap());
    fake.answer(id.unwrap(), import_action());
    settle(&venus);
    assert!(given.borrow().is_empty(), "not wanted");
}

#[test]
fn a_file_that_is_not_open_has_none() {
    let (_venus, fake, lsp, _controller) = ready();
    let given: Given = Rc::default();
    let into = Rc::clone(&given);
    assert_eq!(
        lsp.code_actions(Path::new("/project/src/other.rs"), 0..1, move |fixes| into
            .borrow_mut()
            .push(fixes)),
        None
    );
    assert!(fake.requests().is_empty());
    assert!(matches!(given.borrow().as_slice(), [None]));
}

fn fix(edits: Vec<(std::ops::Range<usize>, &str)>, text: &str) -> CodeFix {
    CodeFix {
        title: "t".into(),
        preferred: false,
        edits: edits
            .into_iter()
            .map(|(range, new)| (range, new.to_owned()))
            .collect(),
        text: TextSnapshot::from_text(text),
    }
}

#[test]
fn a_fix_puts_its_edits_in_as_one_edit_that_one_undo_takes_back_and_the_caret_stays_with_its_text()
{
    let controller = TextEditingController::with_text(TEXT);
    // The caret is after `make`: byte 12 + 22 = 34.
    controller.select_bytes(34, 34);
    let fixed = fix(vec![(0..0, "use std::rc::Rc;\n")], TEXT);
    assert!(fixed.apply(&controller));
    assert_eq!(
        controller.value().text(),
        format!("use std::rc::Rc;\n{TEXT}")
    );
    assert_eq!(
        controller.value().selection().range(),
        34 + 17..34 + 17,
        "moved down with the line that went in above it"
    );
    assert!(controller.undo());
    assert_eq!(controller.value().text(), TEXT, "one undo");
}

#[test]
fn a_fix_with_edits_on_both_sides_of_the_caret_moves_it_by_those_before_it_only() {
    let controller = TextEditingController::with_text("abcdefghij");
    controller.select_bytes(5, 5);
    let fixed = fix(vec![(0..2, "XXXX"), (8..10, "")], "abcdefghij");
    assert!(fixed.apply(&controller));
    assert_eq!(controller.value().text(), "XXXXcdefgh");
    assert_eq!(
        controller.value().selection().range(),
        7..7,
        "two bytes were made four before it"
    );
}

#[test]
fn a_caret_in_what_a_fix_replaces_goes_to_the_start_of_it_and_a_selection_keeps_both_its_ends() {
    let controller = TextEditingController::with_text("abcdefghij");
    controller.select_bytes(3, 6);
    let fixed = fix(vec![(4..5, "ZZ")], "abcdefghij");
    assert!(fixed.apply(&controller));
    assert_eq!(controller.value().text(), "abcdZZfghij");
    assert_eq!(
        controller.value().selection().range(),
        3..7,
        "the end is after what was replaced and moved by it"
    );
}

#[test]
fn a_fix_is_not_applied_to_a_text_that_is_not_the_one_it_was_made_for() {
    let controller = TextEditingController::with_text(TEXT);
    let fixed = fix(vec![(0..0, "use std::rc::Rc;\n")], TEXT);
    controller.replace_range(0..0, "// typed since\n");
    assert!(
        !fixed.apply(&controller),
        "the places are of a text that is not this one"
    );
    assert!(
        controller
            .value()
            .text()
            .starts_with("// typed since\nfn main")
    );
    assert_eq!(
        controller
            .value()
            .text()
            .matches("use std::rc::Rc;")
            .count(),
        0
    );
}

#[test]
fn edits_that_overlap_are_not_applied() {
    let controller = TextEditingController::with_text("abcdef");
    let fixed = fix(vec![(1..4, "x"), (3..5, "y")], "abcdef");
    assert!(!fixed.apply(&controller));
    assert_eq!(controller.value().text(), "abcdef");
}
