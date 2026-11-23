use std::cell::RefCell;
use std::path::Path;

use aimer::input::TextEditingController;
use aimer::input::input::TextSnapshot;
use serde_json::{Value, json};

use super::*;
use crate::services::lsp::Position;
use crate::services::lsp::fake::{Fake, installed, lsp_with, settle};
use crate::services::settings::ThemeMode;
use crate::model::syntax::{SyntaxHighlighter, SyntaxPalette};
use crate::model::tree::Scratch;

fn spot() -> FieldSpot {
    FieldSpot {
        x: 10.0,
        y: 20.0,
        width: 30.0,
        height: 16.0,
        extent: Some((800.0, 600.0)),
    }
}

/// No file is open with changes the disk does not have.
fn no_buffers(_: &Path) -> Option<TextSnapshot> {
    None
}

/// The byte of a line that a character of the server's is, for a text of one byte to a character.
fn ascii(_line: &str, character: u32) -> usize {
    character as usize
}

fn heading() -> Heading {
    Heading {
        kind: Some("Struct"),
        name: "Rules".into(),
        definition: "src/rules.rs".into(),
        noun: "usages",
        count: 3,
    }
}

/// The places of a list, in the order they are shown: its entries without the note.
fn rows(list: &UsageList) -> Vec<&UsageRow> {
    list.entries
        .iter()
        .filter_map(|entry| match entry {
            UsageEntry::Place(row) => Some(row),
            UsageEntry::More(_) => None,
        })
        .collect()
}

fn place(path: &Path, line: u32, character: u32) -> Place {
    Place {
        path: path.to_path_buf(),
        at: Position { line, character },
        end: Position {
            line,
            character: character + 3,
        },
    }
}

#[test]
fn a_list_is_by_file_in_the_order_files_are_met_and_by_line_in_each_with_the_line_of_code() {
    let scratch = Scratch::new("usages-list");
    let a = scratch.file(
        "src/a.rs",
        "fn a() {\n    use_it(1);\n}\n\n    use_it(2);   \n",
    );
    let b = scratch.file("src/b.rs", "// one\r\nuse_it(3);\r\n");
    let places = [place(&b, 1, 0), place(&a, 4, 4), place(&a, 1, 4)];
    let list = UsageList::gather(heading(), &places, &scratch.0, spot(), &ascii, &no_buffers);
    assert_eq!(list.heading, heading());
    assert_eq!(list.spot, spot());
    assert_eq!(list.len(), 3);
    assert_eq!(list.omitted, 0);
    let shown: Vec<(&str, usize, &str)> = rows(&list)
        .iter()
        .map(|row| (&*row.file, row.number, row.text.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            ("src/b.rs", 2, "use_it(3);"),
            ("src/a.rs", 2, "use_it(1);"),
            ("src/a.rs", 5, "use_it(2);")
        ],
        "relative to the folder, the files in the order they were met, and in each by line with the blanks cut and a line break of two characters not in the line"
    );
    let indexes: Vec<usize> = rows(&list)
        .iter()
        .map(|row| row.index)
        .collect();
    assert_eq!(
        indexes,
        [0, 1, 2],
        "each row says which it is, as the card builds one at a time"
    );
    assert_eq!(list.entries.len(), 3, "no note where none were left out");
    assert_eq!(
        list.place_at(1),
        Some(&places[2]),
        "the places are found by the number they are shown at"
    );
    assert_eq!(list.place_at(3), None);
}

#[test]
fn the_lines_of_a_file_that_is_open_are_read_from_what_is_open_and_not_from_the_disk() {
    let scratch = Scratch::new("usages-buffer");
    // On the disk the name is on the second line, and in the editor a line was added above it: the server counts the editor's.
    let file = scratch.file("a.rs", "// old\nuse_it();\nrest();\n");
    let open = TextSnapshot::from_text("// old\n// added\nuse_it();\nrest();\n");
    let buffers = |path: &Path| (path == file.as_path()).then(|| open.clone());
    let at = |line: u32| Place {
        path: file.clone(),
        at: Position { line, character: 0 },
        end: Position { line, character: 6 },
    };
    let list = UsageList::gather(
        heading(),
        &[at(2), at(3)],
        &scratch.0,
        spot(),
        &ascii,
        &buffers,
    );
    let shown: Vec<(usize, &str)> = rows(&list)
        .iter()
        .map(|row| (row.number, row.text.as_str()))
        .collect();
    assert_eq!(
        shown,
        [(3, "use_it();"), (4, "rest();")],
        "the lines are the editor's, which is what the server counted"
    );
    assert_eq!(
        &rows(&list)[0].text[rows(&list)[0].mark.clone()],
        "use_it",
        "and the name is marked in them"
    );
    // A place after the last line of what is open has no line, and is still listed.
    let beyond = UsageList::gather(heading(), &[at(40)], &scratch.0, spot(), &ascii, &buffers);
    assert_eq!((beyond.len(), rows(&beyond)[0].text.as_str()), (1, ""));
    // A file that is not open is read from the disk, as it was.
    let other = scratch.file("b.rs", "x\nfrom_disk();\n");
    let on_disk = Place {
        path: other,
        at: Position {
            line: 1,
            character: 0,
        },
        end: Position {
            line: 1,
            character: 9,
        },
    };
    let list = UsageList::gather(heading(), &[on_disk], &scratch.0, spot(), &ascii, &buffers);
    assert_eq!(rows(&list)[0].text, "from_disk();");
}

