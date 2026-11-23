use super::*;
use crate::services::lsp::{Edit, ItemKind};

type Edits = Option<Vec<(Range<usize>, String)>>;

/// A server that keeps what it is asked, for the test to answer.
#[derive(Default)]
struct Fake {
    pending: RefCell<Vec<Box<dyn FnOnce(Option<Answer>)>>>,
    asked: Cell<usize>,
    /// The labels of the items whose rest was asked for, and what is called with the answer.
    resolving: RefCell<Vec<(String, Box<dyn FnOnce(Edits)>)>>,
}

impl Source for Fake {
    fn request(&self, done: Box<dyn FnOnce(Option<Answer>)>) {
        self.asked.set(self.asked.get() + 1);
        self.pending.borrow_mut().push(done);
    }

    fn resolve(&self, item: &Item, done: Box<dyn FnOnce(Edits)>) {
        self.resolving
            .borrow_mut()
            .push((item.label.clone(), done));
    }
}

impl Fake {
    /// Answers the request that was made last.
    fn answer(&self, answer: Option<Answer>) {
        let done = self
            .pending
            .borrow_mut()
            .pop()
            .expect("a request is waiting");
        done(answer);
    }

    /// The labels of the items that were asked for the rest of, in order, which are not asked again.
    fn resolved_labels(&self) -> Vec<String> {
        self.resolving
            .borrow()
            .iter()
            .map(|(label, _)| label.clone())
            .collect()
    }

    /// Answers the request for the rest of an item that was made last.
    fn resolve_last(&self, edits: Edits) {
        let (_, done) = self
            .resolving
            .borrow_mut()
            .pop()
            .expect("an item is waiting to be resolved");
        done(edits);
    }

    fn answer_first(&self, answer: Option<Answer>) {
        let done = self.pending.borrow_mut().remove(0);
        done(answer);
    }
}

fn item(label: &str, start: usize) -> Item {
    Item {
        label: label.to_owned(),
        // A plain word: a function is called, which is its own matter (see `a_function_is_completed_with_its_call`).
        kind: ItemKind::Struct,
        detail: None,
        label_detail: None,
        sort: label.to_owned(),
        filter: label.to_owned(),
        edit: Edit {
            range: Some(start..start),
            text: label.to_owned(),
            extra: vec![],
        },
        resolve: None,
        deprecated: false,
    }
}

fn items_at(start: usize, labels: &[&str]) -> Option<Answer> {
    Some(Answer {
        items: labels
            .iter()
            .map(|label| item(label, start))
            .collect(),
        incomplete: false,
    })
}

/// A completer over `text` with the caret at its end, and the listener the pane makes.
fn typing_in(
    language: Language,
    text: &str,
    source: Option<Rc<dyn Source>>,
) -> (
    Rc<Completer>,
    TextEditingController,
    aimer::input::ControllerListener,
) {
    let controller = TextEditingController::with_text(text);
    let completer = Completer::new(language, controller.clone(), source);
    let listener = {
        let completer = Rc::clone(&completer);
        controller
            .clone()
            .listen(move |_| completer.refresh())
    };
    (completer, controller, listener)
}

fn typing(
    text: &str,
) -> (
    Rc<Completer>,
    TextEditingController,
    aimer::input::ControllerListener,
) {
    typing_in(Language::Rust, text, None)
}

fn served(
    text: &str,
) -> (
    Rc<Completer>,
    TextEditingController,
    aimer::input::ControllerListener,
    Rc<Fake>,
) {
    let fake = Rc::new(Fake::default());
    let (completer, controller, listener) = typing_in(
        Language::Rust,
        text,
        Some(Rc::clone(&fake) as Rc<dyn Source>),
    );
    (completer, controller, listener, fake)
}

fn chosen(completer: &Completer) -> Option<String> {
    completer
        .menu()
        .with(|menu| menu.map(|menu| menu.chosen().label.clone()))
}

/// Every label of the menu, in order, whatever the window shows.
fn all(completer: &Completer) -> Vec<String> {
    completer.menu().with(|menu| {
        let Some(menu) = menu else { return Vec::new() };
        let mut menu = menu.clone();
        let mut found = vec![menu.chosen().label.clone()];
        for _ in 1..menu.len() {
            menu.step(true);
            found.push(menu.chosen().label.clone());
        }
        found
    })
}

// --- Keywords: where there is no server.

#[test]
fn a_word_is_completed_to_the_keywords_it_begins_and_the_shorter_comes_first() {
    let (completer, controller, _listener) = typing("fn main() {\n    ");
    assert_eq!(chosen(&completer), None, "nothing typed yet");
    controller.replace_range(16..16, "st");
    assert_eq!(
        all(&completer),
        ["static", "struct"],
        "and not const, which only has the letters"
    );
    controller.replace_range(18..18, "x");
    assert_eq!(chosen(&completer), None, "no keyword begins stx");

    let (completer, controller, _listener) = typing("");
    controller.replace_range(0..0, "tr");
    assert_eq!(all(&completer), ["try", "true", "trait"]);
    controller.replace_range(2..2, "u");
    assert_eq!(all(&completer), ["true"]);
}

