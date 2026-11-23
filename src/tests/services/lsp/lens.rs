use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use aimer::input::TextEditingController;
use serde_json::{Value, json};

use super::*;
use crate::services::lsp::fake::{Fake, installed, lsp_with, settle};
use crate::services::lsp::position::Position;

const FILE: &str = "/project/src/main.rs";
const TEXT: &str = "pub struct Rules;\npub fn make() {}\n";

fn place(line: u32) -> Place {
    Place {
        path: PathBuf::from("/project/src/main.rs"),
        at: Position { line, character: 3 },
        end: Position { line, character: 4 },
    }
}

#[test]
fn what_a_count_says_is_in_words_and_a_name_that_nothing_implements_says_nothing_of_it() {
    let lens = |usages: Option<usize>, implementations: Option<usize>| Lens {
        at: 0,
        usages: usages.map(|count| (0..count as u32).map(place).collect()),
        implementations: implementations.map(|count| (0..count as u32).map(place).collect()),
    };
    assert_eq!(
        lens(Some(27), None).usages_caption().as_deref(),
        Some("27 usages")
    );
    assert_eq!(
        lens(Some(1), None).usages_caption().as_deref(),
        Some("1 usage")
    );
    assert_eq!(
        lens(Some(0), None).usages_caption().as_deref(),
        Some("no usages")
    );
    assert_eq!(lens(None, None).usages_caption(), None, "not counted");
    assert_eq!(
        lens(None, Some(1))
            .implementations_caption()
            .as_deref(),
        Some("1 implementation")
    );
    assert_eq!(
        lens(None, Some(3))
            .implementations_caption()
            .as_deref(),
        Some("3 implementations")
    );
    assert_eq!(lens(None, Some(0)).implementations_caption(), None);
    assert_eq!(lens(None, None).implementations_caption(), None);
}

#[test]
fn what_was_counted_for_a_name_is_joined_and_the_names_are_in_the_order_of_the_text() {
    let lenses = group(vec![
        (25, LensKind::Usages, vec![place(1)]),
        (11, LensKind::Implementations, vec![place(2)]),
        (11, LensKind::Usages, vec![place(3), place(4)]),
    ]);
    assert_eq!(
        lenses,
        vec![
            Lens {
                at: 11,
                usages: Some(vec![place(3), place(4)]),
                implementations: Some(vec![place(2)])
            },
            Lens {
                at: 25,
                usages: Some(vec![place(1)]),
                implementations: None
            },
        ]
    );
    assert!(group(Vec::new()).is_empty());
}

fn unresolved(line: u32, character: u32, kind: &str) -> Value {
    json!({"range": {"start": {"line": line, "character": character}, "end": {"line": line, "character": character + 4}},
           "data": {"version": 1, "kind": {kind: {"textDocument": {"uri": "file:///project/src/main.rs"}, "position": {"line": line, "character": character}}}}})
}

fn resolved(line: u32, character: u32, title: &str, places: Vec<Value>) -> Value {
    json!({"range": {"start": {"line": line, "character": character}, "end": {"line": line, "character": character + 4}},
           "command": {"title": title, "command": "rust-analyzer.showReferences",
                       "arguments": ["file:///project/src/main.rs", {"line": line, "character": character}, places]}})
}

fn location(line: u32) -> Value {
    json!({"uri": "file:///project/src/main.rs", "range": {"start": {"line": line, "character": 3}, "end": {"line": line, "character": 4}}})
}

/// A server that is ready, with the file open, and the fake that is its script.
fn ready() -> (Rc<aimer::Venus>, Fake, Lsp) {
    let venus = installed();
    let fake = Fake::new();
    let lsp = lsp_with(&fake);
    lsp.open(
        Path::new(FILE),
        &TextEditingController::with_text(TEXT),
        Rc::new(|| {}),
    );
    settle(&venus);
    let _ = fake.requests();
    fake.answer(1, json!({"capabilities": {"textDocumentSync": 2, "positionEncoding": "utf-8", "codeLensProvider": {"resolveProvider": true}}}));
    settle(&venus);
    let _ = fake.requests();
    (venus, fake, lsp)
}

/// What the loads were given, each time.
type Given = Rc<RefCell<Vec<Option<Vec<Lens>>>>>;

fn load(lsp: &Lsp) -> (LensLoad, Given) {
    let given: Given = Rc::default();
    let into = Rc::clone(&given);
    let load = lsp.lenses(Path::new(FILE), move |lenses| {
        into.borrow_mut().push(lenses)
    });
    (load, given)
}

