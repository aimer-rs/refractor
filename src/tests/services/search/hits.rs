use std::path::PathBuf;

use super::*;
use crate::services::lsp::{Place, Position};

fn place(path: &str) -> Place {
    Place {
        path: PathBuf::from(path),
        at: Position {
            line: 4,
            character: 2,
        },
        end: Position {
            line: 4,
            character: 9,
        },
    }
}

fn symbol(name: &str, kind: SymbolKind, container: Option<&str>) -> Symbol {
    Symbol {
        name: name.to_owned(),
        kind,
        container: container.map(str::to_owned),
        place: place("/p/src/a.rs"),
    }
}

fn file(name: &str, directory: &str) -> FileHit {
    FileHit {
        path: PathBuf::from(format!("/p/{directory}/{name}")),
        name: name.to_owned(),
        directory: directory.to_owned(),
        marked: vec![0..3],
        score: 40,
    }
}

fn text(path: &str, line: usize, preview: &str) -> TextHit {
    TextHit {
        path: PathBuf::from(path),
        line,
        column: 4,
        preview: preview.to_owned(),
        marked: 0..3,
    }
}

fn titles(hits: &[Hit]) -> Vec<&str> {
    hits.iter()
        .map(|hit| hit.title.as_str())
        .collect()
}

fn parts() -> Parts {
    Parts {
        root: PathBuf::from("/p"),
        symbols: vec![
            symbol("RequestBuilder", SymbolKind::Struct, Some("ureq::request")),
            symbol("request", SymbolKind::Function, Some("ureq")),
            symbol("RequestExt", SymbolKind::Trait, None),
        ],
        files: vec![file("request.rs", "src")],
        actions: vec![],
        text: vec![text("/p/src/a.rs", 12, "let request = 1;")],
        pattern: Pattern::new("request"),
        at: None,
    }
}

#[test]
fn a_file_hit_goes_to_the_line_and_column_that_were_typed() {
    let mut parts = parts();
    parts.at = Some((109, 4));
    let files = Tab::Files.combine(&parts);
    assert_eq!(
        files[0].target,
        Target::File {
            path: PathBuf::from("/p/src/request.rs"),
            at: Some((109, 4))
        }
    );
}

#[test]
fn each_tab_shows_its_own_kind_of_hit() {
    let parts = parts();
    // The shorter name fits the letters better, so it is first.
    assert_eq!(
        titles(&Tab::Types.combine(&parts)),
        ["RequestExt", "RequestBuilder"]
    );
    assert_eq!(titles(&Tab::Symbols.combine(&parts)), ["request"]);
    assert_eq!(titles(&Tab::Files.combine(&parts)), ["request.rs"]);
    assert_eq!(titles(&Tab::Text.combine(&parts)), ["let request = 1;"]);
    assert!(Tab::Actions.combine(&parts).is_empty());
}

#[test]
fn the_all_tab_has_names_by_how_well_they_fit_and_the_text_last() {
    let all = Tab::All.combine(&parts());
    assert_eq!(all.last().map(|hit| &hit.kind), Some(&Kind::Text));
    // Four names, and then the text: the names are ordered by score, the best first.
    assert_eq!(all.len(), 5);
    let scores: Vec<i32> = all[..4].iter().map(|hit| hit.score).collect();
    let mut sorted = scores.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(scores, sorted);
    // The definition that is exactly the name is the best of them, and a file that begins with it comes before the rest.
    assert_eq!(all[0].title, "request");
}

#[test]
fn a_hit_says_where_it_goes_and_what_it_is_in() {
    let parts = parts();
    let types = Tab::Types.combine(&parts);
    let builder = types
        .iter()
        .find(|hit| hit.title == "RequestBuilder")
        .expect("a struct");
    assert_eq!(builder.detail, "in ureq::request");
    assert_eq!(builder.kind, Kind::Symbol(SymbolKind::Struct));
    assert_eq!(builder.target, Target::Place(place("/p/src/a.rs")));
    let ext = types
        .iter()
        .find(|hit| hit.title == "RequestExt")
        .expect("a trait");
    assert_eq!(
        ext.detail, "src/a.rs",
        "a symbol that is in nothing is said to be where its file is"
    );
    let files = Tab::Files.combine(&parts);
    assert_eq!(files[0].detail, "src");
    assert_eq!(
        files[0].target,
        Target::File {
            path: PathBuf::from("/p/src/request.rs"),
            at: None
        }
    );
    assert_eq!(files[0].marked, vec![0..3]);
    let text = Tab::Text.combine(&parts);
    assert_eq!(text[0].detail, "src/a.rs:12");
    assert_eq!(
        text[0].target,
        Target::File {
            path: PathBuf::from("/p/src/a.rs"),
            at: Some((12, 4))
        }
    );
}

#[test]
fn a_symbol_is_marked_where_the_letters_are_and_not_at_all_when_they_are_not_there() {
    let mut parts = parts();
    parts
        .symbols
        .push(symbol("Unrelated", SymbolKind::Struct, None));
    let types = Tab::Types.combine(&parts);
    assert_eq!(types[0].marked, vec![0..7]);
    let last = types
        .last()
        .expect("the server's symbol is kept");
    assert_eq!(last.title, "Unrelated");
    assert!(last.marked.is_empty());
    assert!(
        last.score < types[0].score,
        "what does not fit the letters is the last"
    );
}

#[test]
fn the_all_tab_is_short_and_the_text_in_it_is_shorter() {
    let mut parts = parts();
    parts.symbols = (0..200)
        .map(|n| symbol(&format!("request_{n}"), SymbolKind::Function, None))
        .collect();
    parts.text = (0..200)
        .map(|n| text("/p/a.rs", n + 1, "request"))
        .collect();
    let all = Tab::All.combine(&parts);
    let names = all
        .iter()
        .filter(|hit| hit.kind != Kind::Text)
        .count();
    let texts = all
        .iter()
        .filter(|hit| hit.kind == Kind::Text)
        .count();
    assert_eq!((names, texts), (Tab::NAMES_IN_ALL, Tab::TEXT_IN_ALL));
    // The other tabs are not cut so short.
    assert_eq!(Tab::Text.combine(&parts).len(), 200);
}

#[test]
fn nothing_found_is_an_empty_list() {
    let parts = Parts {
        root: PathBuf::from("/p"),
        pattern: Pattern::new("x"),
        ..Parts::default()
    };
    for tab in Tab::ALL {
        assert!(tab.combine(&parts).is_empty(), "{tab:?}");
    }
}

#[test]
fn stepping_through_the_tabs_goes_round_the_ends() {
    assert_eq!(Tab::All.step(1), Tab::Types);
    assert_eq!(Tab::Text.step(1), Tab::All);
    assert_eq!(Tab::All.step(-1), Tab::Text);
    assert_eq!(Tab::Files.step(-1), Tab::Types);
    assert_eq!(Tab::Files.step(6), Tab::Files);
}

#[test]
fn the_tabs_have_titles_and_the_first_is_all() {
    assert_eq!(Tab::ALL[0], Tab::All);
    let titles: Vec<_> = Tab::ALL.iter().map(|tab| tab.title()).collect();
    assert_eq!(
        titles,
        ["All", "Types", "Files", "Symbols", "Actions", "Text"]
    );
    assert!(Tab::All.wants_text() && Tab::Text.wants_text() && !Tab::Files.wants_text());
    assert!(Tab::All.wants_symbols() && Tab::Types.wants_symbols() && Tab::Symbols.wants_symbols());
    assert!(!Tab::Actions.wants_symbols());
}