#[test]
fn the_first_letter_of_a_word_is_completed_and_a_complete_word_is_not() {
    let (completer, controller, _listener) = typing("");
    controller.replace_range(0..0, "f");
    assert_eq!(all(&completer), ["fn", "for", "false", "final"]);
    controller.replace_range(0..1, "fn");
    assert_eq!(
        chosen(&completer),
        None,
        "a keyword that is complete and begins no other"
    );
    controller.replace_range(0..2, "as");
    assert_eq!(
        all(&completer),
        ["as", "async"],
        "as is a keyword, and begins async"
    );
    controller.replace_range(0..2, "q");
    assert_eq!(chosen(&completer), None);
    controller.replace_range(0..1, "1st");
    assert_eq!(chosen(&completer), None, "a number is not a word");
}

#[test]
fn a_comment_a_space_and_other_languages_are_not_completed() {
    let (completer, controller, _listener) = typing("let a = 1; // ");
    controller.replace_range(14..14, "pu");
    assert_eq!(chosen(&completer), None, "a comment");
    let (completer, controller, _listener) = typing("let a = 1;\n");
    controller.replace_range(11..11, "pu");
    assert_eq!(
        chosen(&completer),
        Some("pub".to_owned()),
        "the line before is another line"
    );
    controller.replace_range(13..13, " ");
    assert_eq!(chosen(&completer), None, "a space ends the word");

    let controller = TextEditingController::with_text("pu");
    let completer = Completer::new(Language::PlainText, controller.clone(), None);
    controller.replace_range(2..2, "b");
    completer.refresh();
    assert!(!completer.menu().is_open(), "plain text has no completions");
    // Not even a source is asked for a language that has none.
    let fake = Rc::new(Fake::default());
    let controller = TextEditingController::with_text("");
    let markdown = Completer::new(
        Language::Markdown,
        controller.clone(),
        Some(Rc::clone(&fake) as Rc<dyn Source>),
    );
    controller.replace_range(0..0, "pu");
    markdown.refresh();
    assert_eq!(fake.asked.get(), 0);
}

#[test]
fn typing_opens_the_menu_and_keys_move_in_it() {
    let (completer, controller, _listener) = typing("fn main() {\n    ");
    controller.replace_range(16..16, "st");
    assert_eq!(chosen(&completer), Some("static".to_owned()));
    assert!(completer.handle_key(&NamedKey::ArrowDown));
    assert_eq!(chosen(&completer), Some("struct".to_owned()));
    assert!(completer.handle_key(&NamedKey::ArrowUp));
    assert_eq!(chosen(&completer), Some("static".to_owned()));
    assert!(
        !completer.handle_key(&NamedKey::ArrowLeft),
        "other keys are the text's"
    );
    controller.replace_range(18..18, "x");
    assert!(
        !completer.handle_key(&NamedKey::Enter),
        "a closed menu takes no key"
    );
}

#[test]
fn return_and_tab_insert_the_item_and_do_not_open_the_menu_again() {
    for key in [NamedKey::Enter, NamedKey::Tab] {
        let (completer, controller, _listener) = typing("");
        controller.replace_range(0..0, "ma");
        assert!(completer.handle_key(&NamedKey::ArrowDown));
        assert!(completer.handle_key(&key));
        assert_eq!(controller.value().text(), "match");
        assert_eq!(
            controller.value().selection().range(),
            5..5,
            "the caret is after the word"
        );
        assert_eq!(chosen(&completer), None, "inserted, and done");
        assert!(controller.undo(), "one undo takes the item back out");
        assert_eq!(controller.value().text(), "ma");
    }
}

#[test]
fn a_word_in_the_middle_of_the_text_is_replaced_up_to_the_caret_only() {
    let (completer, controller, _listener) = typing("let a = p_rest;");
    controller.select(9, 9);
    assert_eq!(chosen(&completer), None, "moving the caret is not typing");
    controller.replace_range(9..9, "u");
    assert_eq!(chosen(&completer), Some("pub".to_owned()));
    completer.accept();
    assert_eq!(
        controller.value().text(),
        "let a = pub_rest;",
        "what follows the caret stays"
    );
}

#[test]
fn escape_closes_the_menu_and_a_moved_caret_or_a_selection_closes_it_too() {
    let (completer, controller, _listener) = typing("");
    controller.replace_range(0..0, "pu");
    assert!(completer.handle_key(&NamedKey::Escape));
    assert_eq!(chosen(&completer), None);
    assert_eq!(controller.value().text(), "pu", "escape leaves the text");
    controller.replace_range(0..2, "re");
    assert_eq!(chosen(&completer), Some("ref".to_owned()));
    controller.select(0, 0);
    assert_eq!(chosen(&completer), None, "the caret moved away");
    controller.replace_range(0..2, "pu");
    assert!(completer.menu().is_open());
    controller.select(0, 2);
    assert!(
        !completer.menu().is_open(),
        "a selected range is not a word being typed"
    );
}

