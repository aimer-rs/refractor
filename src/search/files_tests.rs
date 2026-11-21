use std::sync::atomic::AtomicBool;

use super::*;
use crate::search::Pattern;
use crate::search::scratch::Scratch;

fn index(dir: &Scratch) -> FileIndex {
    FileIndex::build(dir.path(), 10_000, &AtomicBool::new(false))
}

fn names(hits: &[FileHit]) -> Vec<&str> {
    hits.iter().map(|hit| hit.name.as_str()).collect()
}

#[test]
fn it_lists_the_files_and_not_what_the_ignore_files_leave_out() {
    let dir = Scratch::new("files-list");
    dir.file(".gitignore", "target/\n");
    dir.file("src/lib.rs", "");
    dir.file("src/request.rs", "");
    dir.file("target/debug/out.rs", "");
    dir.file(".git/config", "");
    let index = index(&dir);
    // `.gitignore` is a hidden file, so it is left out as well: what is left is the two files of `src`.
    assert_eq!(index.len(), 2, "{index:?}");
    let hits = index.find(&Pattern::new(".rs"), 10);
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().all(|hit| hit.path.starts_with(dir.path())));
}

#[test]
fn a_file_is_found_by_the_letters_of_its_name_and_the_folder_is_its_detail() {
    let dir = Scratch::new("files-find");
    dir.file("src/pool/config_pooling_tests.rs", "");
    dir.file("src/request.rs", "");
    dir.file("README.md", "");
    let hits = index(&dir).find(&Pattern::new("req"), 10);
    assert_eq!(names(&hits), ["request.rs"]);
    assert_eq!(hits[0].directory, "src");
    assert_eq!(&hits[0].name[hits[0].marked[0].clone()], "req");
    let hits = index(&dir).find(&Pattern::new("cpt"), 10);
    assert_eq!(names(&hits), ["config_pooling_tests.rs"]);
    assert_eq!(hits[0].directory, "src/pool");
}

#[test]
fn a_file_at_the_root_has_no_folder() {
    let dir = Scratch::new("files-root");
    dir.file("Cargo.toml", "");
    let hits = index(&dir).find(&Pattern::new("cargo"), 10);
    assert_eq!(hits[0].directory, "");
}

#[test]
fn the_better_name_comes_first() {
    let dir = Scratch::new("files-order");
    dir.file("a/request_ext.rs", "");
    dir.file("b/request.rs", "");
    dir.file("c/my_request.rs", "");
    let hits = index(&dir).find(&Pattern::new("request"), 10);
    assert_eq!(names(&hits), ["request.rs", "request_ext.rs", "my_request.rs"]);
}

#[test]
fn a_query_with_a_folder_in_it_is_matched_against_the_whole_path() {
    let dir = Scratch::new("files-path");
    dir.file("src/pool/mod.rs", "");
    dir.file("src/other/mod.rs", "");
    let hits = index(&dir).find(&Pattern::new("pool/mod"), 10);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].directory, "src/pool");
}

#[test]
fn the_empty_query_finds_nothing_and_the_limit_is_kept() {
    let dir = Scratch::new("files-limit");
    for n in 0..8 {
        dir.file(&format!("f{n}.rs"), "");
    }
    let index = index(&dir);
    assert!(index.find(&Pattern::new(""), 10).is_empty());
    assert_eq!(index.find(&Pattern::new("f"), 3).len(), 3);
}

#[test]
fn the_index_stops_at_its_limit_and_says_so() {
    let dir = Scratch::new("files-cap");
    for n in 0..8 {
        dir.file(&format!("f{n}.rs"), "");
    }
    let index = FileIndex::build(dir.path(), 5, &AtomicBool::new(false));
    assert_eq!(index.len(), 5);
    assert!(index.is_truncated());
}

#[test]
fn a_cancelled_index_is_empty_and_a_missing_folder_has_no_files() {
    let dir = Scratch::new("files-cancel");
    dir.file("a.rs", "");
    assert_eq!(FileIndex::build(dir.path(), 10, &AtomicBool::new(true)).len(), 0);
    assert_eq!(FileIndex::build(&dir.path().join("gone"), 10, &AtomicBool::new(false)).len(), 0);
}

#[test]
fn names_that_are_not_ascii_are_found() {
    let dir = Scratch::new("files-unicode");
    dir.file("café.txt", "");
    let hits = index(&dir).find(&Pattern::new("CAF"), 10);
    assert_eq!(names(&hits), ["café.txt"]);
}
