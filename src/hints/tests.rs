use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use aimer::Color;
use aimer::input::TextEditingController;
use serde_json::{Value, json};

use super::*;
use crate::lsp::fake::{Fake, installed, lsp_with, settle};

const FILE: &str = "/project/src/main.rs";
const TEXT: &str = "fn main() {\n    let x = vec![1u32];\n    take(x);\n}\n";
const COLORS: HintColors = HintColors { text: Color::Rgb(238, 190, 212), pill: Color::Rgb(90, 50, 70) };

/// A server that has hints to give and is ready, with the file open, and a cell that is attached to the file.
fn ready() -> (Rc<aimer::Venus>, Fake, Rc<Hints>, TextEditingController) {
    let venus = installed();
    let fake = Fake::new();
    let lsp = lsp_with(&fake);
    let controller = TextEditingController::with_text(TEXT);
    lsp.open(Path::new(FILE), &controller, Rc::new(|| {}));
    settle(&venus);
    let _ = fake.requests();
    fake.answer(1, json!({"capabilities": {"textDocumentSync": 2, "positionEncoding": "utf-8", "inlayHintProvider": true}}));
    settle(&venus);
    let _ = fake.requests();
    let hints = Rc::new(Hints::default());
    hints.attach(lsp, PathBuf::from(FILE), controller.clone(), COLORS);
    (venus, fake, hints, controller)
}

fn type_hint(line: u32, character: u32, label: &str) -> Value {
    json!({"position": {"line": line, "character": character}, "label": label, "kind": 1})
}

fn parameter_hint(line: u32, character: u32, label: &str) -> Value {
    json!({"position": {"line": line, "character": character}, "label": label, "kind": 2, "paddingRight": true})
}

#[test]
fn nothing_is_drawn_until_the_server_has_said_something() {
    let (_venus, _fake, hints, _controller) = ready();
    assert!(hints.inlays().is_none());
}

#[test]
fn the_whole_file_is_asked_for_and_what_comes_is_drawn_in_the_colors_of_the_pane() {
    let (venus, fake, hints, _controller) = ready();
    hints.refresh();
    let told = fake.requests();
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].0, "textDocument/inlayHint");
    // Four lines of text, and the end of the text is the start of the empty one after the last line break: not past it.
    assert_eq!(told[0].1["range"], json!({"start": {"line": 0, "character": 0}, "end": {"line": 4, "character": 0}}));
    fake.answer(told[0].2.unwrap(), json!([type_hint(1, 9, ": Vec<u32>"), parameter_hint(2, 9, "value:")]));
    settle(&venus);

    let inlays = hints.inlays().expect("the server said");
    let items = inlays.items();
    assert_eq!(items.len(), 2);
    // The second line starts at byte 12 and the third at 36.
    assert_eq!((items[0].offset(), items[0].label()), (21, ": Vec<u32>"));
    assert_eq!((items[1].offset(), items[1].label()), (45, "value:"));
    for item in items {
        assert_eq!(item.color(), COLORS.text);
        assert_eq!(item.pill(), Some(COLORS.pill));
    }
    assert!(items[0].pad_left() && !items[0].pad_right(), "a type stands apart from its name, though the server did not say so");
    assert!(!items[1].pad_left() && items[1].pad_right(), "a parameter is apart from its argument");
}

#[test]
fn the_same_notes_are_given_while_nothing_changes_and_new_ones_when_the_colors_do() {
    let (venus, fake, hints, controller) = ready();
    hints.refresh();
    let id = fake.requests()[0].2.unwrap();
    fake.answer(id, json!([type_hint(1, 9, ": i32")]));
    settle(&venus);
    let first = hints.inlays().unwrap();
    // Not made again: the area is not to lay out its text again for notes it already has.
    assert!(hints.inlays().unwrap().same_as(&first));
    let lsp = hints.context.borrow().as_ref().unwrap().lsp.clone();
    hints.attach(lsp.clone(), PathBuf::from(FILE), controller.clone(), COLORS);
    assert!(hints.inlays().unwrap().same_as(&first), "the same colors");
    let other = HintColors { text: Color::Rgb(1, 2, 3), ..COLORS };
    hints.attach(lsp, PathBuf::from(FILE), controller, other);
    let again = hints.inlays().unwrap();
    assert!(!again.same_as(&first));
    assert_eq!(again.items()[0].color(), other.text);
}

#[test]
fn what_was_said_is_kept_when_the_server_cannot_say_it_again_and_the_pane_is_drawn_when_it_can() {
    let (venus, fake, hints, _controller) = ready();
    let drawn = Rc::new(Cell::new(0));
    let counted = Rc::clone(&drawn);
    hints.on_change(Rc::new(move || {
        counted.set(counted.get() + 1);
        true
    }));
    hints.refresh();
    let id = fake.requests()[0].2.unwrap();
    fake.answer(id, json!([type_hint(1, 9, ": i32")]));
    settle(&venus);
    assert_eq!(drawn.get(), 1);
    hints.refresh();
    let id = fake.requests()[0].2.unwrap();
    fake.fail(id);
    settle(&venus);
    assert_eq!(drawn.get(), 1, "nothing new to draw");
    assert_eq!(hints.inlays().unwrap().items().len(), 1, "what there was is kept");
}