#[test]
fn each_row_says_which_part_of_its_line_is_the_usage_in_the_bytes_of_the_line_as_shown() {
    let scratch = Scratch::new("usages-mark");
    // The line is indented by four, which is not shown, and the name is the second word.
    let file = scratch.file(
        "a.rs",
        "    let r = Rules::new();\n\u{e9}t\u{e9} = Rules;\n",
    );
    let places = [
        Place {
            path: file.clone(),
            at: Position {
                line: 0,
                character: 12,
            },
            end: Position {
                line: 0,
                character: 17,
            },
        },
        // After the two accents, which are two bytes each but a character each for a server that counts characters.
        Place {
            path: file.clone(),
            at: Position {
                line: 1,
                character: 6,
            },
            end: Position {
                line: 1,
                character: 11,
            },
        },
    ];
    let utf16 = |line: &str, character: u32| {
        line.char_indices()
            .nth(character as usize)
            .map_or(line.len(), |(at, _)| at)
    };
    let list = UsageList::gather(heading(), &places, &scratch.0, spot(), &utf16, &no_buffers);
    let shown = rows(&list);
    assert_eq!(shown[0].text, "let r = Rules::new();");
    assert_eq!(
        &shown[0].text[shown[0].mark.clone()],
        "Rules",
        "the blanks that were cut are not counted"
    );
    assert_eq!(shown[1].text, "\u{e9}t\u{e9} = Rules;");
    assert_eq!(
        &shown[1].text[shown[1].mark.clone()],
        "Rules",
        "bytes, though the server counted characters"
    );
    // A usage that is beyond what is kept of a long line is marked as far as the line goes, or not at all.
    let long = scratch.file("long.rs", &format!("{}Rules\n", "x".repeat(300)));
    let far = Place {
        path: long,
        at: Position {
            line: 0,
            character: 300,
        },
        end: Position {
            line: 0,
            character: 305,
        },
    };
    let list = UsageList::gather(heading(), &[far], &scratch.0, spot(), &ascii, &no_buffers);
    let row = rows(&list)[0];
    assert!(
        row.mark.start >= row.text.len() && row.mark.end >= row.mark.start,
        "outside what is kept: {:?} of {}",
        row.mark,
        row.text.len()
    );
    // A file that cannot be read has no line, and so nothing to mark.
    let missing = place(&scratch.0.join("gone.rs"), 0, 0);
    let list = UsageList::gather(
        heading(),
        &[missing],
        &scratch.0,
        spot(),
        &ascii,
        &no_buffers,
    );
    assert_eq!(rows(&list)[0].mark, 0..0);
}

#[test]
fn each_row_has_the_pieces_its_line_is_drawn_in_which_are_colored_by_the_syntax_with_the_name_in_bold()
 {
    let scratch = Scratch::new("usages-pieces");
    let file = scratch.file("a.rs", "    let r = Rules;\n");
    let at = Place {
        path: file,
        at: Position {
            line: 0,
            character: 12,
        },
        end: Position {
            line: 0,
            character: 17,
        },
    };
    let plain = UsageList::gather(
        heading(),
        &[at.clone()],
        &scratch.0,
        spot(),
        &ascii,
        &no_buffers,
    );
    assert!(
        rows(&plain)[0].pieces.is_empty(),
        "not before the syntax has been asked"
    );
    let highlighter = SyntaxHighlighter::rust(SyntaxPalette::dark());
    let list = UsageList::gather_highlighted(
        heading(),
        &[at],
        &scratch.0,
        spot(),
        &ascii,
        &no_buffers,
        &highlighter,
    );
    let pieces = &rows(&list)[0].pieces;
    assert_eq!(
        pieces
            .iter()
            .map(|(text, _)| text.as_str())
            .collect::<String>(),
        "let r = Rules;",
        "they are the line"
    );
    let keyword = pieces
        .iter()
        .find(|(text, _)| text == "let")
        .expect("the keyword is a piece");
    assert_eq!(keyword.1.color, Some(SyntaxPalette::dark().keyword));
    let name = pieces
        .iter()
        .find(|(text, _)| text == "Rules")
        .expect("so is the name");
    assert_eq!(
        name.1.font_weight,
        Some(aimer::style::FontWeight::Bold),
        "the name that is used is in bold"
    );
    assert_eq!(
        pieces
            .iter()
            .filter(|(_, style)| style.font_weight.is_some())
            .count(),
        1,
        "and nothing else"
    );
}