// --- The server.

#[test]
fn the_server_is_asked_when_a_word_is_begun_and_the_menu_is_the_answer() {
    let (completer, controller, _listener, fake) = served("let v = ");
    controller.replace_range(8..8, "Ve");
    assert_eq!(fake.asked.get(), 1);
    assert!(
        !completer.menu().is_open(),
        "nothing is shown that is not the server's, while it is asked"
    );
    fake.answer(items_at(8, &["VecDeque", "Vec", "Version", "HashMap"]));
    // In the order of the sort text, which is the label here; HashMap does not have the letters.
    assert_eq!(all(&completer), ["Vec", "VecDeque", "Version"]);
}

#[test]
fn a_word_that_grows_is_not_asked_of_the_server_again_and_narrows_what_there_is() {
    let (completer, controller, _listener, fake) = served("");
    controller.replace_range(0..0, "p");
    fake.answer(items_at(0, &["push", "pull", "pop", "print", "peek"]));
    assert_eq!(all(&completer).len(), 5);
    controller.replace_range(1..1, "u");
    assert_eq!(fake.asked.get(), 1, "one request for the word");
    assert_eq!(all(&completer), ["pull", "push"]);
    controller.replace_range(2..2, "s");
    assert_eq!(all(&completer), ["push"]);
    controller.replace_range(3..3, "h");
    assert_eq!(
        chosen(&completer),
        None,
        "push is what is typed: there is nothing to add"
    );
    assert_eq!(fake.asked.get(), 1);
}

#[test]
fn a_word_that_is_cut_shorter_than_what_was_asked_or_is_another_is_asked_again() {
    let (completer, controller, _listener, fake) = served("");
    controller.replace_range(0..0, "pu");
    fake.answer(items_at(0, &["push", "pull"]));
    assert_eq!(fake.asked.get(), 1);
    // The answer is for words that begin with pu, and p is not one.
    controller.replace_range(1..2, "");
    assert_eq!(fake.asked.get(), 2);
    fake.answer(items_at(0, &["push", "pop"]));
    assert_eq!(all(&completer), ["pop", "push"]);
    // Another word begun, after a space.
    controller.replace_range(1..1, " ");
    assert!(!completer.menu().is_open());
    controller.replace_range(2..2, "x");
    assert_eq!(fake.asked.get(), 3, "a new word is a new request");
}

#[test]
fn an_answer_for_a_word_that_was_typed_on_since_is_not_used() {
    let (completer, controller, _listener, fake) = served("");
    controller.replace_range(0..0, "V");
    controller.replace_range(1..1, "e");
    assert_eq!(fake.asked.get(), 2);
    // The server answers the first, which is late, and then the second.
    fake.answer_first(items_at(0, &["Value"]));
    assert!(!completer.menu().is_open(), "that was for V");
    fake.answer(items_at(0, &["Vec"]));
    assert_eq!(chosen(&completer), Some("Vec".to_owned()));
}

#[test]
fn an_answer_that_comes_after_the_caret_has_left_the_word_is_not_used() {
    let (completer, controller, _listener, fake) = served("a b");
    controller.replace_range(3..3, "x");
    controller.select(0, 0);
    fake.answer(items_at(3, &["xylophone"]));
    assert!(!completer.menu().is_open());
}

#[test]
fn where_the_server_cannot_say_the_keywords_are_offered_and_where_it_says_nothing_nothing_is() {
    let (completer, controller, _listener, fake) = served("");
    controller.replace_range(0..0, "pu");
    fake.answer(None);
    assert_eq!(
        chosen(&completer),
        Some("pub".to_owned()),
        "the server could not be asked"
    );

    controller.replace_range(2..2, "x");
    fake.answer(items_at(0, &[]));
    assert!(
        !completer.menu().is_open(),
        "the server looked and there is nothing: not a keyword either"
    );
}

#[test]
fn after_a_dot_or_a_double_colon_everything_the_server_says_is_offered_and_a_letter_narrows_it() {
    let (completer, controller, _listener, fake) = served("value");
    controller.replace_range(5..5, ".");
    assert_eq!(
        fake.asked.get(),
        1,
        "a dot is asked at once, with no letter"
    );
    fake.answer(items_at(6, &["len", "push", "iter", "is_empty"]));
    assert_eq!(
        all(&completer),
        ["is_empty", "iter", "len", "push"],
        "all of them, in the order of their sort text"
    );
    controller.replace_range(6..6, "i");
    assert_eq!(all(&completer), ["is_empty", "iter"]);
    assert_eq!(fake.asked.get(), 1);
    // Back to the dot: the answer for it still holds.
    controller.replace_range(6..7, "");
    assert_eq!(all(&completer).len(), 4);
    assert_eq!(fake.asked.get(), 1);

    let (completer, controller, _listener, fake) = served("std");
    controller.replace_range(3..3, "::");
    assert_eq!(fake.asked.get(), 1);
    fake.answer(items_at(5, &["fs", "io"]));
    assert_eq!(all(&completer), ["fs", "io"]);
    // Keywords are not offered after a dot, and two dots are a range, which is not asked of.
    let (_, controller2, _listener2, fake2) = served("0");
    controller2.replace_range(1..1, "..");
    assert_eq!(fake2.asked.get(), 0);
    let _ = controller;
}

