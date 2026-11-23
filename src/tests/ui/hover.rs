use std::cell::{Cell, RefCell};

use super::*;
use crate::services::lsp::Weight;

#[test]
fn the_code_blocks_that_follow_each_other_are_one_block_with_a_blank_line_between_their_parts() {
    // As rust-analyzer says what a name is: where it is, and then what it is, in two fences.
    let said = "```rust\naimer_quiver::aimer_app\n```\n\n```rust\npub struct AimerApp\n```\n\n---\n\nDocs.";
    assert_eq!(
        Card::merged(said),
        "```rust\naimer_quiver::aimer_app\n\npub struct AimerApp\n```\n\n---\n\nDocs."
    );
    let three = "```rust\na\n```\n```rust\nb\n```\n\n\n```rust\nc\n```";
    assert_eq!(
        Card::merged(three),
        "```rust\na\n\nb\n\nc\n```",
        "any number, however many blank lines are between"
    );
}

#[test]
fn blocks_that_have_words_between_them_or_are_in_other_languages_or_are_not_closed_stay_as_they_are()
 {
    let apart = "```rust\na\n```\n\nwords\n\n```rust\nb\n```";
    assert_eq!(Card::merged(apart), apart);
    let others = "```rust\na\n```\n\n```toml\nb = 1\n```";
    assert_eq!(
        Card::merged(others),
        others,
        "another language is another block"
    );
    let open = "```rust\na\n```\n\n```rust\nb";
    assert_eq!(
        Card::merged(open),
        open,
        "a fence that is never closed is left alone"
    );
    assert_eq!(Card::merged("just words\nmore"), "just words\nmore");
    assert_eq!(Card::merged(""), "");
}

#[test]
fn a_block_whose_language_is_not_said_joins_one_that_is_and_a_longer_or_tilde_fence_is_a_fence() {
    assert_eq!(
        Card::merged("```\na\n```\n\n```\nb\n```"),
        "```\na\n\nb\n```"
    );
    assert_eq!(
        Card::merged("~~~rust\na\n~~~\n\n~~~rust\nb\n~~~"),
        "~~~rust\na\n\nb\n~~~"
    );
    // A shorter run of the mark inside a longer fence is code, and does not close it.
    assert_eq!(
        Card::merged("````rust\n```\n````\n\n````rust\nb\n````"),
        "````rust\n```\n\nb\n````"
    );
}

fn spot(start: usize) -> HoverSpot {
    HoverSpot {
        range: start..start + 4,
        x: 10.0,
        y: 20.0,
        width: 30.0,
        height: 16.0,
        extent: Some((400.0, 300.0)),
    }
}

fn card(words: &str) -> Card {
    Card::docs(spot(0), words.to_owned())
}

fn counted(cell: &HoverCell) -> Rc<Cell<usize>> {
    let drawn = Rc::new(Cell::new(0));
    let counted = Rc::clone(&drawn);
    cell.on_change(Rc::new(move || {
        counted.set(counted.get() + 1);
        true
    }));
    drawn
}

#[test]
fn a_card_that_is_on_screen_is_still_drawn_when_one_that_was_made_after_it_is_gone() {
    // The pane is built again, and with it the widget of the card: the one that was made last may be the one the framework drops,
    // and the one that is on screen has to be told all the same.
    let cell = HoverCell::default();
    let drawn = counted(&cell);
    cell.on_change(Rc::new(|| false));
    cell.show(card("one"));
    assert_eq!(drawn.get(), 1, "the one on screen was told");
    cell.show(card("two"));
    assert_eq!(
        drawn.get(),
        2,
        "and again, the one that is gone having been forgotten"
    );
    cell.hide();
    assert_eq!(drawn.get(), 3);
}

#[test]
fn the_card_is_shown_and_taken_away_and_what_draws_it_is_told_only_when_it_changes() {
    let cell = HoverCell::default();
    let drawn = counted(&cell);
    assert!(!cell.is_open());
    cell.hide();
    assert_eq!(drawn.get(), 0, "nothing to take away");
    cell.show(card("one"));
    cell.show(card("one"));
    assert_eq!(
        (cell.is_open(), drawn.get()),
        (true, 1),
        "the same card twice is one change"
    );
    cell.show(card("two"));
    assert_eq!(
        (
            cell.with(|card| card.map(|card| card.markdown.clone())),
            drawn.get()
        ),
        (Some("two".to_owned()), 2)
    );
    cell.hide();
    assert_eq!((cell.is_open(), drawn.get()), (false, 3));
}

#[test]
fn a_card_goes_when_the_pointer_leaves_the_name_and_is_not_on_the_card() {
    // With no runtime to wait on, the moment that lets the pointer get to the card is no moment at all.
    let cell = Rc::new(HoverCell::default());
    cell.show(card("one"));
    cell.leave();
    assert!(!cell.is_open());
    cell.leave();
    assert!(!cell.is_open(), "nothing to take away");
}

#[test]
fn a_card_the_pointer_is_on_stays_and_goes_when_the_pointer_leaves_it() {
    let cell = Rc::new(HoverCell::default());
    cell.show(card("one"));
    cell.pointer_over(true);
    cell.leave();
    assert!(cell.is_open(), "the pointer is on it");
    cell.pointer_over(false);
    assert!(
        !cell.is_open(),
        "and off it, it goes as it does when the pointer leaves the name"
    );
}

