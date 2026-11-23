use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use aimer::input::TextEditingController;
use serde_json::{Value, json};

use super::*;
use crate::services::lsp::fake::{Fake, installed, lsp_with, settle};
use crate::model::tree::Scratch;

const FILE: &str = "/project/src/main.rs";
const UNFORMATTED: &str = "fn main(){let x=1;}\n";
const FORMATTED: &str = "fn main() {\n    let x = 1;\n}\n";

fn ready() -> (Rc<aimer::Venus>, Fake, Lsp, TextEditingController) {
    let venus = installed();
    let fake = Fake::new();
    let lsp = lsp_with(&fake);
    let controller = TextEditingController::with_text(UNFORMATTED);
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

type Given = Rc<RefCell<Vec<Option<FormatEdit>>>>;

fn ask(lsp: &Lsp, tab_spaces: usize) -> (Option<i64>, Given) {
    let given: Given = Rc::default();
    let into = Rc::clone(&given);
    let id = lsp.format(Path::new(FILE), tab_spaces, move |edit| {
        into.borrow_mut().push(edit)
    });
    (id, given)
}

#[test]
fn formatting_a_file_asks_the_server_with_the_tab_width_and_gives_the_edits_as_bytes() {
    let (venus, fake, lsp, _controller) = ready();
    let (id, given) = ask(&lsp, 2);
    let told = fake.requests();
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].0, "textDocument/formatting");
    assert_eq!(
        told[0].1,
        json!({
            "textDocument": {"uri": "file:///project/src/main.rs"},
            "options": {"tabSize": 2, "insertSpaces": true},
        })
    );
    assert_eq!(id, told[0].2);
    fake.answer(
        id.unwrap(),
        json!([
            {
                "range": {"start": {"line": 0, "character": 9}, "end": {"line": 0, "character": 9}},
                "newText": " ",
            },
            {
                "range": {"start": {"line": 0, "character": 10}, "end": {"line": 0, "character": 18}},
                "newText": "\n  let x = 1;\n",
            },
        ]),
    );
    settle(&venus);
    let given = given.borrow();
    assert_eq!(given.len(), 1);
    let edit = given[0].as_ref().expect("the server answered");
    assert_eq!(
        edit.edits,
        vec![
            (9..9, " ".to_owned()),
            (10..18, "\n  let x = 1;\n".to_owned()),
        ]
    );
}

#[test]
fn a_server_that_answers_null_gives_no_edits_and_an_error_or_closed_file_gives_none() {
    let (venus, fake, lsp, _controller) = ready();
    let (id, given) = ask(&lsp, 4);
    let _ = fake.requests();
    fake.answer(id.unwrap(), Value::Null);
    settle(&venus);
    assert_eq!(
        given.borrow()[0]
            .as_ref()
            .map(|edit| edit.edits.len()),
        Some(0),
        "null means the file is already formatted"
    );

    let (id, given) = ask(&lsp, 4);
    let _ = fake.requests();
    fake.fail(id.unwrap());
    settle(&venus);
    assert!(
        given.borrow()[0].is_none(),
        "an error gives None so fallback can run"
    );

    let given: Given = Rc::default();
    let into = Rc::clone(&given);
    assert_eq!(
        lsp.format(Path::new("/project/src/other.rs"), 4, move |edit| {
            into.borrow_mut().push(edit)
        }),
        None
    );
    assert!(matches!(given.borrow().as_slice(), [None]));
}

fn format_edit(edits: Vec<(std::ops::Range<usize>, &str)>, text: &str) -> FormatEdit {
    FormatEdit {
        edits: edits
            .into_iter()
            .map(|(range, new)| (range, new.to_owned()))
            .collect(),
        text: TextSnapshot::from_text(text),
    }
}

#[test]
fn applying_formatting_puts_the_edits_in_as_one_undo_and_keeps_the_caret_and_selection() {
    let controller = TextEditingController::with_text(UNFORMATTED);
    // Caret after `let x=1;` at byte 18 (`}`).
    controller.select_bytes(18, 18);
    let edit = format_edit(
        vec![(9..9, " "), (10..18, "\n    let x = 1;\n")],
        UNFORMATTED,
    );
    assert!(edit.apply(&controller));
    assert_eq!(controller.value().text(), FORMATTED);
    assert_eq!(
        controller.value().selection().range(),
        27..27,
        "moved with the formatted lines before it"
    );
    assert!(controller.undo());
    assert_eq!(
        controller.value().text(),
        UNFORMATTED,
        "one undo restores it"
    );
}

