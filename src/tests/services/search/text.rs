use std::sync::atomic::{AtomicBool, Ordering};

use super::*;
use crate::services::search::scratch::Scratch;

fn run(root: &Scratch, query: &str) -> Found {
    TextSearch::new(query).run(root.path(), 100, &AtomicBool::new(false))
}

fn lines(found: &Found) -> Vec<(String, usize)> {
    found
        .hits
        .iter()
        .map(|hit| {
            (
                hit.path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                hit.line,
            )
        })
        .collect()
}

#[test]
fn it_finds_each_line_that_has_the_text_with_its_line_and_column() {
    let dir = Scratch::new("text-basic");
    dir.file("a.rs", "fn main() {}\nlet request = 1;\n");
    dir.file("b.rs", "nothing here\n  request again\n");
    let found = run(&dir, "request");
    assert_eq!(
        lines(&found),
        [("a.rs".to_owned(), 2), ("b.rs".to_owned(), 2)]
    );
    let first = &found.hits[0];
    assert_eq!(
        (first.column, first.preview.as_str()),
        (4, "let request = 1;")
    );
    assert_eq!(&first.preview[first.marked.clone()], "request");
    // The leading blanks are not in the preview, and the mark is of the preview.
    let second = &found.hits[1];
    assert_eq!(second.preview, "request again");
    assert_eq!(second.column, 2);
    assert_eq!(&second.preview[second.marked.clone()], "request");
    assert!(!found.truncated);
}

#[test]
fn the_text_is_literal_and_not_a_pattern() {
    let dir = Scratch::new("text-literal");
    dir.file("a.txt", "a.c\nabc\n(x\n");
    assert_eq!(lines(&run(&dir, "a.c")), [("a.txt".to_owned(), 1)]);
    // A bracket that a pattern would not parse is only a character.
    assert_eq!(lines(&run(&dir, "(x")), [("a.txt".to_owned(), 3)]);
}

#[test]
fn case_is_ignored_unless_the_query_has_a_capital() {
    let dir = Scratch::new("text-case");
    dir.file("a.txt", "Request\nrequest\nREQUEST\n");
    assert_eq!(run(&dir, "request").hits.len(), 3);
    assert_eq!(lines(&run(&dir, "Request")), [("a.txt".to_owned(), 1)]);
}

#[test]
fn what_the_ignore_files_leave_out_is_left_out() {
    let dir = Scratch::new("text-ignore");
    dir.file(".gitignore", "target/\n");
    dir.file("target/out.rs", "request\n");
    dir.file(".hidden/secret.rs", "request\n");
    dir.file("src/lib.rs", "request\n");
    assert_eq!(lines(&run(&dir, "request")), [("lib.rs".to_owned(), 1)]);
}

#[test]
fn a_binary_file_is_skipped() {
    let dir = Scratch::new("text-binary");
    dir.file("image.bin", b"request\0\x01\x02 request");
    dir.file("text.txt", "request\n");
    assert_eq!(lines(&run(&dir, "request")), [("text.txt".to_owned(), 1)]);
}

#[test]
fn the_empty_query_and_a_blank_one_find_nothing() {
    let dir = Scratch::new("text-empty");
    dir.file("a.txt", "a b\n");
    assert!(run(&dir, "").hits.is_empty());
    assert!(run(&dir, "   ").hits.is_empty());
}

#[test]
fn an_empty_folder_and_a_folder_that_is_not_there_have_nothing() {
    let dir = Scratch::new("text-nothing");
    assert!(run(&dir, "x").hits.is_empty());
    let gone = dir.path().join("gone");
    let found = TextSearch::new("x").run(&gone, 10, &AtomicBool::new(false));
    assert!(found.hits.is_empty());
}

#[test]
fn the_last_line_is_found_without_a_line_break_and_with_a_carriage_return() {
    let dir = Scratch::new("text-ends");
    dir.file("a.txt", "one\r\ntwo needle\r\nthree needle");
    let found = run(&dir, "needle");
    assert_eq!(
        found
            .hits
            .iter()
            .map(|hit| hit.line)
            .collect::<Vec<_>>(),
        [2, 3]
    );
    assert_eq!(found.hits[0].preview, "two needle");
    assert_eq!(found.hits[1].preview, "three needle");
}

#[test]
fn the_column_is_counted_in_bytes_and_the_mark_is_on_character_boundaries() {
    let dir = Scratch::new("text-multibyte");
    dir.file("a.txt", "héllo wörld naïve café\n");
    let found = run(&dir, "café");
    let hit = &found.hits[0];
    assert_eq!(hit.column, "héllo wörld naïve ".len());
    assert_eq!(&hit.preview[hit.marked.clone()], "café");
    // The combining mark that follows `e` is a character of its own that the match may not split off the letter.
    dir.file("b.txt", "cafe\u{301} cafe\n");
    let found = run(&dir, "cafe");
    assert!(
        found
            .hits
            .iter()
            .all(|hit| hit.preview.is_char_boundary(hit.marked.end))
    );
}

#[test]
fn a_very_long_line_is_cut_around_the_match() {
    let dir = Scratch::new("text-long");
    let line = format!("{}needle{}", "x".repeat(50_000), "y".repeat(50_000));
    dir.file("a.txt", line);
    let found = run(&dir, "needle");
    let hit = &found.hits[0];
    assert!(hit.preview.len() < 400, "{} bytes", hit.preview.len());
    assert_eq!(&hit.preview[hit.marked.clone()], "needle");
    assert_eq!(hit.column, 50_000);
}

#[test]
fn it_stops_at_the_limit_and_says_there_was_more() {
    let dir = Scratch::new("text-limit");
    dir.file("a.txt", "x\nx\nx\nx\nx\n");
    let found = TextSearch::new("x").run(dir.path(), 3, &AtomicBool::new(false));
    assert_eq!(found.hits.len(), 3);
    assert!(found.truncated);
    let found = TextSearch::new("x").run(dir.path(), 5, &AtomicBool::new(false));
    assert_eq!(found.hits.len(), 5);
    assert!(!found.truncated);
}

#[test]
fn a_cancelled_search_gives_up_at_once() {
    let dir = Scratch::new("text-cancel");
    for n in 0..20 {
        dir.file(&format!("f{n}.txt"), "x\n");
    }
    let found = TextSearch::new("x").run(dir.path(), 100, &AtomicBool::new(true));
    assert!(found.hits.is_empty());
    let cancel = AtomicBool::new(false);
    cancel.store(true, Ordering::Relaxed);
    assert!(
        TextSearch::new("x")
            .run(dir.path(), 100, &cancel)
            .hits
            .is_empty()
    );
}

#[test]
fn a_file_that_cannot_be_read_is_passed_over() {
    let dir = Scratch::new("text-unreadable");
    dir.file("a.txt", "x\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.file("locked.txt", "x\n");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        let found = run(&dir, "x");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            found
                .hits
                .iter()
                .any(|hit| hit.path.ends_with("a.txt"))
        );
    }
}