#[test]
fn an_answer_that_says_it_is_cut_short_is_asked_again_as_the_word_grows() {
    let (completer, controller, _listener, fake) = served("");
    controller.replace_range(0..0, "a");
    let mut answer = items_at(0, &["alpha", "apple"]).unwrap();
    answer.incomplete = true;
    fake.answer(Some(answer));
    assert_eq!(all(&completer).len(), 2);
    controller.replace_range(1..1, "l");
    assert_eq!(fake.asked.get(), 2, "the answer holds for no other word");
    assert!(
        completer.menu().is_open(),
        "and what was shown stays until the new answer comes"
    );
    fake.answer(items_at(0, &["alpha", "alloy"]));
    assert_eq!(all(&completer), ["alloy", "alpha"]);
}

#[test]
fn an_answer_whose_words_begin_elsewhere_than_the_word_is_not_used() {
    let (completer, controller, _listener, fake) = served("");
    controller.replace_range(0..0, "ab");
    fake.answer(items_at(1, &["abc"]));
    assert!(!completer.menu().is_open());
}

#[test]
fn every_item_of_a_long_answer_can_be_chosen_and_the_menu_shows_a_window_of_them() {
    let (completer, controller, _listener, fake) = served("");
    controller.replace_range(0..0, "i");
    let labels: Vec<String> = (0..3000)
        .map(|number| format!("item{number:04}"))
        .collect();
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    fake.answer(items_at(0, &refs));
    assert_eq!(
        completer
            .menu()
            .with(|menu| menu.map(|menu| (menu.len(), menu.rows().count()))),
        Some((3000, super::super::menu::Menu::WINDOW))
    );
    completer.handle_key(&NamedKey::ArrowUp);
    assert_eq!(
        chosen(&completer),
        Some("item2999".to_owned()),
        "the last of them is one key from the first"
    );
    controller.replace_range(1..1, "tem29");
    // The hundred that begin with it, and no others: the other digits are not at the start of a word.
    let found = all(&completer);
    assert_eq!(found.len(), 100);
    assert!(
        found
            .iter()
            .all(|label| label.starts_with("item29"))
    );
}

#[test]
fn an_item_is_put_in_from_the_start_of_the_word_to_the_caret_by_the_text_it_says() {
    let (completer, controller, _listener, fake) = served("let v = ");
    controller.replace_range(8..8, "Ve");
    let mut vector = item("Vector", 8);
    vector.edit.text = "Vector::new()".to_owned();
    fake.answer(Some(Answer {
        items: vec![vector],
        incomplete: false,
    }));
    // Typed on after the answer, which narrows it and is replaced too.
    controller.replace_range(10..10, "c");
    assert!(completer.handle_key(&NamedKey::Tab));
    assert_eq!(controller.value().text(), "let v = Vector::new()");
    assert_eq!(controller.value().selection().range(), 21..21);
    assert!(controller.undo());
    assert_eq!(controller.value().text(), "let v = Vec");
}

#[test]
fn the_other_changes_an_item_asks_for_are_made_with_it_as_one_edit() {
    let (completer, controller, _listener, fake) = served("fn main() {\n    let m = HashM");
    let at = controller.value().text().find("HashM").unwrap();
    controller.replace_range(at + 5..at + 5, "a");
    let mut map = item("HashMap", at);
    map.edit.extra = vec![(0..0, "use std::collections::HashMap;\n\n".to_owned())];
    fake.answer(Some(Answer {
        items: vec![map],
        incomplete: false,
    }));
    completer.accept();
    assert_eq!(
        controller.value().text(),
        "use std::collections::HashMap;\n\nfn main() {\n    let m = HashMap"
    );
    assert_eq!(
        controller.value().selection().range().end,
        controller.value().text().len(),
        "the caret is after the word and not where the import went"
    );
    assert!(controller.undo());
    assert_eq!(
        controller.value().text(),
        "fn main() {\n    let m = HashMa",
        "both are taken back by one undo"
    );
}

#[test]
fn a_change_that_is_not_before_the_word_is_not_made_and_the_word_is() {
    let (completer, controller, _listener, fake) = served("x = ");
    controller.replace_range(4..4, "ab");
    let mut it = item("abc", 4);
    it.edit.extra = vec![(10..10, "after;".to_owned()), (0..0, "before;".to_owned())];
    fake.answer(Some(Answer {
        items: vec![it],
        incomplete: false,
    }));
    completer.accept();
    assert_eq!(controller.value().text(), "before;x = abc");
}