#[test]
fn the_lenses_of_a_file_are_listed_and_each_resolved_and_given_once_all_are_in() {
    let (venus, fake, lsp) = ready();
    let (_load, given) = load(&lsp);
    let told = fake.requests();
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].0, "textDocument/codeLens");
    assert_eq!(
        told[0].1,
        json!({"textDocument": {"uri": "file:///project/src/main.rs"}})
    );
    let list = json!([
        unresolved(0, 11, "impls"),
        unresolved(0, 11, "references"),
        unresolved(1, 7, "references")
    ]);
    fake.answer(told[0].2.unwrap(), list.clone());
    settle(&venus);
    let resolves = fake.requests();
    assert_eq!(resolves.len(), 3, "each lens is asked for");
    assert!(
        resolves
            .iter()
            .all(|(method, _, _)| method == "codeLens/resolve")
    );
    assert_eq!(resolves[0].1, list[0], "the lens is sent back as it came");
    assert!(given.borrow().is_empty(), "not before the counts are in");

    // Out of order, as a server may.
    fake.answer(
        resolves[2].2.unwrap(),
        resolved(1, 7, "0 references", vec![]),
    );
    fake.answer(
        resolves[0].2.unwrap(),
        resolved(0, 11, "1 implementation", vec![location(1)]),
    );
    settle(&venus);
    assert!(given.borrow().is_empty());
    fake.answer(
        resolves[1].2.unwrap(),
        resolved(0, 11, "2 references", vec![location(2), location(3)]),
    );
    settle(&venus);

    let given = given.borrow();
    assert_eq!(given.len(), 1, "once");
    let lenses = given[0].as_ref().expect("the server said");
    assert_eq!(lenses.len(), 2);
    // The name of the struct is at byte 11 and the function's at 25: the second line begins at byte 18.
    assert_eq!(lenses[0].at, 11);
    assert_eq!(lenses[0].usages_caption().as_deref(), Some("2 usages"));
    assert_eq!(
        lenses[0].implementations_caption().as_deref(),
        Some("1 implementation")
    );
    assert_eq!(
        lenses[0].usages.as_ref().unwrap()[1].at.line,
        3,
        "with where they are"
    );
    assert_eq!(lenses[1].at, 25);
    assert_eq!(lenses[1].usages_caption().as_deref(), Some("no usages"));
    let _ = venus;
}

#[test]
fn only_a_few_lenses_are_asked_for_at_once_and_each_answer_asks_for_the_next() {
    let (venus, fake, lsp) = ready();
    let (_load, given) = load(&lsp);
    let list_id = fake.requests()[0].2.unwrap();
    let list: Vec<Value> = (0..10)
        .map(|line| unresolved(line, 0, "references"))
        .collect();
    fake.answer(list_id, Value::Array(list));
    settle(&venus);
    let first = fake.requests();
    assert_eq!(first.len(), IN_FLIGHT, "not all ten");
    // One comes back: one more goes.
    fake.answer(
        first[0].2.unwrap(),
        resolved(0, 0, "1 reference", vec![location(5)]),
    );
    settle(&venus);
    let next = fake.requests();
    assert_eq!(next.len(), 1);
    assert_eq!(
        next[0].1["range"]["start"]["line"],
        json!(IN_FLIGHT),
        "in the order of the list"
    );
    assert!(given.borrow().is_empty());
    let _ = venus;
}

#[test]
fn a_load_that_is_cancelled_tells_the_server_and_gives_nothing() {
    let (venus, fake, lsp) = ready();
    let (load, given) = load(&lsp);
    let list_id = fake.requests()[0].2.unwrap();
    fake.answer(
        list_id,
        json!([
            unresolved(0, 11, "references"),
            unresolved(1, 7, "references")
        ]),
    );
    settle(&venus);
    let resolves = fake.requests();
    assert_eq!(resolves.len(), 2);
    load.cancel();
    let told = fake.requests();
    assert_eq!(
        told.iter()
            .filter(|(method, _, _)| method == "$/cancelRequest")
            .count(),
        2,
        "each that is waiting"
    );
    fake.answer(
        resolves[0].2.unwrap(),
        resolved(0, 11, "1 reference", vec![location(1)]),
    );
    fake.answer(
        resolves[1].2.unwrap(),
        resolved(1, 7, "1 reference", vec![location(1)]),
    );
    settle(&venus);
    assert!(given.borrow().is_empty(), "nothing is given");
    assert!(fake.requests().is_empty(), "and nothing more is asked");
    let _ = venus;
}