#[test]
fn a_list_that_opens_has_its_first_place_selected_and_the_arrows_move_the_selection_and_stop_at_the_ends()
 {
    let pane = pane();
    pane.load();
    assert!(
        !pane.usages.move_selection(1),
        "no list is open: the key is not the list's"
    );
    assert_eq!(pane.usages.selected(), None);
    assert!(pane.usages.show_usages_at(11, spot()), "two places");
    assert_eq!(pane.usages.selected(), Some(0));
    let drawn = pane.list_drawn.get();
    assert!(pane.usages.move_selection(1), "the key is the list's");
    assert_eq!(pane.usages.selected(), Some(1));
    assert_eq!(
        pane.list_drawn.get(),
        drawn,
        "the card is not drawn again for the new row: the two rows that change draw themselves"
    );
    assert!(
        pane.usages.move_selection(1),
        "still the list's key at the end"
    );
    assert_eq!(
        pane.usages.selected(),
        Some(1),
        "and there is nowhere further to go"
    );
    assert_eq!(
        pane.list_drawn.get(),
        drawn,
        "nothing changed, so nothing is drawn"
    );
    assert!(pane.usages.move_selection(-5));
    assert_eq!(
        pane.usages.selected(),
        Some(0),
        "a long way up stops at the first"
    );
    pane.usages.select_edge(true);
    assert_eq!(pane.usages.selected(), Some(1));
    pane.usages.select_edge(false);
    assert_eq!(pane.usages.selected(), Some(0));
    // The pointer chooses a row too, on a list that no key has moved, and one that is not there is not chosen.
    let fresh = Pane::fresh_list();
    fresh.usages.select(1);
    assert_eq!(fresh.usages.selected(), Some(1));
    fresh.usages.select(99);
    assert_eq!(fresh.usages.selected(), Some(1));
}

/// What a row of the list does when it is told: counts the times, and says whether it is still there.
fn row_counter(alive: &Rc<Cell<bool>>) -> (Rc<Cell<usize>>, Rc<dyn Fn() -> bool>) {
    let (calls, still) = (Rc::new(Cell::new(0)), Rc::clone(alive));
    let counted = Rc::clone(&calls);
    (
        calls,
        Rc::new(move || {
            counted.set(counted.get() + 1);
            still.get()
        }),
    )
}

#[test]
fn a_choice_tells_only_the_row_that_was_chosen_and_the_one_that_is_now_and_a_row_that_is_gone_is_not_told_again()
 {
    let pane = Pane::fresh_list();
    let alive = Rc::new(Cell::new(true));
    let (first, second, third) = (
        row_counter(&alive),
        row_counter(&alive),
        row_counter(&alive),
    );
    pane.usages
        .on_row_change(0, Rc::clone(&first.1));
    pane.usages
        .on_row_change(1, Rc::clone(&second.1));
    pane.usages
        .on_row_change(2, Rc::clone(&third.1));
    pane.usages.select(1);
    assert_eq!(
        (first.0.get(), second.0.get(), third.0.get()),
        (1, 1, 0),
        "the row that was chosen and the one that is: not the rest"
    );
    pane.usages.select(1);
    assert_eq!(
        (first.0.get(), second.0.get()),
        (1, 1),
        "nothing is changed by the row that is chosen already"
    );
    // A row that says it is gone is forgotten, and is not asked again.
    alive.set(false);
    pane.usages.select(0);
    assert_eq!((first.0.get(), second.0.get()), (2, 2));
    pane.usages.select(1);
    assert_eq!(
        (first.0.get(), second.0.get()),
        (2, 2),
        "both said they were gone"
    );
    // A row that is made again takes the place of the one before it, and is the only one told.
    let again = row_counter(&Rc::new(Cell::new(true)));
    pane.usages
        .on_row_change(0, Rc::clone(&again.1));
    pane.usages
        .on_row_change(0, Rc::clone(&again.1));
    pane.usages.select(0);
    assert_eq!(
        again.0.get(),
        1,
        "once, though it was added twice for the same row"
    );
}

#[test]
fn return_opens_the_place_that_is_selected_and_shuts_the_list_and_the_selection_follows_each_new_list()
 {
    let pane = pane();
    pane.load();
    assert_eq!(pane.usages.open_selected(), None, "nothing is open");
    assert!(pane.usages.show_usages_at(11, spot()));
    pane.usages.move_selection(1);
    let place = pane
        .usages
        .open_selected()
        .expect("the selected place");
    assert_eq!(
        place.at.line, 2,
        "the second of the two places that the lens listed"
    );
    assert!(
        pane.usages.list().is_none(),
        "the list is shut by going there"
    );
    assert_eq!(pane.usages.selected(), None);
    // A list that is opened again begins at its first place, and not where the last one was left.
    assert!(pane.usages.show_usages_at(11, spot()));
    assert_eq!(pane.usages.selected(), Some(0));
    pane.usages.move_selection(1);
    pane.usages.close();
    assert!(pane.usages.show_usages_at(11, spot()));
    assert_eq!(pane.usages.selected(), Some(0));
}