#[test]
fn changes_that_overlap_are_not_made_and_the_word_still_is() {
    let (completer, controller, _listener, fake) = served("x = ");
    controller.replace_range(4..4, "ab");
    let mut it = item("abc", 4);
    it.edit.extra = vec![(0..3, "one".to_owned()), (2..4, "two".to_owned())];
    fake.answer(Some(Answer {
        items: vec![it],
        incomplete: false,
    }));
    completer.accept();
    assert_eq!(controller.value().text(), "x = abc");
}

// --- A function is completed with its call.

/// A function the server offers at `start`, with the signature it says.
fn function_at(label: &str, start: usize, detail: Option<&str>) -> Item {
    Item {
        kind: ItemKind::Function,
        detail: detail.map(str::to_owned),
        ..item(label, start)
    }
}

/// Completes `text`, of which the word at its end is begun, with `function`, and answers what the text is and where the caret is.
fn completed(
    text: &str,
    function: impl FnOnce(usize) -> Item,
) -> (String, std::ops::Range<usize>, TextEditingController) {
    let (completer, controller, _listener, fake) = served(&text[..text.len() - 1]);
    controller.replace_range(text.len() - 1..text.len() - 1, &text[text.len() - 1..]);
    let word = text
        .rfind(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
        .map_or(0, |at| at + 1);
    fake.answer(Some(Answer {
        items: vec![function(word)],
        incomplete: false,
    }));
    completer.accept();
    let value = controller.value();
    (
        value.text().into_owned(),
        value.selection().range(),
        controller,
    )
}

#[test]
fn a_function_is_completed_with_its_call_and_the_caret_goes_in_where_there_is_an_argument() {
    let (text, caret, controller) = completed("let n = v.pu", |at| {
        function_at("push", at, Some("fn(&mut self, value: T)"))
    });
    assert_eq!(text, "let n = v.push()");
    assert_eq!(caret, 15..15, "between the brackets");
    assert!(
        controller.undo(),
        "one undo takes back the word and its brackets"
    );
    assert_eq!(controller.value().text(), "let n = v.pu");
}

#[test]
fn a_function_that_takes_nothing_is_completed_with_the_caret_after_its_call() {
    let (text, caret, _) = completed("let n = v.le", |at| {
        function_at("len", at, Some("fn(&self) -> usize"))
    });
    assert_eq!(text, "let n = v.len()");
    assert_eq!(
        caret,
        15..15,
        "after the brackets, where what follows the call goes"
    );
}

#[test]
fn a_macro_is_completed_with_the_caret_in_its_brackets() {
    let (text, caret, _) = completed("    printl", |at| function_at("println!", at, None));
    assert_eq!(text, "    println!()");
    assert_eq!(caret, 13..13);
}

#[test]
fn what_is_not_called_is_not_given_brackets() {
    let (text, _, _) = completed("let n = Ve", |at| Item {
        kind: ItemKind::Struct,
        ..item("Vec", at)
    });
    assert_eq!(text, "let n = Vec");
    let (text, _, _) = completed("use std::mem::sw", |at| {
        function_at("swap", at, Some("fn(a: &mut T, b: &mut T)"))
    });
    assert_eq!(
        text, "use std::mem::swap",
        "a name in an import is not called"
    );
}

#[test]
fn a_function_that_is_followed_by_its_arguments_is_not_given_brackets_again() {
    let (completer, controller, _listener, fake) = served("v.p(1)");
    controller.select(3, 3);
    controller.replace_range(3..3, "u");
    fake.answer(Some(Answer {
        items: vec![function_at("push", 2, Some("fn(&mut self, value: T)"))],
        incomplete: false,
    }));
    completer.accept();
    assert_eq!(controller.value().text(), "v.push(1)");
}

// --- The rest of an item, which the server says when the item is chosen.

/// An item that is to be imported, as the server says it.
fn importable(label: &str, start: usize) -> Item {
    Item {
        resolve: Some(serde_json::json!({"label": label})),
        ..item(label, start)
    }
}

fn answer_of(items: Vec<Item>) -> Option<Answer> {
    Some(Answer {
        items,
        incomplete: false,
    })
}

#[test]
fn the_item_that_is_chosen_is_asked_for_its_other_edits_when_it_has_some_to_give_and_only_once() {
    let (completer, controller, _listener, fake) = served("");
    controller.replace_range(0..0, "C");
    fake.answer(answer_of(vec![
        item("Cat", 0),
        importable("Circle", 0),
        importable("Cube", 0),
    ]));
    // The menu begins with Cat, which has nothing to say.
    assert_eq!(chosen(&completer), Some("Cat".to_owned()));
    assert_eq!(fake.resolved_labels(), Vec::<String>::new());
    completer.handle_key(&NamedKey::ArrowDown);
    assert_eq!(
        fake.resolved_labels(),
        ["Circle"],
        "the item that is chosen now"
    );
    fake.resolve_last(Some(vec![]));
    completer.handle_key(&NamedKey::ArrowDown);
    completer.handle_key(&NamedKey::ArrowUp);
    assert_eq!(
        fake.resolved_labels(),
        ["Cube"],
        "Circle is not asked for again: it said what it had"
    );
    // A word that narrows the menu to an item that is not resolved asks for it.
    controller.replace_range(1..1, "u");
    assert_eq!(chosen(&completer), Some("Cube".to_owned()));
}

#[test]
fn an_item_that_is_first_in_the_menu_is_asked_for_as_soon_as_the_menu_opens() {
    let (_completer, controller, _listener, fake) = served("");
    controller.replace_range(0..0, "C");
    fake.answer(answer_of(vec![
        importable("Cube", 0),
        importable("Circle", 0),
    ]));
    assert_eq!(
        fake.resolved_labels(),
        ["Circle"],
        "the first of the menu, which is the one that sorts first"
    );
}

#[test]
fn an_item_is_put_in_with_the_line_that_imports_it_as_one_edit() {
    let (completer, controller, _listener, fake) =
        served("mod shapes {\n    pub struct Circle;\n}\n\nfn main() {\n    ");
    let at = controller.value().text().len();
    controller.replace_range(at..at, "Circ");
    fake.answer(answer_of(vec![importable("Circle", at)]));
    assert_eq!(fake.resolved_labels(), ["Circle"]);
    fake.resolve_last(Some(vec![(0..0, "use shapes::Circle;\n".to_owned())]));
    assert!(completer.handle_key(&NamedKey::Enter));
    let text = controller.value().text().to_string();
    assert!(
        text.starts_with("use shapes::Circle;\nmod shapes"),
        "{text}"
    );
    assert!(text.ends_with("    Circle"), "{text}");
    assert_eq!(
        controller.value().selection().range().end,
        text.len(),
        "the caret is after the word"
    );
    assert!(
        controller.undo(),
        "one undo takes the word and the line out"
    );
    assert!(
        controller.value().text().ends_with("    Circ")
            && controller
                .value()
                .text()
                .starts_with("mod shapes")
    );
}

#[test]
fn an_item_that_is_put_in_before_the_server_has_said_its_other_edits_is_put_in_without_them() {
    let (completer, controller, _listener, fake) = served("fn main() {\n    ");
    let at = controller.value().text().len();
    controller.replace_range(at..at, "Circ");
    fake.answer(answer_of(vec![importable("Circle", at)]));
    completer.accept();
    assert_eq!(controller.value().text(), "fn main() {\n    Circle");
    // The answer comes after, to what has been put in: it is not made, and does nothing.
    fake.resolve_last(Some(vec![(0..0, "use late;\n".to_owned())]));
    assert_eq!(controller.value().text(), "fn main() {\n    Circle");
}

#[test]
fn what_the_server_says_of_an_item_of_an_answer_that_is_gone_is_not_taken_for_one_of_the_next() {
    let (completer, controller, _listener, fake) = served("");
    controller.replace_range(0..0, "C");
    fake.answer(answer_of(vec![importable("Circle", 0)]));
    // Another word is begun, which is asked for and answered, and where the same number is another item.
    controller.replace_range(1..1, " ");
    controller.replace_range(2..2, "D");
    fake.answer(answer_of(vec![importable("Dx", 2)]));
    assert_eq!(fake.resolved_labels(), ["Circle", "Dx"]);
    // Circle's answer comes now: it is for the answer that is not kept.
    let (_, late) = fake.resolving.borrow_mut().remove(0);
    late(Some(vec![(0..0, "use circle;\n".to_owned())]));
    completer.accept();
    assert_eq!(
        controller.value().text(),
        "C Dx",
        "the line for Circle was not put in with Dx"
    );
}

#[test]
fn an_item_that_could_not_be_resolved_is_put_in_as_it_is_and_is_not_asked_for_again() {
    let (completer, controller, _listener, fake) = served("");
    controller.replace_range(0..0, "Ci");
    fake.answer(answer_of(vec![
        importable("Circle", 0),
        importable("Cirrus", 0),
    ]));
    fake.resolve_last(None);
    completer.handle_key(&NamedKey::ArrowDown);
    completer.handle_key(&NamedKey::ArrowUp);
    assert_eq!(
        fake.resolved_labels(),
        ["Cirrus"],
        "Circle was asked for once, and said it could not"
    );
    completer.accept();
    assert_eq!(controller.value().text(), "Circle");
}

// --- Comfort: what a word that is typed whole does, how fast the menu follows, and a click.

#[test]
fn return_after_a_word_that_is_typed_whole_is_a_new_line_and_not_a_longer_item() {
    // `as` is a keyword and begins `async`: Return must not turn the one into the other.
    let (completer, controller, _listener) = typing("");
    controller.replace_range(0..0, "as");
    assert_eq!(
        chosen(&completer),
        Some("as".to_owned()),
        "the word itself is the item that is chosen"
    );
    assert!(
        !completer.handle_key(&NamedKey::Enter),
        "the key is the text's, which makes the new line"
    );
    assert_eq!(controller.value().text(), "as");
    assert!(!completer.menu().is_open());

    // The longer item is one key away, and then Return is for it.
    let (completer, controller, _listener) = typing("");
    controller.replace_range(0..0, "as");
    assert!(completer.handle_key(&NamedKey::ArrowDown));
    assert!(completer.handle_key(&NamedKey::Enter));
    assert_eq!(controller.value().text(), "async");
}

#[test]
fn a_word_that_is_typed_whole_is_the_first_of_what_the_server_offers_and_is_not_replaced_by_what_it_does_not_begin()
 {
    let (completer, controller, _listener, fake) = served("");
    controller.replace_range(0..0, "let");
    // As rust-analyzer says it: the keywords are not first, and the types that have an l, an e and a t are among them.
    let mut list = vec![
        "AnyElement",
        "BoxAlignment",
        "BuildContext",
        "letm",
        "Letter",
    ];
    list.push("let");
    fake.answer(items_at(0, &list));
    assert_eq!(
        all(&completer),
        ["let", "letm", "Letter"],
        "AnyElement has the three letters, none of them at the start of a word"
    );
    assert!(
        !completer.handle_key(&NamedKey::Enter),
        "Return is a new line"
    );
    assert_eq!(
        controller.value().text(),
        "let",
        "and the word is as it was typed"
    );
}

#[test]
fn letters_in_the_middle_of_the_names_of_types_do_not_make_a_menu() {
    let (completer, controller, _listener, fake) = served("");
    controller.replace_range(0..0, "el");
    fake.answer(items_at(0, &["AnyElement", "BoxElement", "Extension"]));
    assert_eq!(
        all(&completer),
        ["AnyElement", "BoxElement"],
        "El of Element is the start of a word"
    );
    // The l and the e of Element are in it, but not where a word starts: there is nothing to offer.
    controller.replace_range(0..2, "le");
    assert!(!completer.menu().is_open());
}

#[test]
fn a_word_that_has_grown_is_shown_narrowed_at_once_while_the_server_is_asked_again_for_a_list_cut_short()
 {
    let (completer, controller, _listener, fake) = served("");
    controller.replace_range(0..0, "p");
    let mut first = items_at(0, &["push", "pull", "pop", "peek"]).unwrap();
    first.incomplete = true;
    fake.answer(Some(first));
    assert_eq!(all(&completer).len(), 4);
    controller.replace_range(1..1, "u");
    // What the server has not said yet is not waited for: what it said is narrowed by the letter typed.
    assert_eq!(all(&completer), ["pull", "push"]);
    assert_eq!(
        fake.asked.get(),
        2,
        "and the server is asked for the word as it is"
    );
    fake.answer(items_at(0, &["push", "pub", "pull"]));
    assert_eq!(
        all(&completer),
        ["pub", "pull", "push"],
        "its answer replaces what was shown"
    );
}

#[test]
fn a_click_on_a_row_puts_that_item_in_and_does_not_need_a_key() {
    let (completer, controller, _listener) = typing("");
    controller.replace_range(0..0, "st");
    assert_eq!(all(&completer), ["static", "struct"]);
    completer.menu().pick(1);
    assert_eq!(controller.value().text(), "struct");
    assert!(!completer.menu().is_open());
    assert_eq!(controller.value().selection().range(), 6..6);
    // A click after the menu is gone, or past the rows, does nothing.
    completer.menu().pick(0);
    assert_eq!(controller.value().text(), "struct");
}

#[test]
fn a_whole_word_that_is_to_be_imported_is_still_offered_so_that_it_can_be_imported() {
    let (completer, controller, _listener, fake) = served("fn main() {\n    ");
    let at = controller.value().text().len();
    controller.replace_range(at..at, "Circle");
    fake.answer(answer_of(vec![importable("Circle", at)]));
    assert_eq!(
        chosen(&completer),
        Some("Circle".to_owned()),
        "it adds the line that imports it"
    );
    fake.resolve_last(Some(vec![(0..0, "use shapes::Circle;\n".to_owned())]));
    assert!(
        completer.handle_key(&NamedKey::Enter),
        "the key is the menu's, which imports"
    );
    assert_eq!(
        controller.value().text(),
        "use shapes::Circle;\nfn main() {\n    Circle"
    );
}

// --- A server that is slow to answer.

fn settle(venus: &Rc<aimer::Venus>) {
    for _ in 0..50 {
        if !venus.has_ready_work() {
            return;
        }
        venus.run_microtasks();
    }
}

#[test]
fn a_server_that_has_not_answered_after_a_moment_is_covered_by_the_keywords_until_it_does() {
    let venus = aimer::Venus::new();
    venus.install();
    let (completer, controller, _listener, fake) = served("");
    controller.replace_range(0..0, "pu");
    assert!(
        !completer.menu().is_open(),
        "not at once: the server may answer in the moment"
    );
    settle(&venus);
    assert_eq!(
        chosen(&completer),
        Some("pub".to_owned()),
        "the moment passed, and the keywords are what there is"
    );
    // The answer comes, and is the menu.
    fake.answer(items_at(0, &["push", "pull", "pub"]));
    assert_eq!(all(&completer), ["pub", "pull", "push"]);
    let _ = aimer::Venus::uninstall();
}

#[test]
fn the_keywords_do_not_replace_a_menu_that_is_showing_or_cover_a_word_that_has_moved_on() {
    let venus = aimer::Venus::new();
    venus.install();
    let (completer, controller, _listener, fake) = served("");
    controller.replace_range(0..0, "p");
    let mut first = items_at(0, &["push", "pull"]).unwrap();
    first.incomplete = true;
    fake.answer(Some(first));
    // Typed on: narrowed at once, and asked again. The keywords do not come over what is shown while it waits.
    controller.replace_range(1..1, "u");
    settle(&venus);
    assert_eq!(all(&completer), ["pull", "push"]);
    // A word that is gone is not covered: the caret has left it.
    controller.replace_range(2..2, " ");
    controller.replace_range(3..3, "m");
    controller.select(0, 0);
    settle(&venus);
    assert!(!completer.menu().is_open());
    let _ = aimer::Venus::uninstall();
}

#[test]
fn a_menu_that_is_dismissed_is_gone_with_what_was_asked_for_it() {
    let (completer, controller, _listener, fake) = served("");
    controller.replace_range(0..0, "p");
    fake.answer(items_at(0, &["push", "pull"]));
    assert!(completer.menu().is_open());
    // The editor lost focus, or changed size: the menu is not where it was.
    completer.menu().dismiss();
    assert!(!completer.menu().is_open());
    // And an answer that was on its way for it does not bring it back.
    controller.replace_range(1..1, "u");
    completer.menu().dismiss();
    fake.answer(items_at(0, &["push"]));
    assert!(
        !completer.menu().is_open(),
        "that answer was for what is gone"
    );
    // The next word typed is asked for as usual.
    controller.replace_range(2..2, " ");
    controller.replace_range(3..3, "s");
    fake.answer(items_at(3, &["static"]));
    assert_eq!(chosen(&completer), Some("static".to_owned()));
}

#[test]
fn return_after_a_whole_function_name_completes_the_call_instead_of_inserting_a_newline() {
    // Multiple matches with the typed function name selected first: Return completes the call.
    let (completer, controller, _listener, fake) = served("v.");
    controller.replace_range(2..2, "push");
    fake.answer(Some(Answer {
        items: vec![
            Item {
                kind: ItemKind::Method,
                detail: Some("fn(&mut self, value: T)".to_owned()),
                ..item("push", 2)
            },
            Item {
                kind: ItemKind::Method,
                detail: Some("fn(&mut self, s: &str)".to_owned()),
                ..item("push_str", 2)
            },
        ],
        incomplete: false,
    }));
    assert_eq!(chosen(&completer), Some("push".to_owned()));
    assert!(
        completer.handle_key(&NamedKey::Enter),
        "Return completes `push` to `push()` instead of passing through as a newline"
    );
    assert_eq!(controller.value().text(), "v.push()");
    assert_eq!(controller.value().selection().range(), 7..7);

    // A sole matching function is still offered so Return can add its parentheses.
    let (completer, controller, _listener, fake) = served("v.");
    controller.replace_range(2..2, "len");
    fake.answer(Some(Answer {
        items: vec![Item {
            kind: ItemKind::Method,
            detail: Some("fn(&self) -> usize".to_owned()),
            ..item("len", 2)
        }],
        incomplete: false,
    }));
    assert_eq!(chosen(&completer), Some("len".to_owned()));
    assert!(completer.handle_key(&NamedKey::Enter));
    assert_eq!(controller.value().text(), "v.len()");
    assert_eq!(controller.value().selection().range(), 7..7);
}

#[test]
fn postfix_items_with_an_earlier_range_start_do_not_discard_completions_for_the_word() {
    let (completer, controller, _listener, fake) = served("v.");
    controller.replace_range(2..2, "i");
    let mut postfix_if = item("if", 0);
    postfix_if.edit.range = Some(0..3);
    postfix_if.edit.text = "if v {}".to_owned();
    let into = Item {
        kind: ItemKind::Method,
        detail: Some("fn(self) -> T".to_owned()),
        ..item("into", 2)
    };
    let is_empty = Item {
        kind: ItemKind::Method,
        detail: Some("fn(&self) -> bool".to_owned()),
        ..item("is_empty", 2)
    };
    fake.answer(Some(Answer {
        items: vec![postfix_if, into, is_empty],
        incomplete: false,
    }));
    assert_eq!(all(&completer), ["into", "is_empty"]);
    assert!(completer.handle_key(&NamedKey::Enter));
    assert_eq!(controller.value().text(), "v.into()");
}
