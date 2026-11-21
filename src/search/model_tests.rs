use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use super::*;
use crate::lsp::Lsp;
use crate::search::Kind;
use crate::search::scratch::Scratch;

/// A server that is not there, which answers at once that it cannot say.
fn no_server(root: PathBuf) -> Lsp {
    Lsp::with_starter(root, Box::new(|_| Err(std::io::Error::other("no server"))), true)
}

fn search(dir: &Scratch) -> Rc<Search> {
    Search::new(dir.path().to_path_buf(), no_server(dir.path().to_path_buf()))
}

fn project() -> Scratch {
    let dir = Scratch::new("model");
    dir.file("src/request.rs", "pub struct Request;\nlet needle = 1;\n");
    dir.file("src/lib.rs", "mod request;\n");
    dir.file("README.md", "a needle in the text\n");
    dir
}

fn titles(search: &Search) -> Vec<String> {
    search.hits().iter().map(|hit| hit.title.clone()).collect()
}

#[test]
fn stepping_goes_round_the_ends_and_stays_put_for_an_empty_list() {
    assert_eq!(step(0, 3, 1), 1);
    assert_eq!(step(2, 3, 1), 0);
    assert_eq!(step(0, 3, -1), 2);
    assert_eq!(step(1, 3, -5), 2);
    assert_eq!(step(0, 0, 1), 0);
    assert_eq!(step(0, 0, -1), 0);
}

#[test]
fn nothing_typed_lists_nothing() {
    let dir = project();
    let search = search(&dir);
    assert!(search.hits().is_empty());
    search.set_query("   ");
    assert!(search.hits().is_empty());
    assert_eq!(search.current(), None);
}

#[test]
fn a_file_is_found_as_it_is_typed_and_the_first_is_chosen() {
    let dir = project();
    let search = search(&dir);
    search.set_tab(Tab::Files);
    search.set_query("req");
    assert_eq!(titles(&search), ["request.rs"]);
    assert_eq!(search.current().expect("chosen").title, "request.rs");
    search.set_query("zzz");
    assert!(search.hits().is_empty());
    assert_eq!(search.selected(), 0);
}

#[test]
fn the_text_tab_lists_the_lines_that_have_the_text() {
    let dir = project();
    let search = search(&dir);
    search.set_tab(Tab::Text);
    search.set_query("needle");
    let hits = search.hits();
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().all(|hit| hit.kind == Kind::Text));
    assert_eq!(hits[0].detail, "README.md:1");
    assert_eq!(hits[1].detail, "src/request.rs:2");
}

#[test]
fn one_letter_is_not_read_for_in_the_files() {
    let dir = project();
    let search = search(&dir);
    search.set_tab(Tab::Text);
    search.set_query("n");
    assert!(search.hits().is_empty());
    search.set_query("ne");
    assert!(!search.hits().is_empty());
}

#[test]
fn the_all_tab_has_files_actions_and_text_and_each_tab_changes_the_list() {
    let dir = project();
    let search = search(&dir);
    search.set_query("req");
    let all = search.hits();
    assert!(all.iter().any(|hit| hit.kind == Kind::File && hit.title == "request.rs"));
    search.set_tab(Tab::Actions);
    assert!(search.hits().is_empty());
    search.set_query("setti");
    assert_eq!(titles(&search), ["Open Settings"]);
    search.set_tab(Tab::Files);
    assert!(search.hits().is_empty());
}

#[test]
fn a_new_query_chooses_the_first_hit_again_and_the_choice_stays_in_the_list() {
    let dir = project();
    let search = search(&dir);
    search.set_tab(Tab::Files);
    search.set_query("re");
    assert_eq!(search.hits().len(), 2);
    search.move_selection(1);
    assert_eq!(search.selected(), 1);
    search.move_selection(1);
    assert_eq!(search.selected(), 0, "round the end");
    search.move_selection(-1);
    assert_eq!(search.selected(), 1);
    search.set_query("req");
    assert_eq!(search.hits().len(), 1);
    assert_eq!(search.selected(), 0);
    search.select(99);
    assert_eq!(search.selected(), 0, "there is no hit there");
}

#[test]
fn a_server_that_cannot_say_is_said_for_the_tabs_of_definitions_only() {
    let dir = project();
    let search = search(&dir);
    search.set_tab(Tab::Types);
    assert_eq!(search.note(), None, "nothing was asked yet");
    search.set_query("req");
    assert_eq!(search.note(), Some("The language server is not running: open a Rust file to start it"));
    assert!(!search.busy(), "it was told at once");
    search.set_tab(Tab::Files);
    assert_eq!(search.note(), None);
}

#[test]
fn the_redraw_is_called_when_the_list_or_the_choice_changes() {
    let dir = project();
    let search = search(&dir);
    let calls = Rc::new(Cell::new(0));
    let seen = Rc::clone(&calls);
    search.redraw().add(Rc::new(move || {
        seen.set(seen.get() + 1);
        true
    }));
    search.set_tab(Tab::Files);
    let after_tab = calls.get();
    assert!(after_tab > 0);
    search.set_query("req");
    assert!(calls.get() > after_tab);
    let before = calls.get();
    search.set_query("req");
    assert_eq!(calls.get(), before, "the same query is not looked for again");
    search.set_shift(true);
    search.set_shift(true);
    assert_eq!(calls.get(), before + 1, "only a change is a change");
}

#[test]
fn the_preview_is_of_the_chosen_hit_and_is_read_again_when_the_choice_moves() {
    let dir = project();
    let search = search(&dir);
    search.set_tab(Tab::Text);
    search.set_query("needle");
    let first = search.preview().expect("a file to show");
    assert!(first.path.ends_with("README.md"));
    assert!(Rc::ptr_eq(&first, &search.preview().expect("kept")), "read once for the same hit");
    search.move_selection(1);
    let second = search.preview().expect("another file");
    assert!(second.path.ends_with("request.rs"));
    assert_eq!(second.focus, Some(2));
    search.set_tab(Tab::Actions);
    assert_eq!(search.preview(), None, "no hit, no preview");
}
