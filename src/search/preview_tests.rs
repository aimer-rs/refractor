use std::path::PathBuf;

use super::*;
use crate::lsp::{Place, Position};
use crate::search::ActionId;
use crate::search::scratch::Scratch;

fn numbered(count: usize) -> String {
    (1..=count)
        .map(|n| format!("line {n}\n"))
        .collect()
}

fn at(path: PathBuf, line: usize) -> Target {
    Target::File {
        path,
        at: Some((line, 0)),
    }
}

#[test]
fn it_shows_the_lines_around_the_one_that_is_gone_to() {
    let dir = Scratch::new("preview-around");
    let path = dir.file("a.txt", numbered(100));
    let preview = Preview::of(&at(path, 50)).expect("a preview");
    assert_eq!(preview.focus, Some(50));
    assert_eq!(preview.first, 50 - Preview::BEFORE);
    assert_eq!(preview.lines.len(), Preview::BEFORE + 1 + Preview::AFTER);
    assert_eq!(preview.lines[Preview::BEFORE], "line 50");
    assert_eq!(preview.lines[0], format!("line {}", 50 - Preview::BEFORE));
}

#[test]
fn near_the_start_and_the_end_the_window_is_cut_short_and_not_moved() {
    let dir = Scratch::new("preview-edges");
    let path = dir.file("a.txt", numbered(10));
    let start = Preview::of(&at(path.clone(), 2)).unwrap();
    assert_eq!((start.first, start.focus), (1, Some(2)));
    assert_eq!(start.lines.len(), 10, "everything there is");
    let end = Preview::of(&at(path, 10)).unwrap();
    assert_eq!(end.lines.last().map(String::as_str), Some("line 10"));
    assert_eq!(end.focus, Some(10));
}

#[test]
fn a_file_without_a_line_to_go_to_is_shown_from_its_start_with_nothing_chosen() {
    let dir = Scratch::new("preview-start");
    let path = dir.file("a.txt", numbered(100));
    let preview = Preview::of(&Target::File { path, at: None }).unwrap();
    assert_eq!((preview.first, preview.focus), (1, None));
    assert_eq!(preview.lines.len(), Preview::BEFORE + 1 + Preview::AFTER);
}

#[test]
fn a_place_of_the_server_is_a_line_counted_from_zero() {
    let dir = Scratch::new("preview-place");
    let path = dir.file("a.rs", numbered(20));
    let place = Place {
        path,
        at: Position {
            line: 4,
            character: 0,
        },
        end: Position {
            line: 4,
            character: 4,
        },
    };
    let preview = Preview::of(&Target::Place(place)).unwrap();
    assert_eq!(preview.focus, Some(5));
    assert_eq!(
        preview.lines[preview.focus.unwrap() - preview.first],
        "line 5"
    );
}

#[test]
fn a_line_past_the_end_shows_the_end_with_nothing_chosen() {
    let dir = Scratch::new("preview-past");
    let path = dir.file("a.txt", numbered(5));
    let preview = Preview::of(&at(path, 500)).unwrap();
    assert_eq!(preview.focus, None);
    assert_eq!(preview.lines.last().map(String::as_str), Some("line 5"));
}

#[test]
fn nothing_is_shown_for_an_action_a_missing_file_an_empty_file_or_a_huge_one() {
    let dir = Scratch::new("preview-none");
    assert!(Preview::of(&Target::Action(ActionId::SaveFile)).is_none());
    assert!(Preview::of(&at(dir.path().join("gone.txt"), 1)).is_none());
    assert!(Preview::of(&at(dir.file("empty.txt", ""), 1)).is_none());
    let huge = dir.file("huge.txt", vec![b'x'; (Preview::LARGEST_FILE + 1) as usize]);
    assert!(Preview::of(&at(huge, 1)).is_none());
}

#[test]
fn line_breaks_are_not_in_the_lines_and_a_long_line_is_cut() {
    let dir = Scratch::new("preview-lines");
    let long = "x".repeat(10_000);
    let path = dir.file(
        "a.txt",
        format!("one\r\ntwo\r\n{long}\nlast without a break"),
    );
    let preview = Preview::of(&at(path, 1)).unwrap();
    assert_eq!(preview.lines[0], "one");
    assert_eq!(preview.lines[1], "two");
    assert!(
        preview.lines[2].len() <= Preview::LINE,
        "{}",
        preview.lines[2].len()
    );
    assert_eq!(preview.lines[3], "last without a break");
}

#[test]
fn text_that_is_not_utf8_is_shown_with_what_cannot_be_read_replaced() {
    let dir = Scratch::new("preview-bytes");
    let path = dir.file("a.txt", b"caf\xff\nok\n");
    let preview = Preview::of(&at(path, 1)).unwrap();
    assert_eq!(preview.lines[1], "ok");
    assert!(preview.lines[0].starts_with("caf"));
}