fn problem_card(range: std::ops::Range<usize>, fixes: Vec<Fix>) -> Card {
    Card {
        spot: HoverSpot { range, ..spot(0) },
        markdown: String::new(),
        problems: vec![Problem {
            weight: Weight::Error,
            message: "m".to_owned(),
            code: None,
        }],
        fixes,
    }
}

#[test]
fn the_list_of_fixes_is_opened_by_asking_and_is_shut_when_the_card_is_for_another_place_or_goes() {
    let cell = Rc::new(HoverCell::default());
    let drawn = counted(&cell);
    cell.toggle_expanded();
    assert!(cell.is_expanded());
    assert_eq!(drawn.get(), 0, "no card, nothing to draw again");
    cell.toggle_expanded();
    cell.show(problem_card(
        0..3,
        vec![Fix::new("a", true, || true), Fix::new("b", false, || true)],
    ));
    assert_eq!(drawn.get(), 1);
    cell.toggle_expanded();
    assert!(cell.is_expanded());
    assert_eq!(drawn.get(), 2, "the card is drawn again with the list");
    // The same place, said again with more: the list stays open.
    cell.show(problem_card(
        0..3,
        vec![
            Fix::new("a", true, || true),
            Fix::new("b", false, || true),
            Fix::new("c", false, || true),
        ],
    ));
    assert!(cell.is_expanded());
    // Another place: it is another card.
    cell.show(problem_card(10..13, vec![Fix::new("a", true, || true)]));
    assert!(!cell.is_expanded());
    cell.toggle_expanded();
    cell.hide();
    assert!(!cell.is_expanded(), "and it is shut when the card goes");
}

#[test]
fn the_fix_for_the_key_is_the_first_of_the_card_and_there_is_none_without_a_card_or_without_fixes()
{
    let cell = Rc::new(HoverCell::default());
    assert!(cell.first_fix().is_none() && !cell.has_more_fixes());
    cell.show(problem_card(0..3, Vec::new()));
    assert!(cell.first_fix().is_none());
    let done = Rc::new(Cell::new(0));
    let (first, second) = (Rc::clone(&done), Rc::clone(&done));
    cell.show(problem_card(
        0..3,
        vec![
            Fix::new("first", true, move || {
                first.set(first.get() + 1);
                true
            }),
            Fix::new("second", false, move || {
                second.set(second.get() + 10);
                true
            }),
        ],
    ));
    assert!(cell.has_more_fixes());
    let fix = cell.first_fix().expect("the first");
    assert_eq!(fix.title, "first");
    assert!(fix.run());
    assert_eq!(done.get(), 1);
    cell.show(problem_card(0..3, vec![Fix::new("only", true, || true)]));
    assert!(!cell.has_more_fixes(), "one is not more");
}

#[test]
fn two_cards_with_the_same_fixes_are_the_same_card_and_with_other_fixes_are_not() {
    let fix = Fix::new("a", true, || true);
    assert_eq!(
        problem_card(0..3, vec![fix.clone()]),
        problem_card(0..3, vec![fix.clone()])
    );
    assert_ne!(
        problem_card(0..3, vec![fix]),
        problem_card(0..3, vec![Fix::new("a", true, || true)]),
        "a fix is its own"
    );
}

/// What is asked, and what answers.
type Asked = Rc<RefCell<Vec<(usize, Box<dyn FnOnce(Option<Hover>)>)>>>;

fn following() -> (
    Rc<HoverCell>,
    Asked,
    Rc<Cell<usize>>,
    impl Fn(Option<HoverSpot>),
) {
    let cell = Rc::new(HoverCell::default());
    let asked: Asked = Rc::default();
    let cancelled = Rc::new(Cell::new(0));
    let (record, count) = (Rc::clone(&asked), Rc::clone(&cancelled));
    let told = follow(
        Rc::clone(&cell),
        move |at, done| record.borrow_mut().push((at, done)),
        move || count.set(count.get() + 1),
    );
    (cell, asked, cancelled, told)
}

fn hover(words: &str) -> Hover {
    Hover {
        markdown: words.to_owned(),
        range: None,
    }
}

#[test]
fn a_name_is_asked_about_at_its_start_and_the_card_is_shown_when_the_words_come() {
    let (cell, asked, _, told) = following();
    told(Some(spot(7)));
    assert_eq!(asked.borrow().len(), 1);
    assert_eq!(asked.borrow()[0].0, 7, "the start of the name");
    assert!(!cell.is_open(), "not until the server has said");
    let (_, done) = asked.borrow_mut().remove(0);
    done(Some(hover("`f`")));
    let shown = cell.with(|card| card.cloned()).expect("shown");
    assert_eq!(
        (shown.markdown.as_str(), shown.spot.range),
        ("`f`", 7..11),
        "beside the name it is for"
    );
}

#[test]
fn a_name_the_server_has_nothing_to_say_of_has_no_card_and_takes_the_one_before_it_away() {
    let (cell, asked, _, told) = following();
    cell.show(card("the one before"));
    told(Some(spot(7)));
    assert!(cell.is_open(), "kept while the server is asked");
    let (_, done) = asked.borrow_mut().remove(0);
    done(None);
    assert!(!cell.is_open());
}

#[test]
fn the_pointer_leaving_drops_the_question_and_lets_the_card_go() {
    let (cell, asked, cancelled, told) = following();
    told(Some(spot(7)));
    told(None);
    assert_eq!(
        cancelled.get(),
        1,
        "the server is told that the answer is not wanted"
    );
    let _ = asked;
    cell.show(card("shown"));
    told(None);
    assert!(!cell.is_open());
    assert_eq!(cancelled.get(), 2);
}
