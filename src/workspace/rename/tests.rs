use std::path::Path;
use std::rc::Rc;

use super::*;
use crate::lsp::Position;
use crate::lsp::WorkspaceEdit as Edits;
use crate::tree::Scratch;

fn notify() -> Rc<dyn Fn()> {
    Rc::new(|| {})
}

fn at(line: u32, character: u32) -> Position {
    Position { line, character }
}

/// The edit that puts `text` over the characters `from` to `to` of `line`.
fn edit(line: u32, from: u32, to: u32, text: &str) -> ((Position, Position), String) {
    ((at(line, from), at(line, to)), text.to_owned())
}

fn rename_of(
    origin: &Path,
    space: &Workspace,
    files: Vec<(&Path, Vec<((Position, Position), String)>)>,
) -> Rename {
    let text = space
        .documents
        .iter()
        .find(|document| document.path == origin)
        .expect("the file it was asked in is open")
        .controller
        .snapshot();
    Rename {
        edit: Edits {
            files: files
                .into_iter()
                .map(|(path, edits)| (path.to_path_buf(), edits))
                .collect(),
        },
        of: (origin.to_path_buf(), text),
    }
}

/// Makes a rename as the app does: planned, made, and reported.
fn apply_rename(space: &mut Workspace, rename: &Rename) -> Result<Renamed, String> {
    let planned = space.plan_rename(rename, notify());
    let done = planned.and_then(|plan| plan.apply());
    space.report_rename(done.clone());
    done
}

fn text_of(space: &Workspace, path: &Path) -> String {
    space
        .documents
        .iter()
        .find(|document| document.path == path)
        .expect("open")
        .controller
        .value()
        .text()
        .to_string()
}

#[test]
fn a_rename_changes_every_place_in_a_file_that_is_open_as_one_edit_that_one_undo_takes_back() {
    let scratch = Scratch::new("rename-one");
    let a = scratch.file("a.rs", "struct State;\nfn f(s: State) -> State { s }\n");
    let mut space = Workspace::new(scratch.0.clone());
    space.open(&a, notify()).unwrap();
    let rename = rename_of(
        &a,
        &space,
        vec![(
            &a,
            vec![
                edit(0, 7, 12, "Fresh"),
                edit(1, 8, 13, "Fresh"),
                edit(1, 18, 23, "Fresh"),
            ],
        )],
    );

    let done = apply_rename(&mut space, &rename).expect("renamed");

    assert_eq!(
        done,
        Renamed {
            places: 3,
            files: 1
        }
    );
    assert_eq!(
        text_of(&space, &a),
        "struct Fresh;\nfn f(s: Fresh) -> Fresh { s }\n"
    );
    let document = space
        .documents
        .iter()
        .find(|document| document.path == a)
        .unwrap();
    assert!(document.controller.undo(), "one undo");
    assert_eq!(
        text_of(&space, &a),
        "struct State;\nfn f(s: State) -> State { s }\n"
    );
    assert!(
        space
            .message
            .as_deref()
            .is_some_and(|message| message.contains("3 places"))
    );
}

#[test]
fn a_file_that_is_not_open_is_opened_for_it_and_the_tab_that_was_shown_is_shown_again() {
    let scratch = Scratch::new("rename-two");
    let a = scratch.file("a.rs", "struct State;\n");
    let b = scratch.file("b.rs", "use crate::State;\nfn g(_: State) {}\n");
    let mut space = Workspace::new(scratch.0.clone());
    space.open(&a, notify()).unwrap();
    let shown = space.active;
    let rename = rename_of(
        &a,
        &space,
        vec![
            (&a, vec![edit(0, 7, 12, "Fresh")]),
            (&b, vec![edit(0, 11, 16, "Fresh"), edit(1, 8, 13, "Fresh")]),
        ],
    );

    let done = apply_rename(&mut space, &rename).expect("renamed");

    assert_eq!(
        done,
        Renamed {
            places: 3,
            files: 2
        }
    );
    assert_eq!(
        text_of(&space, &b),
        "use crate::Fresh;\nfn g(_: Fresh) {}\n"
    );
    assert!(
        space
            .documents
            .iter()
            .find(|document| document.path == b)
            .unwrap()
            .is_dirty(),
        "changed, and not saved"
    );
    assert_eq!(
        std::fs::read_to_string(&b).unwrap(),
        "use crate::State;\nfn g(_: State) {}\n",
        "the file on the disk is as it was"
    );
    assert_eq!(space.active, shown, "the tab that was shown is shown again");
    assert!(
        space
            .message
            .as_deref()
            .is_some_and(|message| message.contains("2 files")),
        "{:?}",
        space.message
    );
}