#[test]
fn the_pane_is_drawn_again_only_for_hints_that_differ_from_the_ones_it_has() {
    // The server is asked again while it loads, and says the same: drawing the whole pane again for it costs a layout of the text and
    // puts what the pane holds, such as the card of a hover, through a rebuild for nothing.
    let (venus, fake, hints, _controller) = ready();
    let drawn = Rc::new(Cell::new(0));
    let counted = Rc::clone(&drawn);
    hints.on_change(Rc::new(move || {
        counted.set(counted.get() + 1);
        true
    }));
    let ask = |answer: Value| {
        hints.refresh();
        let id = fake.requests()[0].2.unwrap();
        fake.answer(id, answer);
        settle(&venus);
    };
    ask(json!([type_hint(1, 9, ": i32")]));
    assert_eq!(drawn.get(), 1);
    let first = hints.inlays().unwrap();
    ask(json!([type_hint(1, 9, ": i32")]));
    assert_eq!(drawn.get(), 1, "the same");
    assert!(hints.inlays().unwrap().same_as(&first), "and the notes are the same ones, which the area does not lay its text out for again");
    ask(json!([type_hint(1, 9, ": i32"), type_hint(2, 9, ": u8")]));
    assert_eq!(drawn.get(), 2, "one more");
    ask(json!([type_hint(1, 9, ": i64"), type_hint(2, 9, ": u8")]));
    assert_eq!(drawn.get(), 3, "a label that is another");
    ask(json!([]));
    assert_eq!(drawn.get(), 4, "none, where there were some");
    ask(json!([]));
    assert_eq!(drawn.get(), 4, "none again");
}

#[test]
fn a_request_that_is_made_again_makes_the_one_before_unwanted() {
    let (venus, fake, hints, _controller) = ready();
    hints.refresh();
    let first = fake.requests()[0].2.unwrap();
    hints.refresh();
    let told = fake.requests();
    assert_eq!(told[0].0, "$/cancelRequest", "the server is told that the first is not wanted");
    assert_eq!(told[0].1, json!({"id": first}));
    fake.answer(first, json!([type_hint(1, 9, ": stale")]));
    settle(&venus);
    assert!(hints.inlays().is_none(), "and its answer is not taken");
}

#[test]
fn a_file_has_at_most_so_many_hints_drawn_and_they_are_the_first() {
    let (venus, fake, hints, _controller) = ready();
    hints.refresh();
    let id = fake.requests()[0].2.unwrap();
    let many: Vec<Value> = (0..Hints::MOST as u32 + 50).map(|n| type_hint(1, 9, &format!(": T{n}"))).collect();
    fake.answer(id, Value::Array(many));
    settle(&venus);
    let inlays = hints.inlays().unwrap();
    assert_eq!(inlays.items().len(), Hints::MOST);
    assert_eq!(inlays.items()[0].label(), ": T0");
}

#[test]
fn the_notes_are_made_for_the_text_they_were_asked_about_so_that_the_area_moves_them_with_the_edits() {
    let (venus, fake, hints, controller) = ready();
    hints.refresh();
    let id = fake.requests()[0].2.unwrap();
    // The text changes while the server works.
    controller.replace_range(0..0, "// new\n");
    fake.answer(id, json!([type_hint(1, 9, ": i32")]));
    settle(&venus);
    let inlays = hints.inlays().unwrap();
    assert_eq!(inlays.items()[0].offset(), 21, "offsets are those of the text that was asked about");
    assert_eq!(inlays.text().map(|text| text.len()), Some(TEXT.len()), "and the notes say which text that is");
}

#[test]
fn the_retries_go_on_while_the_server_is_not_done_or_could_not_say_and_stop_at_last() {
    assert_eq!(Hints::retry_delay(true, true, 0), Some(Hints::RETRY_DELAY), "it could not say");
    assert_eq!(Hints::retry_delay(false, false, 5), Some(Hints::RETRY_DELAY), "it is still loading the project");
    assert_eq!(Hints::retry_delay(false, true, 0), None, "it said, and is done");
    assert_eq!(Hints::retry_delay(true, false, Hints::MOST_RETRIES), None, "not for ever");
}

#[test]
fn a_cell_that_has_no_pane_asks_for_nothing() {
    let hints = Rc::new(Hints::default());
    hints.refresh();
    assert!(hints.inlays().is_none());
    assert!(!hints.server_idle());
}