#[test]
fn a_load_that_is_cancelled_before_the_list_comes_asks_for_none() {
    let (venus, fake, lsp) = ready();
    let (load, given) = load(&lsp);
    let list_id = fake.requests()[0].2.unwrap();
    load.cancel();
    assert_eq!(fake.requests()[0].0, "$/cancelRequest");
    fake.answer(list_id, json!([unresolved(0, 11, "references")]));
    settle(&venus);
    assert!(given.borrow().is_empty() && fake.requests().is_empty());
    let _ = venus;
}

#[test]
fn a_list_that_could_not_be_given_is_no_lenses_and_a_lens_that_could_not_be_given_is_left_out() {
    let (venus, fake, lsp) = ready();
    let (_first, failed) = load(&lsp);
    fake.fail(fake.requests()[0].2.unwrap());
    settle(&venus);
    assert_eq!(
        *failed.borrow(),
        vec![None],
        "the server could not say: the caller keeps what it had"
    );

    let (_second, partial) = load(&lsp);
    let list_id = fake.requests()[0].2.unwrap();
    fake.answer(
        list_id,
        json!([
            unresolved(0, 11, "references"),
            unresolved(1, 7, "references"),
            unresolved(1, 7, "impls")
        ]),
    );
    settle(&venus);
    let resolves = fake.requests();
    fake.fail(resolves[0].2.unwrap());
    // A lens that is not one of places is none either.
    fake.answer(
        resolves[1].2.unwrap(),
        json!({"range": {"start": {"line": 1, "character": 7}}, "command": null}),
    );
    fake.answer(
        resolves[2].2.unwrap(),
        resolved(1, 7, "1 implementation", vec![location(0)]),
    );
    settle(&venus);
    let partial = partial.borrow();
    let lenses = partial[0].as_ref().expect("the rest are given");
    assert_eq!(lenses.len(), 1);
    assert_eq!(
        (
            lenses[0].at,
            lenses[0].usages.is_none(),
            lenses[0].implementations.is_some()
        ),
        (25, true, true)
    );
    let _ = venus;
}

#[test]
fn nothing_is_loaded_where_the_file_is_not_open_or_the_server_is_not_ready() {
    let venus = installed();
    let lsp = lsp_with(&Fake::new());
    let given: Given = Rc::default();
    let into = Rc::clone(&given);
    let _ = lsp.lenses(Path::new(FILE), move |lenses| {
        into.borrow_mut().push(lenses)
    });
    assert_eq!(
        *given.borrow(),
        vec![None],
        "a file that is not open: at once, with none"
    );

    let (starting, fake) = (lsp_with(&Fake::new()), Fake::new());
    let _ = fake;
    starting.open(
        Path::new(FILE),
        &TextEditingController::with_text(TEXT),
        Rc::new(|| {}),
    );
    settle(&venus);
    let into = Rc::clone(&given);
    let _ = starting.lenses(Path::new(FILE), move |lenses| {
        into.borrow_mut().push(lenses)
    });
    assert_eq!(
        given.borrow().len(),
        2,
        "a server that is not ready: at once, with none"
    );
    assert_eq!(given.borrow()[1], None);
    let _ = venus;
}

#[test]
fn a_file_with_more_lenses_than_are_wanted_has_the_first_of_them() {
    let (venus, fake, lsp) = ready();
    let (_load, given) = load(&lsp);
    let list_id = fake.requests()[0].2.unwrap();
    let list: Vec<Value> = (0..MOST as u32 + 50)
        .map(|line| unresolved(line, 0, "references"))
        .collect();
    fake.answer(list_id, Value::Array(list));
    settle(&venus);
    // Answer what is asked for, again and again, until nothing more is.
    let mut asked = 0;
    loop {
        let waiting = fake.requests();
        if waiting.is_empty() {
            break;
        }
        for (_, lens, id) in waiting {
            asked += 1;
            let line = lens["range"]["start"]["line"].as_u64().unwrap() as u32;
            fake.answer(
                id.unwrap(),
                resolved(line, 0, "1 reference", vec![location(1)]),
            );
        }
        settle(&venus);
    }
    assert_eq!(
        asked, MOST,
        "{asked} lenses were asked for, and no more than {MOST} are wanted"
    );
    let given = given.borrow();
    // They all lie past the end of the two lines of the text, so what is given is few: it is that it is given once that is asked.
    assert_eq!(given.len(), 1);
    assert!(given[0].is_some());
    let _ = venus;
}