#[test]
fn a_selection_that_moves_by_the_keys_is_brought_into_view_and_one_the_pointer_chose_is_not_moved_to()
 {
    let pane = pane();
    pane.load();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&seen);
    pane.usages
        .on_reveal(Rc::new(move |index, count| {
            record.borrow_mut().push((index, count))
        }));
    assert!(pane.usages.show_usages_at(11, spot()));
    pane.usages.move_selection(1);
    assert_eq!(
        *seen.borrow(),
        vec![(1, 2)],
        "the keys ask for the row to be shown, and say how many rows there are"
    );
    pane.usages.select(0);
    assert_eq!(
        *seen.borrow(),
        vec![(1, 2)],
        "the pointer is over a row that is in view, and the list is not scrolled under it"
    );
}

#[test]
fn the_keys_of_an_open_list_move_in_it_go_to_a_place_and_shut_it_and_all_others_are_the_texts() {
    use aimer::events::element::NamedKey;
    let pane = pane();
    pane.load();
    // With no list open, no key is the list's, not even Escape or Return.
    for key in [
        NamedKey::ArrowDown,
        NamedKey::Enter,
        NamedKey::Escape,
        NamedKey::Home,
    ] {
        assert_eq!(pane.usages.take_key(&key), Taken::No, "{key:?}");
    }
    assert!(pane.usages.show_usages_at(11, spot()));
    assert_eq!(pane.usages.take_key(&NamedKey::ArrowDown), Taken::Yes);
    assert_eq!(pane.usages.selected(), Some(1));
    assert_eq!(pane.usages.take_key(&NamedKey::ArrowUp), Taken::Yes);
    assert_eq!(pane.usages.selected(), Some(0));
    assert_eq!(pane.usages.take_key(&NamedKey::End), Taken::Yes);
    assert_eq!(pane.usages.selected(), Some(1));
    assert_eq!(pane.usages.take_key(&NamedKey::Home), Taken::Yes);
    assert_eq!(pane.usages.selected(), Some(0));
    assert_eq!(pane.usages.take_key(&NamedKey::PageDown), Taken::Yes);
    assert_eq!(
        pane.usages.selected(),
        Some(1),
        "a page down in a short list is the end"
    );
    assert_eq!(pane.usages.take_key(&NamedKey::PageUp), Taken::Yes);
    assert_eq!(pane.usages.selected(), Some(0));
    // Return goes to the place that is chosen, and shuts the list.
    pane.usages.take_key(&NamedKey::ArrowDown);
    match pane.usages.take_key(&NamedKey::Enter) {
        Taken::Go(place) => assert_eq!(place.at.line, 2),
        other => panic!("Return goes to a place, and it was {other:?}"),
    }
    assert!(pane.usages.list().is_none());
    // Escape shuts it and is taken, and the keys of the text are the text's, which also shuts it, as the field says.
    assert!(pane.usages.show_usages_at(11, spot()));
    assert_eq!(pane.usages.take_key(&NamedKey::Escape), Taken::Yes);
    assert!(pane.usages.list().is_none());
    assert!(pane.usages.show_usages_at(11, spot()));
    for key in [NamedKey::ArrowLeft, NamedKey::Backspace, NamedKey::Tab] {
        assert_eq!(pane.usages.take_key(&key), Taken::No, "{key:?}");
    }
    assert!(
        pane.usages.list().is_some(),
        "taking no key does not shut it: the field does that when it types"
    );
}

#[test]
fn the_pointer_does_not_take_back_the_choice_that_the_keys_just_made() {
    let pane = pane();
    pane.load();
    assert!(pane.usages.show_usages_at(11, spot()));
    // The list is scrolled by a key and a row comes to be under a pointer that has not moved: that is not the pointer choosing it.
    pane.usages.move_selection(1);
    pane.usages.select(0);
    assert_eq!(pane.usages.selected(), Some(1), "the key's choice stands");
    // When no key has moved the choice lately, the pointer chooses as it did.
    let fresh = Pane::fresh_list();
    fresh.usages.select(1);
    assert_eq!(fresh.usages.selected(), Some(1));
}

#[test]
fn the_kind_of_a_definition_is_the_word_before_its_name() {
    let kind = Usages::kind_of;
    assert_eq!(kind("pub struct "), Some("Struct"));
    assert_eq!(kind("    pub(crate) enum "), Some("Enum"));
    assert_eq!(kind("pub trait "), Some("Trait"));
    assert_eq!(kind("    pub async fn "), Some("Function"));
    assert_eq!(kind("pub const unsafe fn "), Some("Function"));
    assert_eq!(kind("type "), Some("Type alias"));
    assert_eq!(kind("pub union "), Some("Union"));
    assert_eq!(kind("    pub const "), Some("Constant"));
    assert_eq!(kind("pub static "), Some("Static"));
    assert_eq!(kind("let "), None, "not a definition of those");
    assert_eq!(kind(""), None);
    assert_eq!(kind("structure "), None, "a word that only starts like one");
}

