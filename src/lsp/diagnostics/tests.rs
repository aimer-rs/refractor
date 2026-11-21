use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use aimer::input::TextEditingController;
use serde_json::{Value, json};

use super::*;
use crate::lsp::fake::{Fake, installed, lsp_with, settle};
use crate::lsp::protocol::Severity;

const FILE: &str = "/project/src/main.rs";
const TEXT: &str = "fn main() {\n    let x = 1;\n}\n";

fn ready() -> (Rc<aimer::Venus>, Fake, Lsp, TextEditingController) {
    let venus = installed();
    let fake = Fake::new();
    let lsp = lsp_with(&fake);
    let controller = TextEditingController::with_text(TEXT);
    lsp.open(Path::new(FILE), &controller, Rc::new(|| {}));
    settle(&venus);
    let _ = fake.requests();
    fake.answer(1, json!({"capabilities": {"textDocumentSync": 2, "positionEncoding": "utf-8"}}));
    settle(&venus);
    let _ = fake.requests();
    (venus, fake, lsp, controller)
}

fn publish(fake: &Fake, version: i32, diagnostics: Value) {
    fake.notify(
        "textDocument/publishDiagnostics",
        json!({"uri": "file:///project/src/main.rs", "version": version, "diagnostics": diagnostics}),
    );
}

fn reported(line: u32, from: u32, to: u32, severity: u32, message: &str) -> Value {
    json!({"range": {"start": {"line": line, "character": from}, "end": {"line": line, "character": to}}, "severity": severity, "message": message})
}

#[test]
fn what_the_server_says_is_wrong_is_given_as_bytes_with_the_text_they_are_of() {
    let (venus, fake, lsp, _controller) = ready();
    let (none, _) = lsp.diagnostics(Path::new(FILE)).expect("a file that is open");
    assert!(none.is_empty(), "nothing is published yet");
    publish(&fake, 1, json!([reported(1, 8, 9, 1, "unused"), reported(1, 4, 7, 2, "a warning")]));
    settle(&venus);
    let (found, text) = lsp.diagnostics(Path::new(FILE)).expect("current");
    assert_eq!(text.len(), TEXT.len());
    // The second line starts at byte 12.
    let summary: Vec<_> = found.iter().map(|found| (found.range.clone(), found.severity)).collect();
    assert_eq!(summary, [(20..21, Severity::Error), (16..19, Severity::Warning)]);
}

#[test]
fn a_file_that_is_not_open_has_none_and_one_that_is_edited_and_not_yet_told_still_has_what_was_said() {
    let (venus, fake, lsp, controller) = ready();
    assert!(lsp.diagnostics(Path::new("/project/src/other.rs")).is_none());
    publish(&fake, 1, json!([reported(1, 8, 9, 1, "unused")]));
    settle(&venus);
    // Typed, and not yet told to the server: what it said is of the text it has, which is given with it.
    controller.set_text(&format!("// c\n{TEXT}"));
    let (found, text) = lsp.diagnostics(Path::new(FILE)).expect("what the server said is of what it has");
    assert_eq!((found.len(), text.len()), (1, TEXT.len()));
}

#[test]
fn whoever_asked_to_know_is_told_when_what_the_server_says_changes_and_one_that_is_gone_is_forgotten() {
    let (venus, fake, lsp, _controller) = ready();
    let told = Rc::new(Cell::new(0));
    let counted = Rc::clone(&told);
    lsp.on_diagnostics(Rc::new(move || {
        counted.set(counted.get() + 1);
        true
    }));
    let gone = Rc::new(Cell::new(0));
    let seen = Rc::clone(&gone);
    lsp.on_diagnostics(Rc::new(move || {
        seen.set(seen.get() + 1);
        false
    }));
    publish(&fake, 1, json!([reported(1, 8, 9, 1, "unused")]));
    settle(&venus);
    assert_eq!((told.get(), gone.get()), (1, 1));
    publish(&fake, 1, json!([]));
    settle(&venus);
    assert_eq!((told.get(), gone.get()), (2, 1), "the one that said it was gone is not asked again");
}
