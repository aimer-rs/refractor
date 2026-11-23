use std::cell::RefCell;
use std::path::{Path, PathBuf};

use aimer::input::TextEditingController;
use serde_json::{Value, json};

use super::*;
use crate::services::lsp::Weight;
use crate::services::lsp::fake::{Fake, installed, lsp_with, settle};
use crate::ui::problem_lines::LineColors;

const FILE: &str = "/project/src/main.rs";
const TEXT: &str = "fn main() {\n    let x: Rc<u8> = make();\n}\n";

fn colors() -> LineColors {
    LineColors {
        error: aimer::Color::Rgb(1, 1, 1),
        warning: aimer::Color::Rgb(2, 2, 2),
        weak: aimer::Color::Rgb(3, 3, 3),
    }
}

struct Rig {
    venus: Rc<aimer::Venus>,
    fake: Fake,
    source: CardSource,
    controller: TextEditingController,
    lines: Rc<ProblemLines>,
    shown: Rc<RefCell<Vec<Option<Card>>>>,
}

/// A server that is ready, with the file open and a problem under `Rc` of the second line (bytes 23 to 25) that it has said.
fn rig(problem: bool) -> Rig {
    let venus = installed();
    let fake = Fake::new();
    let lsp = lsp_with(&fake);
    let controller = TextEditingController::with_text(TEXT);
    lsp.open(Path::new(FILE), &controller, Rc::new(|| {}));
    settle(&venus);
    let _ = fake.requests();
    fake.answer(1, json!({"capabilities": {"textDocumentSync": 2, "positionEncoding": "utf-8", "hoverProvider": true}}));
    settle(&venus);
    let _ = fake.requests();
    let lines = Rc::new(ProblemLines::default());
    lines.attach(
        lsp.clone(),
        PathBuf::from(FILE),
        controller.clone(),
        colors(),
    );
    if problem {
        fake.notify(
            "textDocument/publishDiagnostics",
            json!({"uri": "file:///project/src/main.rs", "version": 1, "diagnostics": [{
                "range": {"start": {"line": 1, "character": 11}, "end": {"line": 1, "character": 13}},
                "severity": 1, "message": "Cannot find type `Rc` in this scope", "code": "E0412"}]}),
        );
        settle(&venus);
    }
    let source = CardSource::new(
        Rc::clone(&lines),
        Rc::new(move || lsp.clone()),
        PathBuf::from(FILE),
        controller.clone(),
    );
    Rig {
        venus,
        fake,
        source,
        controller,
        lines,
        shown: Rc::default(),
    }
}

impl Rig {
    fn ask(&self, at: std::ops::Range<usize>) {
        let spot = HoverSpot {
            range: at,
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            extent: None,
        };
        let shown = Rc::clone(&self.shown);
        self.source
            .ask(spot, Rc::new(move |card| shown.borrow_mut().push(card)));
    }

    fn last(&self) -> Option<Card> {
        self.shown.borrow().last().cloned().flatten()
    }
}

