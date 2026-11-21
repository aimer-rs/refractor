use std::cell::Cell;
use std::path::{Path, PathBuf};

use aimer::input::TextEditingController;
use serde_json::{Value, json};

use super::*;
use crate::lsp::Severity;
use crate::lsp::fake::{Fake, installed, lsp_with, settle};

const FILE: &str = "/project/src/main.rs";
const TEXT: &str = "fn main() {\n    let x = 1;\n    let é = 2;\n}\n";

const COLORS: LineColors = LineColors {
    error: Color::Rgb(240, 84, 84),
    warning: Color::Rgb(240, 190, 70),
    weak: Color::Rgb(140, 150, 175),
};

fn found(range: Range<usize>, severity: Severity) -> Diagnostic {
    Diagnostic { range, severity, message: String::new(), code: None }
}

fn marks(diagnostics: &[Diagnostic]) -> Vec<(Range<usize>, Weight)> {
    marks_of(diagnostics, &TextSnapshot::from_text(TEXT)).into_iter().map(|mark| (mark.range, mark.weight)).collect()
}

#[test]
fn what_is_less_wrong_is_drawn_first_so_that_the_worse_is_over_it() {
    let order = marks(&[
        found(16..17, Severity::Error),
        found(16..17, Severity::Hint),
        found(16..17, Severity::Warning),
        found(16..17, Severity::Information),
    ]);
    assert_eq!(
        order.iter().map(|(_, weight)| *weight).collect::<Vec<_>>(),
        [Weight::Weak, Weight::Weak, Weight::Warning, Weight::Error]
    );
}

#[test]
fn what_weighs_the_same_is_in_the_order_of_the_text() {
    let order = marks(&[found(20..21, Severity::Warning), found(4..5, Severity::Warning), found(16..19, Severity::Warning)]);
    assert_eq!(order.iter().map(|(range, _)| range.start).collect::<Vec<_>>(), [4, 16, 20]);
}

#[test]
fn a_problem_that_is_at_a_point_has_the_character_there_to_be_under() {
    // Before the `x` of the second line, which starts at byte 12.
    assert_eq!(marks(&[found(20..20, Severity::Error)])[0].0, 20..21);
    // At the end of a line, where the character before it is taken: the `;` is at byte 25, and the line break after it.
    assert_eq!(marks(&[found(26..26, Severity::Error)])[0].0, 25..26);
    // A character of more than one byte is under whole: `é` of the third line is two bytes from 35.
    assert_eq!(marks(&[found(35..35, Severity::Error)])[0].0, 35..37);
    assert_eq!(marks(&[found(37..37, Severity::Error)])[0].0, 37..38, "after it there is a space, which is the character at the point");
    // And the one before the point is taken whole where it is at the end of a line.
    let accent = TextSnapshot::from_text("let é\nx");
    let under_accent = marks_of(&[found(6..6, Severity::Error)], &accent);
    assert_eq!(under_accent[0].range, 4..6, "`é` is two bytes");
    // After the last line break there is an empty line, and nothing on it to be under.
    assert!(marks(&[found(TEXT.len()..TEXT.len(), Severity::Error)]).is_empty());
}

#[test]
fn what_is_past_the_end_is_cut_and_what_is_empty_with_no_character_to_be_under_is_left_out() {
    assert_eq!(marks(&[found(40..900, Severity::Error)])[0].0, 40..TEXT.len().min(900));
    let empty = TextSnapshot::from_text("");
    assert!(marks_of(&[found(0..0, Severity::Error)], &empty).is_empty(), "no text, nothing to be under");
    assert!(marks_of(&[found(5..9, Severity::Error)], &empty).is_empty());
    let line_break = TextSnapshot::from_text("\n");
    assert!(marks_of(&[found(0..0, Severity::Error)], &line_break).is_empty(), "a line break is not a character to be under");
}

#[test]
fn a_file_has_at_most_so_many_lines_and_they_are_the_worst() {
    let many: Vec<Diagnostic> = (0..ProblemLines::MOST + 20)
        .map(|index| found(0..1, if index < 5 { Severity::Error } else { Severity::Hint }))
        .collect();
    let drawn = marks(&many);
    assert_eq!(drawn.len(), ProblemLines::MOST);
    assert_eq!(drawn.iter().filter(|(_, weight)| *weight == Weight::Error).count(), 5, "none of the errors was cut");
}