#[test]
fn the_heading_of_a_list_has_the_kind_the_name_and_where_it_is_defined() {
    let pane = pane();
    pane.load();
    assert!(pane.usages.show_usages_at(11, spot()));
    let list = pane.usages.list().unwrap();
    assert_eq!(
        list.heading,
        Heading {
            kind: Some("Struct"),
            name: "Rules".into(),
            definition: "src/main.rs".into(),
            noun: "usages",
            count: 2
        }
    );
    assert_eq!(list.heading.title(), "Struct Rules");
    assert_eq!(
        list.heading.file(),
        "main.rs",
        "the heading says the file and not the whole path: it is a short line"
    );
    assert_eq!(list.heading.count_text(), "2 usages");
    let one = Heading {
        count: 1,
        noun: "implementations",
        ..list.heading.clone()
    };
    assert_eq!(one.count_text(), "1 implementation");
    let plain = Heading {
        kind: None,
        ..list.heading.clone()
    };
    assert_eq!(plain.title(), "Rules", "no kind to say");
}

#[test]
fn a_place_is_listed_when_its_file_cannot_be_read_is_not_text_or_has_no_such_line_and_a_long_line_is_cut()
 {
    let scratch = Scratch::new("usages-unread");
    let binary = scratch.file("bin.rs", "x");
    std::fs::write(&binary, [0xff, 0xfe, 0x00, 0x01]).unwrap();
    let short = scratch.file("short.rs", "one line\n");
    let long = scratch.file("long.rs", &format!("{}\n", "y".repeat(500)));
    let missing = scratch.0.join("gone.rs");
    let places = [
        place(&binary, 0, 0),
        place(&short, 7, 0),
        place(&long, 0, 0),
        place(&missing, 3, 1),
    ];
    let list = UsageList::gather(heading(), &places, &scratch.0, spot(), &ascii, &no_buffers);
    assert_eq!(list.len(), 4, "every place is listed");
    let text_of = |name: &str| {
        rows(&list)
            .iter()
            .find(|row| &*row.file == name)
            .unwrap()
            .text
            .clone()
    };
    assert_eq!(text_of("bin.rs"), "", "not text");
    assert_eq!(text_of("short.rs"), "", "a line the file does not have");
    assert_eq!(text_of("gone.rs"), "", "a file that is not there");
    assert_eq!(
        text_of("long.rs").chars().count(),
        UsageList::MOST_TEXT,
        "cut"
    );
}

#[test]
fn a_file_outside_the_folder_is_named_by_its_whole_path_and_no_places_is_an_empty_list() {
    let scratch = Scratch::new("usages-outside");
    let elsewhere = std::env::temp_dir().join("refractor-usages-elsewhere.rs");
    let list = UsageList::gather(
        heading(),
        &[place(&elsewhere, 0, 0)],
        &scratch.0,
        spot(),
        &ascii,
        &no_buffers,
    );
    assert_eq!(&*rows(&list)[0].file, elsewhere.display().to_string());
    let none = UsageList::gather(heading(), &[], &scratch.0, spot(), &ascii, &no_buffers);
    assert!(none.is_empty() && none.len() == 0 && none.entries.is_empty());
}

#[test]
fn only_so_many_places_are_listed_and_the_rest_are_counted() {
    let scratch = Scratch::new("usages-most");
    let file = scratch.file("a.rs", "x\n");
    let places: Vec<Place> = (0..UsageList::MOST_ROWS as u32 + 25)
        .map(|line| place(&file, line, 0))
        .collect();
    let list = UsageList::gather(heading(), &places, &scratch.0, spot(), &ascii, &no_buffers);
    assert_eq!((list.len(), list.omitted), (UsageList::MOST_ROWS, 25));
    assert_eq!(
        list.entries.len(),
        UsageList::MOST_ROWS + 1,
        "the note is a line after the places"
    );
    assert_eq!(list.entries.last(), Some(&UsageEntry::More(25)));
    assert_eq!(
        list.place_at(UsageList::MOST_ROWS),
        None,
        "and is not a place the keys can go to"
    );
}

const FILE: &str = "/project/src/main.rs";
const TEXT: &str = "pub struct Rules;\npub fn make() {}\nfn other() {}\n";

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

/// A pane's cell, whose file is open in a server that is ready, and what draws it again.
struct Pane {
    venus: Rc<Venus>,
    fake: Fake,
    usages: Rc<Usages>,
    controller: TextEditingController,
    /// How many times the pane was told to draw again, for the labels, and the card, for the list.
    drawn: Rc<Cell<usize>>,
    list_drawn: Rc<Cell<usize>>,
}