fn import() -> Value {
    json!([
        {"title": "Import `std::rc::Rc`", "isPreferred": true, "edit": {"changes": {"file:///project/src/main.rs": [
            {"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}, "newText": "use std::rc::Rc;\n"}]}}},
        {"title": "Import `std::sync::Arc`", "edit": {"changes": {"file:///project/src/main.rs": [
            {"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}, "newText": "use std::sync::Arc;\n"}]}}},
    ])
}

fn requests_of(rig: &Rig) -> Vec<(String, Value, Option<i64>)> {
    rig.fake.requests()
}

#[test]
fn a_place_with_no_problem_has_the_documentation_the_server_has_and_nothing_else() {
    let rig = rig(false);
    rig.ask(16..19);
    let told = requests_of(&rig);
    assert_eq!(
        told.len(),
        1,
        "only the words are asked for: there is no problem to fix"
    );
    assert_eq!(told[0].0, "textDocument/hover");
    rig.fake.answer(
        told[0].2.unwrap(),
        json!({"contents": {"kind": "markdown", "value": "`let`"}}),
    );
    settle(&rig.venus);
    let card = rig.last().expect("shown");
    assert_eq!(card.markdown, "`let`");
    assert!(card.problems.is_empty() && card.fixes.is_empty());
}

#[test]
fn a_place_with_nothing_to_say_has_no_card() {
    let rig = rig(false);
    rig.ask(16..19);
    let told = requests_of(&rig);
    rig.fake.answer(told[0].2.unwrap(), Value::Null);
    settle(&rig.venus);
    assert!(matches!(rig.shown.borrow().as_slice(), [None]));
}

#[test]
fn a_problem_is_shown_at_once_with_what_the_server_said_and_the_fixes_and_the_words_are_put_in_as_they_come()
 {
    let rig = rig(true);
    rig.ask(23..25);
    // At once, before the server has answered anything.
    let first = rig.last().expect("shown at once");
    assert_eq!(first.problems.len(), 1);
    assert_eq!(
        (
            first.problems[0].message.as_str(),
            first.problems[0].code.as_deref()
        ),
        ("Cannot find type `Rc` in this scope", Some("E0412"))
    );
    assert_eq!(first.problems[0].weight, Weight::Error);
    assert!(first.fixes.is_empty() && first.markdown.is_empty());
    let told = requests_of(&rig);
    let methods: Vec<&str> = told
        .iter()
        .map(|(method, ..)| method.as_str())
        .collect();
    assert!(
        methods.contains(&"textDocument/hover") && methods.contains(&"textDocument/codeAction"),
        "{methods:?}"
    );
    let action = told
        .iter()
        .find(|(method, ..)| method == "textDocument/codeAction")
        .unwrap();
    assert_eq!(
        action.1["range"],
        json!({"start": {"line": 1, "character": 11}, "end": {"line": 1, "character": 13}}),
        "the range the problem is under"
    );
    // The fixes come.
    rig.fake.answer(action.2.unwrap(), import());
    settle(&rig.venus);
    let with_fixes = rig.last().unwrap();
    assert_eq!(
        with_fixes
            .fixes
            .iter()
            .map(|fix| fix.title.as_str())
            .collect::<Vec<_>>(),
        ["Import `std::rc::Rc`", "Import `std::sync::Arc`"]
    );
    assert!(with_fixes.fixes[0].preferred && !with_fixes.fixes[1].preferred);
    // And the words, which are under the problem.
    let hover = told
        .iter()
        .find(|(method, ..)| method == "textDocument/hover")
        .unwrap();
    rig.fake.answer(
        hover.2.unwrap(),
        json!({"contents": {"kind": "markdown", "value": "docs"}}),
    );
    settle(&rig.venus);
    let all = rig.last().unwrap();
    assert_eq!(
        (all.markdown.as_str(), all.problems.len(), all.fixes.len()),
        ("docs", 1, 2)
    );
    assert_eq!(all.spot.range, 23..25);
}

#[test]
fn the_fix_the_server_prefers_is_first_whatever_order_it_said_them_in() {
    let rig = rig(true);
    rig.ask(23..25);
    let told = requests_of(&rig);
    let action = told
        .iter()
        .find(|(method, ..)| method == "textDocument/codeAction")
        .unwrap();
    let mut list = import();
    list.as_array_mut().unwrap().reverse();
    rig.fake.answer(action.2.unwrap(), list);
    settle(&rig.venus);
    let fixes = rig.last().unwrap().fixes;
    assert_eq!(fixes[0].title, "Import `std::rc::Rc`");
}

#[test]
fn a_fix_that_is_run_changes_the_text_once_and_says_whether_it_did() {
    let rig = rig(true);
    rig.ask(23..25);
    let told = requests_of(&rig);
    let action = told
        .iter()
        .find(|(method, ..)| method == "textDocument/codeAction")
        .unwrap();
    rig.fake.answer(action.2.unwrap(), import());
    settle(&rig.venus);
    let fix = rig.last().unwrap().fixes.remove(0);
    assert!(fix.run());
    assert_eq!(
        rig.controller.value().text(),
        format!("use std::rc::Rc;\n{TEXT}")
    );
    assert!(
        !fix.run(),
        "it was made for the text that it changed, which is another now"
    );
    assert!(rig.controller.undo());
    assert_eq!(rig.controller.value().text(), TEXT);
    let _ = &rig.lines;
}

#[test]
fn a_fix_for_a_text_that_was_typed_in_since_is_not_run() {
    let rig = rig(true);
    rig.ask(23..25);
    let told = requests_of(&rig);
    let action = told
        .iter()
        .find(|(method, ..)| method == "textDocument/codeAction")
        .unwrap();
    rig.fake.answer(action.2.unwrap(), import());
    settle(&rig.venus);
    let fix = rig.last().unwrap().fixes.remove(0);
    rig.controller.replace_range(0..0, "// typed\n");
    assert!(!fix.run());
    assert!(
        !rig.controller
            .value()
            .text()
            .contains("use std::rc")
    );
}

#[test]
fn asking_again_or_cancelling_drops_the_answers_that_were_not_wanted() {
    let rig = rig(true);
    rig.ask(23..25);
    let told = requests_of(&rig);
    let action = told
        .iter()
        .find(|(method, ..)| method == "textDocument/codeAction")
        .unwrap();
    rig.source.cancel();
    let cancelled = requests_of(&rig);
    assert!(
        cancelled
            .iter()
            .any(|(method, params, _)| method == "$/cancelRequest"
                && params["id"] == json!(action.2.unwrap())),
        "{cancelled:?}"
    );
    let before = rig.shown.borrow().len();
    rig.fake.answer(action.2.unwrap(), import());
    settle(&rig.venus);
    assert_eq!(rig.shown.borrow().len(), before, "not shown");
}
