use std::cell::RefCell;
use std::rc::Rc;

use serde_json::json;

use super::*;
use crate::services::lsp::fake::{Fake, installed, lsp_with, settle};
use crate::model::tree::Scratch;

const UNFORMATTED: &str = "fn main(){let x=1;}\n";
const FORMATTED_4: &str = "fn main() {\n    let x = 1;\n}\n";
const FORMATTED_2: &str = "fn main() {\n  let x = 1;\n}\n";

fn notify() -> Rc<dyn Fn()> {
    Rc::new(|| {})
}

fn wait_all(venus: &Rc<aimer::Venus>) {
    for _ in 0..200 {
        if venus.task_count() == 0 && !venus.has_ready_work() {
            return;
        }
        venus.run_microtasks();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

#[test]
fn auto_mode_uses_the_language_server_when_it_answers() {
    let venus = installed();
    let scratch = Scratch::new("ws-format-lsp");
    let file = scratch.file("main.rs", UNFORMATTED);
    let fake = Fake::new();
    let lsp = lsp_with(&fake);
    let doc = Document::open(1, &file, notify()).unwrap();
    lsp.open(&file, &doc.controller, notify());
    settle(&venus);
    let _ = fake.requests();
    fake.answer(
        1,
        json!({"capabilities": {"textDocumentSync": 2, "positionEncoding": "utf-8"}}),
    );
    settle(&venus);
    let _ = fake.requests();

    let outcomes: Rc<RefCell<Vec<Result<(), String>>>> = Rc::default();
    let recorded = Rc::clone(&outcomes);
    format_document(Rc::clone(&doc), lsp, Formatter::Auto, 4, move |result| {
        recorded.borrow_mut().push(result);
    });

    let requests = fake.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].0, "textDocument/formatting");
    fake.answer(
        requests[0].2.unwrap(),
        json!([
            {
                "range": {"start": {"line": 0, "character": 9}, "end": {"line": 0, "character": 9}},
                "newText": " ",
            },
            {
                "range": {"start": {"line": 0, "character": 10}, "end": {"line": 0, "character": 18}},
                "newText": "\n    let x = 1;\n",
            },
        ]),
    );
    settle(&venus);

    assert_eq!(doc.controller.value().text(), FORMATTED_4);
    assert_eq!(*outcomes.borrow(), vec![Ok(())]);
}

#[test]
fn auto_mode_falls_back_to_the_external_formatter_when_the_server_fails_or_is_not_running() {
    let venus = installed();
    let scratch = Scratch::new("ws-format-fallback");
    let file = scratch.file("main.rs", UNFORMATTED);
    let fake = Fake::new();
    let lsp = lsp_with(&fake);
    let doc = Document::open(1, &file, notify()).unwrap();
    lsp.open(&file, &doc.controller, notify());
    settle(&venus);
    let _ = fake.requests();
    fake.answer(
        1,
        json!({"capabilities": {"textDocumentSync": 2, "positionEncoding": "utf-8"}}),
    );
    settle(&venus);
    let _ = fake.requests();

    let outcomes: Rc<RefCell<Vec<Result<(), String>>>> = Rc::default();
    let recorded = Rc::clone(&outcomes);
    format_document(Rc::clone(&doc), lsp, Formatter::Auto, 2, move |result| {
        recorded.borrow_mut().push(result);
    });

    let requests = fake.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].0, "textDocument/formatting");
    // The server fails the formatting request; Auto mode must fall back to rustfmt.
    fake.fail(requests[0].2.unwrap());
    wait_all(&venus);

    assert_eq!(doc.controller.value().text(), FORMATTED_2);
    assert_eq!(*outcomes.borrow(), vec![Ok(())]);
}