fn pane() -> Pane {
    let venus = installed();
    let fake = Fake::new();
    let lsp = lsp_with(&fake);
    let controller = TextEditingController::with_text(TEXT);
    lsp.open(Path::new(FILE), &controller, Rc::new(|| {}));
    settle(&venus);
    let _ = fake.requests();
    fake.answer(1, json!({"capabilities": {"textDocumentSync": 2, "positionEncoding": "utf-8", "codeLensProvider": {"resolveProvider": true}}}));
    settle(&venus);
    let _ = fake.requests();
    let usages = Rc::new(Usages::default());
    usages.attach(
        lsp,
        PathBuf::from(FILE),
        PathBuf::from("/project"),
        controller.clone(),
        LinkColors {
            rest: Color::Rgba(120, 124, 135, 255),
            hover: Color::Rgba(250, 250, 250, 255),
        },
        ThemeMode::Dark,
        Rc::new(no_buffers),
    );
    let (drawn, list_drawn) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    let (counted, list_counted) = (Rc::clone(&drawn), Rc::clone(&list_drawn));
    usages.on_labels_change(Rc::new(move || {
        counted.set(counted.get() + 1);
        true
    }));
    usages.on_list_change(Rc::new(move || {
        list_counted.set(list_counted.get() + 1);
        true
    }));
    Pane {
        venus,
        fake,
        usages,
        controller,
        drawn,
        list_drawn,
    }
}

impl Pane {
    /// A pane with a list open and no key pressed.
    fn fresh_list() -> Pane {
        let pane = pane();
        pane.load();
        assert!(pane.usages.show_usages_at(11, spot()));
        pane
    }

    /// A pane whose server has not answered `initialize`, so that nothing can be asked.
    fn not_ready() -> Pane {
        let venus = installed();
        let fake = Fake::new();
        let lsp = lsp_with(&fake);
        let controller = TextEditingController::with_text(TEXT);
        lsp.open(Path::new(FILE), &controller, Rc::new(|| {}));
        settle(&venus);
        let usages = Rc::new(Usages::default());
        usages.attach(
            lsp,
            PathBuf::from(FILE),
            PathBuf::from("/project"),
            controller.clone(),
            LinkColors {
                rest: Color::Rgba(1, 2, 3, 255),
                hover: Color::Rgba(9, 9, 9, 255),
            },
            ThemeMode::Dark,
            Rc::new(no_buffers),
        );
        Pane {
            venus,
            fake,
            usages,
            controller,
            drawn: Rc::default(),
            list_drawn: Rc::default(),
        }
    }

    /// Has the server answer the lenses of the file: the struct has two usages and an implementation, the function one usage,
    /// and the other function none.
    fn load(&self) {
        self.usages.schedule(Duration::ZERO);
        settle(&self.venus);
        let list = self.fake.requests();
        assert_eq!(list.len(), 1, "one request for the list: {list:?}");
        self.fake.answer(
            list[0].2.unwrap(),
            json!([
                unresolved(0, 11, "impls"),
                unresolved(0, 11, "references"),
                unresolved(1, 7, "references"),
                unresolved(2, 3, "references")
            ]),
        );
        settle(&self.venus);
        for (method, lens, id) in self.fake.requests() {
            assert_eq!(method, "codeLens/resolve");
            let line = lens["range"]["start"]["line"].as_u64().unwrap() as u32;
            let character = lens["range"]["start"]["character"]
                .as_u64()
                .unwrap() as u32;
            let answer = match (
                line,
                lens["data"]["kind"]
                    .as_object()
                    .unwrap()
                    .contains_key("impls"),
            ) {
                (0, true) => resolved(0, character, "1 implementation", vec![location(1)]),
                (0, false) => {
                    resolved(0, character, "2 references", vec![location(1), location(2)])
                }
                (1, _) => resolved(1, character, "1 reference", vec![location(2)]),
                _ => resolved(2, character, "0 references", vec![]),
            };
            self.fake.answer(id.unwrap(), answer);
        }
        settle(&self.venus);
    }
}

#[test]
fn the_labels_are_what_the_server_counted_for_the_lines_of_the_definitions_and_the_pane_is_drawn_again()
 {
    let pane = pane();
    assert!(
        pane.usages.label_of(0).is_none(),
        "nothing before the server has said"
    );
    pane.load();
    assert_eq!(
        pane.drawn.get(),
        1,
        "the pane is drawn again when the counts are in"
    );
    let label = |line: usize| {
        pane.usages
            .label_of(line)
            .map(|label| label.plain_text())
    };
    assert_eq!(label(0).as_deref(), Some("2 usages | 1 implementation"));
    assert_eq!(label(1).as_deref(), Some("1 usage"));
    assert_eq!(label(2).as_deref(), Some("no usages"));
    assert_eq!(label(3), None, "a line that is not a definition");
    assert!(
        Rc::ptr_eq(
            &pane.usages.label_of(0).unwrap(),
            &pane.usages.label_of(0).unwrap()
        ),
        "made once, and not for each frame"
    );
    assert_eq!(
        pane.usages.label_of(0).unwrap().font(),
        Some(crate::ui::fonts::ui_family()),
        "the counts are not code: they are in the font of the window and not of the text they follow"
    );
    assert_eq!(
        pane.usages.label_of(0).unwrap().hover(),
        Some(Color::Rgba(250, 250, 250, 255)),
        "and brighter under the pointer"
    );
}

