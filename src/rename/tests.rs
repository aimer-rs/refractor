use std::cell::Cell;

use aimer::input::input::FieldSpot;

use super::*;

fn spot() -> FieldSpot {
    FieldSpot {
        x: 10.0,
        y: 20.0,
        width: 0.0,
        height: 0.0,
        extent: Some((800.0, 600.0)),
    }
}

#[test]
fn a_name_is_judged_by_what_was_typed_and_what_it_was() {
    assert_eq!(judge("State", "Fresh"), Verdict::Name("Fresh".to_owned()));
    assert_eq!(
        judge("State", "  Fresh  "),
        Verdict::Name("Fresh".to_owned()),
        "the blanks round it are not part of it"
    );
    assert_eq!(judge("State", "State"), Verdict::Same, "nothing to do");
    assert_eq!(judge("State", " State "), Verdict::Same);
    assert_eq!(judge("State", ""), Verdict::Invalid(RenamePrompt::EMPTY));
    assert_eq!(judge("State", "   "), Verdict::Invalid(RenamePrompt::EMPTY));
}

#[test]
fn what_is_not_an_identifier_is_not_a_name() {
    for typed in [
        "1abc",
        "a b",
        "a-b",
        "a.b",
        "a::b",
        "a(",
        "\u{1F600}",
        "a\nb",
        "r#",
        "r#1",
    ] {
        assert_eq!(
            judge("old", typed),
            Verdict::Invalid(RenamePrompt::INVALID),
            "{typed:?}"
        );
    }
    for typed in [
        "_",
        "_x",
        "x1",
        "\u{e9}t\u{e9}",
        "snake_case",
        "CamelCase",
        "r#type",
        "\u{0915}\u{093e}",
    ] {
        assert!(matches!(judge("old", typed), Verdict::Name(_)), "{typed:?}");
    }
}

#[test]
fn a_very_long_name_is_judged_by_the_same_rules() {
    let long = "x".repeat(100_000);
    assert_eq!(judge("old", &long), Verdict::Name(long.clone()));
    assert_eq!(
        judge("old", &format!("{long}-")),
        Verdict::Invalid(RenamePrompt::INVALID)
    );
}

#[test]
fn opening_the_prompt_asks_for_the_name_with_all_of_it_selected() {
    let prompt = RenamePrompt::default();
    assert!(prompt.asking().is_none() && !prompt.is_open());
    prompt.open(12, "State".to_owned(), spot());
    let asking = prompt.asking().expect("open");
    assert_eq!((asking.at, asking.old.as_str()), (12, "State"));
    assert_eq!(
        asking.controller.value().text(),
        "State",
        "it starts as what the name is"
    );
    assert_eq!(
        asking.controller.value().selection().range(),
        0..5,
        "all of it is selected, so that typing replaces it"
    );
    assert_eq!(asking.spot, spot());
    assert!(prompt.is_open());
}

#[test]
fn opening_it_again_for_another_name_asks_for_that_one() {
    let prompt = RenamePrompt::default();
    prompt.open(12, "State".to_owned(), spot());
    prompt.open(40, "Other".to_owned(), spot());
    let asking = prompt.asking().expect("open");
    assert_eq!((asking.at, asking.old.as_str()), (40, "Other"));
}

#[test]
fn closing_it_forgets_the_question_and_the_complaint_and_a_prompt_that_is_not_open_is_not_closed_again()
 {
    let prompt = RenamePrompt::default();
    let drawn = std::rc::Rc::new(Cell::new(0));
    let counted = std::rc::Rc::clone(&drawn);
    prompt.on_change(std::rc::Rc::new(move || {
        counted.set(counted.get() + 1);
        true
    }));
    prompt.close();
    assert_eq!(drawn.get(), 0, "nothing was open: nothing is drawn");
    prompt.open(1, "a".to_owned(), spot());
    assert_eq!(drawn.get(), 1, "drawn when it opens");
    prompt.complain(RenamePrompt::INVALID);
    assert_eq!(prompt.complaint(), Some(RenamePrompt::INVALID));
    assert_eq!(drawn.get(), 2, "and when it says what is wrong");
    prompt.close();
    assert_eq!(drawn.get(), 3, "and when it closes");
    assert!(prompt.asking().is_none() && prompt.complaint().is_none());
}

#[test]
fn a_complaint_goes_when_the_name_is_typed_again() {
    let prompt = RenamePrompt::default();
    prompt.open(1, "a".to_owned(), spot());
    prompt.complain(RenamePrompt::INVALID);
    prompt.typed();
    assert_eq!(prompt.complaint(), None);
}