#[test]
fn lsp_only_mode_formats_with_the_server_and_does_not_fall_back_when_it_fails() {
    let venus = installed();
    let scratch = Scratch::new("ws-format-lsp-only");
    let file = scratch.file("main.rs", UNFORMATTED);
    let fake = Fake::new();
    let lsp = lsp_with(&fake);
    let doc = Document::open(1, &file, notify()).unwrap();
    lsp.open(&file, &doc.controller, notify());
    settle(&venus);
    let _ = fake.requests();
    fake.answer(
        1,
        json!({"capabilities": {"textDocumentSync": 2, "positionEncoding": "utf-8"}}),
    );
    settle(&venus);
    let _ = fake.requests();

    let outcomes: Rc<RefCell<Vec<Result<(), String>>>> = Rc::default();
    let recorded = Rc::clone(&outcomes);
    format_document(Rc::clone(&doc), lsp, Formatter::Lsp, 2, move |result| {
        recorded.borrow_mut().push(result);
    });

    let requests = fake.requests();
    assert_eq!(requests.len(), 1);
    fake.fail(requests[0].2.unwrap());
    wait_all(&venus);

    assert_eq!(
        doc.controller.value().text(),
        UNFORMATTED,
        "LSP-only mode does not run the external formatter when the server fails"
    );
    assert!(outcomes.borrow()[0].is_err());
}

#[test]
fn external_only_mode_runs_the_external_formatter_without_asking_the_server() {
    let venus = installed();
    let scratch = Scratch::new("ws-format-ext-only");
    let file = scratch.file("main.rs", UNFORMATTED);
    let fake = Fake::new();
    let lsp = lsp_with(&fake);
    let doc = Document::open(1, &file, notify()).unwrap();
    lsp.open(&file, &doc.controller, notify());
    settle(&venus);
    let _ = fake.requests();
    fake.answer(
        1,
        json!({"capabilities": {"textDocumentSync": 2, "positionEncoding": "utf-8"}}),
    );
    settle(&venus);
    let _ = fake.requests();

    let outcomes: Rc<RefCell<Vec<Result<(), String>>>> = Rc::default();
    let recorded = Rc::clone(&outcomes);
    format_document(
        Rc::clone(&doc),
        lsp,
        Formatter::External,
        4,
        move |result| {
            recorded.borrow_mut().push(result);
        },
    );
    wait_all(&venus);

    assert!(
        fake.requests()
            .iter()
            .all(|(method, _, _)| method != "textDocument/formatting"),
        "External mode does not ask the language server to format"
    );
    assert_eq!(doc.controller.value().text(), FORMATTED_4);
    assert_eq!(*outcomes.borrow(), vec![Ok(())]);
}

#[test]
fn only_the_active_file_is_formatted_and_nothing_runs_when_settings_or_no_file_is_active() {
    let scratch = Scratch::new("ws-active-only");
    let first = scratch.file("first.rs", UNFORMATTED);
    let second = scratch.file("second.rs", UNFORMATTED);
    let mut space = Workspace::new(scratch.0.clone());

    // No file open.
    assert!(space.active_document_for_format().is_none());

    space.open(&first, notify()).unwrap();
    space.open(&second, notify()).unwrap();
    assert_eq!(
        space
            .active_document_for_format()
            .map(|(doc, _)| doc.path.clone()),
        Some(second.clone())
    );

    // When the settings tab is active, no document is formatted.
    space.open_settings();
    assert!(space.active_document_for_format().is_none());

    // Activating the second file again targets only `second.rs` and leaves `first.rs` untouched.
    let second_id = space
        .documents
        .iter()
        .find(|doc| doc.path == second)
        .unwrap()
        .id;
    space.activate(second_id);
    let (active_doc, active_lsp) = space
        .active_document_for_format()
        .expect("active document");
    let outcomes: Rc<RefCell<Vec<Result<(), String>>>> = Rc::default();
    let recorded = Rc::clone(&outcomes);
    format_document(
        active_doc,
        active_lsp,
        Formatter::External,
        4,
        move |result| {
            recorded.borrow_mut().push(result);
        },
    );
    assert_eq!(*outcomes.borrow(), vec![Ok(())]);
    let first_doc = space
        .documents
        .iter()
        .find(|doc| doc.path == first)
        .unwrap();
    let second_doc = space
        .documents
        .iter()
        .find(|doc| doc.path == second)
        .unwrap();
    assert_eq!(
        first_doc.controller.value().text(),
        UNFORMATTED,
        "inactive file is untouched"
    );
    assert_eq!(
        second_doc.controller.value().text(),
        FORMATTED_4,
        "only the active file is formatted"
    );

    space.report_format("second.rs", Err("syntax error".to_owned()));
    assert_eq!(
        space.message.as_deref(),
        Some("Could not format second.rs: syntax error")
    );
    space.report_format("second.rs", Ok(()));
    assert_eq!(space.message, None);
}