#[test]
fn the_places_are_the_bytes_the_server_counted_with_accents_before_them() {
    let scratch = Scratch::new("rename-accent");
    // The accent is two bytes and one character: the name starts at the seventh character.
    let a = scratch.file("a.rs", "// caf\u{e9} State\n");
    let mut space = Workspace::new(scratch.0.clone());
    space.open(&a, notify()).unwrap();
    let rename = rename_of(&a, &space, vec![(&a, vec![edit(0, 8, 13, "Fresh")])]);
    apply_rename(&mut space, &rename).expect("renamed");
    assert_eq!(text_of(&space, &a), "// caf\u{e9} Fresh\n");
}

#[test]
fn a_rename_is_refused_when_the_text_it_was_asked_in_changed_meanwhile_and_nothing_is_changed() {
    let scratch = Scratch::new("rename-stale");
    let a = scratch.file("a.rs", "struct State;\n");
    let b = scratch.file("b.rs", "use State;\n");
    let mut space = Workspace::new(scratch.0.clone());
    space.open(&a, notify()).unwrap();
    let rename = rename_of(
        &a,
        &space,
        vec![
            (&a, vec![edit(0, 7, 12, "Fresh")]),
            (&b, vec![edit(0, 4, 9, "Fresh")]),
        ],
    );
    // Typed while the server thought.
    space.documents[0]
        .controller
        .replace_range(0..0, "// x\n");

    let error = apply_rename(&mut space, &rename).expect_err("refused");

    assert!(error.contains("changed"), "{error}");
    assert_eq!(text_of(&space, &a), "// x\nstruct State;\n");
    assert!(
        space
            .documents
            .iter()
            .all(|document| document.path != b),
        "the other file was not even opened"
    );
}

#[test]
fn edits_that_overlap_make_no_change_in_any_file() {
    let scratch = Scratch::new("rename-overlap");
    let a = scratch.file("a.rs", "struct State;\n");
    let b = scratch.file("b.rs", "use State;\n");
    let mut space = Workspace::new(scratch.0.clone());
    space.open(&a, notify()).unwrap();
    // The second file's two edits are of the same bytes.
    let rename = rename_of(
        &a,
        &space,
        vec![
            (&a, vec![edit(0, 7, 12, "Fresh")]),
            (&b, vec![edit(0, 4, 9, "Fresh"), edit(0, 6, 9, "Other")]),
        ],
    );

    let error = apply_rename(&mut space, &rename).expect_err("refused");

    assert!(error.contains("overlap"), "{error}");
    assert_eq!(
        text_of(&space, &a),
        "struct State;\n",
        "the first file was not changed either"
    );
    assert_eq!(text_of(&space, &b), "use State;\n");
}

#[test]
fn a_file_that_cannot_be_opened_stops_the_rename_before_any_change() {
    let scratch = Scratch::new("rename-missing");
    let a = scratch.file("a.rs", "struct State;\n");
    let gone = scratch.0.join("gone.rs");
    let mut space = Workspace::new(scratch.0.clone());
    space.open(&a, notify()).unwrap();
    let rename = rename_of(
        &a,
        &space,
        vec![
            (&a, vec![edit(0, 7, 12, "Fresh")]),
            (&gone, vec![edit(0, 0, 1, "x")]),
        ],
    );

    let error = apply_rename(&mut space, &rename).expect_err("refused");

    assert!(error.contains("gone.rs"), "{error}");
    assert_eq!(text_of(&space, &a), "struct State;\n");
}