#[test]
fn the_labels_are_not_shown_after_an_edit_that_moved_the_lines_and_are_again_when_the_text_is_asked_for_again()
 {
    let pane = pane();
    pane.load();
    assert!(pane.usages.label_of(0).is_some());
    // A word typed in a line moves nothing.
    pane.controller.replace_range(4..4, "x");
    assert!(
        pane.usages.label_of(0).is_some(),
        "the lines are where they were"
    );
    // A line break moves all that follow it.
    pane.controller.replace_range(0..0, "\n");
    assert!(pane.usages.label_of(0).is_none() && pane.usages.label_of(1).is_none());
    pane.usages.schedule(Duration::ZERO);
    settle(&pane.venus);
    assert_eq!(
        pane.fake
            .requests()
            .iter()
            .filter(|(method, _, _)| method == "textDocument/codeLens")
            .count(),
        1
    );
}

#[test]
fn pressing_a_count_opens_the_list_of_what_it_counted_beside_it_and_closing_it_draws_again() {
    let pane = pane();
    pane.load();
    let drawn = pane.list_drawn.get();
    let label = pane.usages.label_of(0).unwrap();
    assert!(pane.usages.list().is_none());
    assert!(label.press(0, spot()), "the count is a button");
    let list = pane.usages.list().expect("the list is open");
    assert_eq!(
        (
            list.heading.title().as_str(),
            list.heading.count_text().as_str()
        ),
        ("Struct Rules", "2 usages")
    );
    assert_eq!(list.spot, spot());
    assert_eq!(list.len(), 2);
    assert_eq!(
        pane.list_drawn.get(),
        drawn + 1,
        "the card is drawn, and not the pane"
    );
    assert_eq!(pane.drawn.get(), 1, "which was drawn for the labels only");
    // The implementations are the other button: after the bar, which is not one.
    assert!(!label.press(1, spot()), "the bar is not");
    assert!(label.press(2, spot()), "and the other count is");
    assert_eq!(
        pane.usages.list().unwrap().heading.count_text(),
        "1 implementation"
    );
    pane.usages.close();
    assert!(pane.usages.list().is_none());
    pane.usages.close();
    assert_eq!(
        pane.list_drawn.get(),
        drawn + 3,
        "shut once, and a list that is not open is not shut again"
    );
}

#[test]
fn a_force_click_on_a_definition_lists_its_usages_and_one_on_a_name_that_has_none_to_list_does_not()
{
    let pane = pane();
    pane.load();
    assert!(
        pane.usages.show_usages_at(11, spot()),
        "the struct, which starts at byte 11"
    );
    assert_eq!(pane.usages.list().unwrap().heading.count_text(), "2 usages");
    pane.usages.close();
    assert!(
        !pane.usages.show_usages_at(12, spot()),
        "not where a name starts"
    );
    assert!(pane.usages.list().is_none());
    // The function that nothing uses has a lens and no places: there is nothing to list.
    let other = TEXT.find("other").unwrap();
    assert!(pane.usages.show_usages_at(other, spot()));
    assert!(pane.usages.list().is_none(), "no usages, no list");
}

#[test]
fn a_refresh_that_is_asked_for_again_before_its_time_is_one_and_one_that_cannot_be_answered_keeps_what_there_was()
 {
    let pane = pane();
    pane.load();
    pane.usages.schedule(Duration::ZERO);
    pane.usages.schedule(Duration::ZERO);
    pane.usages.schedule(Duration::ZERO);
    settle(&pane.venus);
    let asked = pane.fake.requests();
    assert_eq!(
        asked
            .iter()
            .filter(|(method, _, _)| method == "textDocument/codeLens")
            .count(),
        1,
        "{asked:?}"
    );
    pane.fake.fail(asked[0].2.unwrap());
    settle(&pane.venus);
    assert!(pane.usages.label_of(0).is_some(), "what was there is kept");
}

#[test]
fn a_new_color_makes_the_labels_again_and_a_server_that_becomes_idle_asks_for_the_lenses() {
    let pane = pane();
    pane.load();
    let first = pane.usages.label_of(0).unwrap();
    pane.usages.attach(
        lsp_of(&pane),
        PathBuf::from(FILE),
        PathBuf::from("/project"),
        pane.controller.clone(),
        LinkColors {
            rest: Color::Rgba(1, 2, 3, 255),
            hover: Color::Rgba(9, 9, 9, 255),
        },
        ThemeMode::Dark,
        Rc::new(no_buffers),
    );
    assert!(
        !Rc::ptr_eq(&first, &pane.usages.label_of(0).unwrap()),
        "made again in the new color"
    );
    // The server is ready and has nothing it is busy with: that is the moment to ask, once.
    pane.usages.watch_server();
    pane.usages.watch_server();
    settle(&pane.venus);
    let asked = pane.fake.requests();
    assert_eq!(
        asked
            .iter()
            .filter(|(method, _, _)| method == "textDocument/codeLens")
            .count(),
        1,
        "{asked:?}"
    );
}

