use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use aimer::Venus;
use aimer::input::TextEditingController;
use serde_json::json;

use crate::services::lsp::fake::{Fake, installed, lsp_with, settle};
use crate::services::lsp::{Lsp, SignatureHelp};
use crate::model::param_hint::ParamHintCell;
use crate::model::syntax::{SyntaxHighlighter, SyntaxPalette};

const FILE: &str = "/project/src/main.rs";

fn ready_lsp(venus: &Rc<Venus>, fake: &Fake, text: &str) -> (Lsp, TextEditingController) {
    let lsp = lsp_with(fake);
    let controller = TextEditingController::with_text(text);
    lsp.open(Path::new(FILE), &controller, Rc::new(|| {}));
    settle(venus);
    let _ = fake.methods();
    fake.answer(
        1,
        json!({
            "capabilities": {
                "textDocumentSync": 2,
                "positionEncoding": "utf-8",
                "signatureHelpProvider": {"triggerCharacters": ["(", ","]},
            }
        }),
    );
    settle(venus);
    let _ = fake.methods();
    (lsp, controller)
}

#[test]
fn shows_active_and_inactive_parameters_when_caret_is_inside_call_and_hides_when_leaving() {
    let venus = installed();
    let fake = Fake::new();
    let (lsp, controller) = ready_lsp(&venus, &fake, "fn main() { foo(1, 2); }\n");
    let syntax = SyntaxHighlighter::rust(SyntaxPalette::dark());
    let cell = Rc::new(ParamHintCell::default());
    let redraws = Rc::new(Cell::new(0));
    let counted = Rc::clone(&redraws);
    cell.on_change(Rc::new(move || {
        counted.set(counted.get() + 1);
        true
    }));
    cell.attach(lsp, PathBuf::from(FILE), controller.clone(), syntax);

    // Caret outside call: nothing asked.
    controller.select_bytes(0, 0);
    cell.refresh();
    settle(&venus);
    assert!(fake.methods().is_empty());
    assert!(!cell.is_open());

    // Caret inside `foo(1, |2)`: asks for signatureHelp.
    let second_arg = controller.value().text().find("2);").unwrap();
    controller.select_bytes(second_arg, second_arg);
    cell.refresh();
    settle(&venus);
    let told = fake.methods();
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].0, "textDocument/signatureHelp");

    fake.answer(
        2,
        json!({
            "signatures": [{
                "label": "fn foo(first: i32, second: i32)",
                "parameters": [
                    {"label": "first: i32"},
                    {"label": "second: i32"},
                ],
            }],
            "activeParameter": 1,
        }),
    );
    settle(&venus);
    assert_eq!(
        cell.with(|help| help.cloned()),
        Some(SignatureHelp {
            parameters: vec!["first: i32".to_owned(), "second: i32".to_owned()],
            active_parameter: Some(1),
        })
    );
    assert_eq!(redraws.get(), 1);

    // Caret moves outside `foo(1, 2)|`: hides immediately without asking the server.
    let after = controller.value().text().find(';').unwrap();
    controller.select_bytes(after, after);
    cell.refresh();
    settle(&venus);
    assert!(!cell.is_open());
    assert!(fake.methods().is_empty());
    assert_eq!(redraws.get(), 2);

    let _ = Venus::uninstall();
}

#[test]
fn dismiss_keeps_hint_hidden_for_same_call_until_entering_another_call() {
    let venus = installed();
    let fake = Fake::new();
    let (lsp, controller) = ready_lsp(&venus, &fake, "fn main() { foo(1, 2); bar(3); }\n");
    let syntax = SyntaxHighlighter::rust(SyntaxPalette::dark());
    let cell = Rc::new(ParamHintCell::default());
    cell.attach(lsp, PathBuf::from(FILE), controller.clone(), syntax);

    let in_foo_1 = controller.value().text().find("1, 2").unwrap();
    controller.select_bytes(in_foo_1, in_foo_1);
    cell.refresh();
    settle(&venus);
    let _ = fake.methods();
    fake.answer(
        2,
        json!({
            "signatures": [{
                "label": "fn foo(a: i32, b: i32)",
                "parameters": [{"label": "a: i32"}, {"label": "b: i32"}],
            }],
            "activeParameter": 0,
        }),
    );
    settle(&venus);
    assert!(cell.is_open());

    // Escape dismisses it.
    assert!(cell.dismiss());
    assert!(!cell.is_open());

    // Moving inside the same call does not reopen it.
    let in_foo_2 = controller.value().text().find("2);").unwrap();
    controller.select_bytes(in_foo_2, in_foo_2);
    cell.refresh();
    settle(&venus);
    assert!(fake.methods().is_empty());
    assert!(!cell.is_open());

    // Moving into a different call (`bar(3)`) asks again.
    let in_bar = controller.value().text().find("3);").unwrap();
    controller.select_bytes(in_bar, in_bar);
    cell.refresh();
    settle(&venus);
    assert_eq!(fake.methods().len(), 1);

    let _ = Venus::uninstall();
}

#[test]
fn non_empty_selection_hides_parameter_hint() {
    let venus = installed();
    let fake = Fake::new();
    let (lsp, controller) = ready_lsp(&venus, &fake, "fn main() { foo(123); }\n");
    let syntax = SyntaxHighlighter::rust(SyntaxPalette::dark());
    let cell = Rc::new(ParamHintCell::default());
    cell.attach(lsp, PathBuf::from(FILE), controller.clone(), syntax);

    let at = controller.value().text().find("123").unwrap();
    controller.select_bytes(at, at);
    cell.refresh();
    settle(&venus);
    let _ = fake.methods();
    fake.answer(
        2,
        json!({
            "signatures": [{
                "label": "fn foo(x: i32)",
                "parameters": [{"label": "x: i32"}],
            }],
        }),
    );
    settle(&venus);
    assert!(cell.is_open());

    controller.select_bytes(at, at + 3);
    cell.refresh();
    assert!(!cell.is_open());

    let _ = Venus::uninstall();
}