/// A server that is ready, with the file open, and a cell that is attached to the file.
fn ready() -> (Rc<aimer::Venus>, Fake, Rc<ProblemLines>, TextEditingController) {
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
    let lines = Rc::new(ProblemLines::default());
    lines.attach(lsp, PathBuf::from(FILE), controller.clone(), COLORS);
    (venus, fake, lines, controller)
}

fn publish(fake: &Fake, version: i32, diagnostics: Value) {
    fake.notify("textDocument/publishDiagnostics", json!({"uri": "file:///project/src/main.rs", "version": version, "diagnostics": diagnostics}));
}

fn reported(line: u32, from: u32, to: u32, severity: u32) -> Value {
    json!({"range": {"start": {"line": line, "character": from}, "end": {"line": line, "character": to}}, "severity": severity, "message": "m"})
}

#[test]
fn nothing_is_drawn_until_the_server_has_said_something() {
    let (_venus, _fake, lines, _controller) = ready();
    assert!(lines.underlines().is_none());
}

#[test]
fn the_problems_are_drawn_in_the_colors_of_their_weight() {
    let (venus, fake, lines, _controller) = ready();
    publish(&fake, 1, json!([reported(1, 4, 7, 2), reported(1, 8, 9, 1), reported(2, 4, 7, 3)]));
    settle(&venus);
    lines.refresh();
    let set = lines.underlines().expect("the server said");
    let drawn: Vec<_> = set.items().iter().map(|line| (line.range(), line.color())).collect();
    assert_eq!(
        drawn,
        vec![(31..34, COLORS.weak), (16..19, COLORS.warning), (20..21, COLORS.error)],
        "the weakest first, and the error last, which is over the others"
    );
    assert!(set.text().is_some(), "made for the text the server has, which the area moves over what is typed since");
}

#[test]
fn the_pane_is_drawn_again_only_for_problems_that_differ_from_the_ones_it_has() {
    let (venus, fake, lines, _controller) = ready();
    let drawn = Rc::new(Cell::new(0));
    let counted = Rc::clone(&drawn);
    lines.on_change(Rc::new(move || {
        counted.set(counted.get() + 1);
        true
    }));
    publish(&fake, 1, json!([reported(1, 8, 9, 1)]));
    settle(&venus);
    lines.refresh();
    assert_eq!(drawn.get(), 1);
    let first = lines.underlines().unwrap();
    lines.refresh();
    assert_eq!(drawn.get(), 1, "the same");
    assert!(lines.underlines().unwrap().same_as(&first), "and the same lines, which the area does not draw again");
    publish(&fake, 1, json!([reported(1, 8, 9, 1), reported(1, 4, 7, 2)]));
    settle(&venus);
    lines.refresh();
    assert_eq!(drawn.get(), 2);
    publish(&fake, 1, json!([]));
    settle(&venus);
    lines.refresh();
    assert_eq!(drawn.get(), 3, "none, where there were some");
    assert_eq!(lines.underlines().map(|set| set.is_empty()), Some(true), "and nothing is drawn");
}

#[test]
fn the_lines_that_were_drawn_stay_while_the_server_is_told_of_an_edit_and_has_not_said_what_is_wrong_with_it() {
    let (venus, fake, lines, controller) = ready();
    publish(&fake, 1, json!([reported(1, 8, 9, 1)]));
    settle(&venus);
    lines.refresh();
    assert_eq!(lines.underlines().unwrap().len(), 1);
    // An edit that the server is told of: what it said is about the version before, and it is not given; what was drawn stays.
    controller.set_text(&format!("// c\n{TEXT}"));
    settle(&venus);
    lines.refresh();
    assert_eq!(lines.underlines().map(|set| set.len()), Some(1), "kept, to be moved over the edit by the area");
    // The server says what is wrong with the new text.
    publish(&fake, 2, json!([reported(2, 8, 9, 1)]));
    settle(&venus);
    lines.refresh();
    assert_eq!(lines.underlines().unwrap().items()[0].range(), 25..26);
}