#[test]
fn formatting_handles_empty_text_single_line_no_final_newline_crlf_multibyte_combining_marks_and_long_lines()
 {
    // 1. Empty text.
    let empty = TextEditingController::with_text("");
    let edit = FormatEdit::from_formatted(empty.snapshot(), "");
    assert!(edit.apply(&empty));
    assert_eq!(empty.value().text(), "");

    // 2. Single line without a final line break.
    let single = TextEditingController::with_text("fn a(){}");
    single.select_bytes(7, 7);
    let edit = FormatEdit::from_formatted(single.snapshot(), "fn a() {}");
    assert!(edit.apply(&single));
    assert_eq!(single.value().text(), "fn a() {}");
    assert_eq!(single.value().selection().range(), 8..8);

    // 3. CRLF line endings.
    let crlf_in = "fn a(){\r\nlet x=1;\r\n}\r\n";
    let crlf_out = "fn a() {\r\n    let x = 1;\r\n}\r\n";
    let crlf = TextEditingController::with_text(crlf_in);
    crlf.select_bytes(crlf_in.len(), crlf_in.len());
    let edit = FormatEdit::from_formatted(crlf.snapshot(), crlf_out);
    assert!(edit.apply(&crlf));
    assert_eq!(crlf.value().text(), crlf_out);
    assert_eq!(
        crlf.value().selection().range(),
        crlf_out.len()..crlf_out.len()
    );

    // 4. Multi-byte UTF-8 characters and combining marks (`e\u{301}` and `🦀`).
    let unicode_in = "fn cafe\u{301}()  { let 🦀 = 1; }\n";
    let unicode_out = "fn cafe\u{301}() {\n    let 🦀 = 1;\n}\n";
    let unicode = TextEditingController::with_text(unicode_in);
    // Caret inside the replaced span on a multi-byte char boundary stays on a valid char boundary.
    let crab_byte = unicode_in.find('🦀').unwrap() + 2; // middle byte if unguarded
    unicode.select_bytes(
        unicode_in.find('🦀').unwrap(),
        unicode_in.find('🦀').unwrap() + '🦀'.len_utf8(),
    );
    let edit = FormatEdit::from_formatted(unicode.snapshot(), unicode_out);
    assert!(edit.apply(&unicode));
    assert_eq!(unicode.value().text(), unicode_out);
    let sel = unicode.value().selection().range();
    assert!(unicode_out.is_char_boundary(sel.start) && unicode_out.is_char_boundary(sel.end));
    let _ = crab_byte;

    // 5. Very long line.
    let payload = "a".repeat(8192);
    let long_in = format!("fn long(){{let s=\"{payload}\";}}\n");
    let long_out = format!("fn long() {{\n    let s = \"{payload}\";\n}}\n");
    let long = TextEditingController::with_text(&long_in);
    let edit = FormatEdit::from_formatted(long.snapshot(), &long_out);
    assert!(edit.apply(&long));
    assert_eq!(long.value().text(), long_out);
}

#[test]
fn formatting_an_already_formatted_text_leaves_the_document_clean_and_adds_no_undo_step() {
    let controller = TextEditingController::with_text(FORMATTED);
    controller.select_bytes(10, 10);
    let empty_edit = FormatEdit::from_formatted(controller.snapshot(), FORMATTED);
    assert!(empty_edit.edits.is_empty());
    assert!(empty_edit.apply(&controller));
    assert!(!controller.undo(), "no undo step was added");
    assert_eq!(controller.value().selection().range(), 10..10);

    // Even when a server returns a text edit whose replacement equals what is already there, no undo step is added.
    let same_edit = format_edit(vec![(0..FORMATTED.len(), FORMATTED)], FORMATTED);
    assert!(same_edit.apply(&controller));
    assert!(
        !controller.undo(),
        "identical replacement adds no undo step"
    );
}

#[test]
fn formatting_is_not_applied_if_the_text_changed_since_it_was_asked_for_or_if_edits_overlap() {
    let controller = TextEditingController::with_text(UNFORMATTED);
    let edit = FormatEdit::from_formatted(controller.snapshot(), FORMATTED);
    controller.replace_range(0..0, "// typed while formatting\n");
    assert!(!edit.apply(&controller));
    assert!(
        controller
            .value()
            .text()
            .starts_with("// typed while formatting\nfn main(){")
    );

    let overlap = TextEditingController::with_text("abcdef");
    let bad = format_edit(vec![(1..4, "x"), (3..5, "y")], "abcdef");
    assert!(!bad.apply(&overlap));
    assert_eq!(overlap.value().text(), "abcdef");
}

#[test]
fn the_external_formatter_runs_the_program_on_the_text_and_reports_when_it_fails() {
    let scratch = Scratch::new("ext-format");
    let file = scratch.file("main.rs", UNFORMATTED);
    let controller = TextEditingController::with_text(UNFORMATTED);
    let edit = format_external_with(Language::Rust, &file, controller.snapshot(), 2, None)
        .expect("rustfmt formats valid Rust");
    assert!(edit.apply(&controller));
    assert_eq!(controller.value().text(), "fn main() {\n  let x = 1;\n}\n");

    // Syntax error or missing binary reports an error and leaves the text untouched.
    let broken = TextEditingController::with_text("fn main( {\n");
    assert!(
        format_external_with(Language::Rust, &file, broken.snapshot(), 4, None).is_err(),
        "invalid syntax is reported as an error"
    );
    assert!(
        format_external_with(
            Language::Rust,
            &file,
            controller.snapshot(),
            4,
            Some(Path::new("/no/such/formatter-binary")),
        )
        .is_err(),
        "a program that cannot be run is reported as an error"
    );
    assert!(
        format_external_with(Language::PlainText, &file, controller.snapshot(), 4, None).is_err(),
        "a language without an external formatter says so"
    );
}