#[test]
fn overlapping_is_two_ranges_that_share_a_byte_and_touching_is_not() {
    let make = |ranges: &[Range<usize>]| -> Vec<(Range<usize>, String)> {
        ranges
            .iter()
            .map(|range| (range.clone(), String::new()))
            .collect()
    };
    assert!(!overlapping(&make(&[])));
    assert!(!overlapping(&make(&[0..3])));
    assert!(!overlapping(&make(&[0..3, 3..5])), "they touch");
    assert!(
        !overlapping(&make(&[4..4, 4..4])),
        "two insertions at one place do not share a byte"
    );
    assert!(overlapping(&make(&[3..6, 0..4])), "in any order");
    assert!(overlapping(&make(&[0..9, 2..3])));
}

/// What the provider does: the workspace is borrowed for an update, and another update begun inside it is an error.
fn store(space: Workspace) -> Rc<std::cell::RefCell<Workspace>> {
    Rc::new(std::cell::RefCell::new(space))
}

#[test]
fn the_edits_are_made_outside_an_update_of_the_workspace_because_a_changed_document_tells_it_so() {
    let scratch = Scratch::new("rename-nested");
    let a = scratch.file("a.rs", "struct State;\nfn f(_: State) {}\n");
    let b = scratch.file("b.rs", "use State;\n");
    let space = store(Workspace::new(scratch.0.clone()));
    // A document that becomes changed tells the workspace, which is an update of it: this is what the app's notifier does.
    let told = Rc::clone(&space);
    let notify: Rc<dyn Fn()> = Rc::new(move || told.borrow_mut().touch());
    space
        .borrow_mut()
        .open(&a, Rc::clone(&notify))
        .unwrap();
    let rename = rename_of(
        &a,
        &space.borrow(),
        vec![
            (&a, vec![edit(0, 7, 12, "Fresh"), edit(1, 8, 13, "Fresh")]),
            (&b, vec![edit(0, 4, 9, "Fresh")]),
        ],
    );

    // The plan is made inside an update, as the app makes it.
    let plan = space
        .borrow_mut()
        .plan_rename(&rename, Rc::clone(&notify))
        .expect("planned");
    assert_eq!(
        text_of(&space.borrow(), &a),
        "struct State;\nfn f(_: State) {}\n",
        "nothing is changed by planning"
    );

    // And made outside one: here it would panic, where the app does.
    let done = plan.apply().expect("renamed");
    space.borrow_mut().report_rename(Ok(done));

    assert_eq!(
        done,
        Renamed {
            places: 3,
            files: 2
        }
    );
    assert_eq!(
        text_of(&space.borrow(), &a),
        "struct Fresh;\nfn f(_: Fresh) {}\n"
    );
    assert_eq!(text_of(&space.borrow(), &b), "use Fresh;\n");
    assert!(
        space
            .borrow()
            .message
            .as_deref()
            .is_some_and(|message| message.contains("3 places")),
        "{:?}",
        space.borrow().message
    );
}

#[test]
#[should_panic(expected = "already borrowed")]
fn making_the_edits_inside_an_update_is_the_error_that_ended_the_app() {
    let scratch = Scratch::new("rename-nested-bug");
    let a = scratch.file("a.rs", "struct State;\n");
    let space = store(Workspace::new(scratch.0.clone()));
    let told = Rc::clone(&space);
    let notify: Rc<dyn Fn()> = Rc::new(move || told.borrow_mut().touch());
    space
        .borrow_mut()
        .open(&a, Rc::clone(&notify))
        .unwrap();
    let rename = rename_of(
        &a,
        &space.borrow(),
        vec![(&a, vec![edit(0, 7, 12, "Fresh")])],
    );
    // What the app did before: the edits made while the workspace is borrowed for the update.
    let mut inside = space.borrow_mut();
    let plan = inside
        .plan_rename(&rename, Rc::clone(&notify))
        .expect("planned");
    let _ = plan.apply();
}