#[test]
fn a_new_color_makes_the_lines_again() {
    let (venus, fake, lines, controller) = ready();
    publish(&fake, 1, json!([reported(1, 8, 9, 1)]));
    settle(&venus);
    lines.refresh();
    let before = lines.underlines().unwrap();
    let lsp = lines.context.borrow().as_ref().unwrap().lsp.clone();
    let other = LineColors { error: Color::Rgb(1, 2, 3), ..COLORS };
    lines.attach(lsp, PathBuf::from(FILE), controller, other);
    let after = lines.underlines().unwrap();
    assert!(!after.same_as(&before));
    assert_eq!(after.items()[0].color(), other.error);
}

fn reported_with(line: u32, from: u32, to: u32, severity: u32, message: &str, code: &str) -> Value {
    json!({"range": {"start": {"line": line, "character": from}, "end": {"line": line, "character": to}}, "severity": severity, "message": message, "code": code})
}

#[test]
fn the_problems_at_a_byte_are_those_whose_range_has_it_worst_first_with_what_the_server_said_of_them() {
    let (venus, fake, lines, _controller) = ready();
    publish(
        &fake,
        1,
        json!([
            reported_with(1, 4, 7, 2, "a warning", "W1"),
            reported_with(1, 4, 11, 1, "an error", "E0412"),
            reported_with(1, 8, 9, 3, "a hint", "H"),
            reported_with(2, 4, 7, 2, "elsewhere", "W2"),
        ]),
    );
    settle(&venus);
    lines.refresh();
    // The `l` of `let` of the second line: byte 16.
    let found = lines.at(16);
    assert_eq!(found.iter().map(|found| found.message.as_str()).collect::<Vec<_>>(), ["an error", "a warning"], "the error first");
    assert_eq!(found[0].weight, Weight::Error);
    assert_eq!(found[0].code.as_deref(), Some("E0412"));
    assert_eq!(found[0].range, 16..23);
    assert!(lines.at(19).iter().all(|found| found.message != "a warning"), "the warning is over `let`, and this is the space after it");
    assert_eq!(lines.at(20).iter().map(|found| found.message.as_str()).collect::<Vec<_>>(), ["an error", "a hint"]);
    assert!(lines.at(23).is_empty(), "past the end of the last range, which is not in it");
    assert!(lines.at(2).is_empty(), "nothing is there");
}

/// A cell that has been told of a problem over `let` of the second line, bytes 16 to 19, and the controller of its text.
fn with_let_problem() -> (Rc<ProblemLines>, TextEditingController) {
    let (venus, fake, lines, controller) = ready();
    publish(&fake, 1, json!([reported_with(1, 4, 7, 1, "unused", "E1")]));
    settle(&venus);
    lines.refresh();
    (lines, controller)
}

#[test]
fn the_problems_at_a_byte_are_looked_for_in_the_text_as_it_is_now_where_text_was_put_in_before_them() {
    let (lines, controller) = with_let_problem();
    assert_eq!(lines.at(17).len(), 1);
    controller.replace_range(0..0, "// c\n");
    assert!(lines.at(17).is_empty(), "it is not here any more");
    assert_eq!(lines.at(22)[0].range, 21..24, "it is where its text went");
}

#[test]
fn a_problem_is_longer_when_text_is_typed_in_the_middle_of_what_it_is_under() {
    let (lines, controller) = with_let_problem();
    controller.replace_range(17..17, "yy");
    assert_eq!(lines.at(17)[0].range, 16..21);
}

#[test]
fn a_problem_is_gone_when_what_it_is_under_is_only_partly_taken_away() {
    let (lines, controller) = with_let_problem();
    controller.replace_range(18..30, "");
    assert!(lines.at(16).is_empty());
}

#[test]
fn a_cell_with_nothing_said_has_no_problems_at_a_byte() {
    let lines = Rc::new(ProblemLines::default());
    assert!(lines.at(0).is_empty());
    let (_venus, _fake, ready, _controller) = ready();
    assert!(ready.at(0).is_empty(), "attached, and the server has said nothing");
}

#[test]
fn a_cell_that_has_no_pane_has_nothing() {
    let lines = Rc::new(ProblemLines::default());
    lines.refresh();
    assert!(lines.underlines().is_none());
}