fn lsp_of(pane: &Pane) -> Lsp {
    pane.usages
        .context
        .borrow()
        .as_ref()
        .unwrap()
        .lsp
        .clone()
}

#[test]
fn the_counts_are_asked_for_again_while_the_server_cannot_say_or_is_busy_and_not_for_ever() {
    let delay = Usages::retry_delay;
    assert_eq!(
        delay(true, true, 0),
        Some(Usages::RETRY_DELAY),
        "it could not say"
    );
    assert_eq!(
        delay(false, false, 0),
        Some(Usages::RETRY_DELAY),
        "it said while it was still loading the project: not all is counted"
    );
    assert_eq!(delay(true, false, 5), Some(Usages::RETRY_DELAY));
    assert_eq!(
        delay(false, true, 0),
        None,
        "it said, and it is idle: that is the count"
    );
    assert_eq!(
        delay(true, true, Usages::MOST_RETRIES),
        None,
        "not for ever"
    );
    assert_eq!(
        delay(false, false, Usages::MOST_RETRIES - 1),
        Some(Usages::RETRY_DELAY)
    );
}

#[test]
fn a_load_that_the_server_could_not_make_is_counted_and_a_load_the_text_asks_for_starts_the_count_again()
 {
    let pane = Pane::not_ready();
    pane.usages.schedule(Duration::ZERO);
    settle(&pane.venus);
    assert!(pane.usages.attempts.get() > 0, "it was tried again");
    assert!(
        pane.usages.attempts.get() <= Usages::MOST_RETRIES,
        "and not for ever"
    );
    pane.usages.schedule(Duration::ZERO);
    assert_eq!(
        pane.usages.attempts.get(),
        0,
        "an edit that settled asks as for the first time"
    );
}

#[test]
fn the_name_of_a_definition_is_the_word_that_starts_at_its_byte() {
    let text = TextSnapshot::from_text("pub struct Rules_2;\n");
    assert_eq!(Usages::name_at(&text, 11), "Rules_2");
    assert_eq!(Usages::name_at(&text, 1000), "");
    assert_eq!(Usages::name_at(&TextSnapshot::from_text(""), 0), "");
}

#[test]
fn find_usages_on_a_definition_that_has_a_count_opens_its_list_without_asking_the_server_again() {
    let pane = pane();
    pane.load();
    pane.usages.find_usages_at(11, spot());
    assert_eq!(
        pane.usages
            .list()
            .expect("opened at once")
            .heading
            .count_text(),
        "2 usages"
    );
    assert!(
        pane.fake.requests().is_empty(),
        "nothing was asked: the count already had the places"
    );
}

#[test]
fn find_usages_on_any_other_name_asks_the_server_for_the_places_and_opens_them_when_they_come() {
    let pane = pane();
    pane.load();
    // From the middle of the name: no count starts there.
    pane.usages.find_usages_at(13, spot());
    assert!(
        pane.usages.list().is_none(),
        "not before the server has answered"
    );
    let asked = pane.fake.requests();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].0, "textDocument/references");
    assert_eq!(asked[0].1["position"], json!({"line": 0, "character": 13}));

    pane.fake
        .answer(asked[0].2.unwrap(), json!([location(1), location(2)]));
    settle(&pane.venus);

    let list = pane.usages.list().expect("the list is open");
    assert_eq!(list.len(), 2);
    assert_eq!(
        list.heading.name, "les",
        "the name is read from the byte it was asked at: the menu asks at where the name starts"
    );
    assert_eq!(pane.usages.selected(), Some(0));
    assert_eq!(pane.list_drawn.get(), 1, "the card is drawn once");
}

#[test]
fn find_usages_that_finds_none_opens_nothing_and_so_does_a_server_that_cannot_say() {
    let pane = pane();
    pane.load();
    for answer in [Some(json!([])), Some(Value::Null), None] {
        pane.usages.find_usages_at(13, spot());
        let id = pane.fake.requests()[0].2.unwrap();
        match answer {
            Some(value) => pane.fake.answer(id, value),
            None => pane.fake.fail(id),
        }
        settle(&pane.venus);
        assert!(pane.usages.list().is_none());
    }
}

#[test]
fn an_answer_that_comes_after_the_pane_is_gone_is_not_shown() {
    let pane = pane();
    pane.load();
    pane.usages.find_usages_at(13, spot());
    let id = pane.fake.requests()[0].2.unwrap();
    let usages = Rc::downgrade(&pane.usages);
    let fake = pane.fake.clone();
    let venus = Rc::clone(&pane.venus);
    drop(pane.usages);
    fake.answer(id, json!([location(1)]));
    settle(&venus);
    assert!(usages.upgrade().is_none());
}
