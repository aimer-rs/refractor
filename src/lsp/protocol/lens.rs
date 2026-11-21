//! Code lenses, which rust-analyzer uses to say how many places use a definition or implement it.
//!
//! A server answers `textDocument/codeLens` with lenses that have no command yet, and each is asked for with
//! `codeLens/resolve`. What a resolved lens carries is a title such as "27 references" and, in the arguments of the
//! command `rust-analyzer.showReferences`, the places themselves: so one request for a lens gives both the count and
//! what to list when it is pressed.

use std::path::Path;

use serde_json::{Value, json};

use super::super::position::Position;
use super::{Place, from_position, path_of, uri_of};

/// The method that asks for the lenses of a file.
pub const CODE_LENS: &str = "textDocument/codeLens";
/// The method that asks for the command of a lens.
pub const CODE_LENS_RESOLVE: &str = "codeLens/resolve";
/// The command of a lens that lists places, which a client has to say that it can run.
pub const SHOW_REFERENCES: &str = "rust-analyzer.showReferences";

/// What a lens counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LensKind {
    /// The places that use the name.
    Usages,
    /// The places that implement it.
    Implementations,
}

/// A lens the server has found, and not yet said what it counts: what is to be sent back to ask.
#[derive(Clone, Debug, PartialEq)]
pub struct Unresolved {
    /// Where the lens is, which is the start of the name that it is about.
    pub at: Position,
    /// The lens as the server sent it.
    pub raw: Value,
}

/// A lens the server has said what it counts.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub at: Position,
    pub kind: LensKind,
    /// Where they are. A place that is not in a file is left out, so this can be shorter than the count the title gave.
    pub places: Vec<Place>,
}

/// What a client says it can do that is not in the protocol: run the command that lists places. rust-analyzer makes a lens
/// of that kind only for a client that says so.
pub fn client_commands() -> Value {
    json!({"commands": [SHOW_REFERENCES]})
}

/// The settings of the server that make the lenses: how many places use a type, a trait and a function, and how many
/// implement a type and a trait. The buttons that run or debug code are not wanted, nor is a count for each variant of an
/// enum.
pub fn server_options() -> Value {
    json!({"lens": {
        "enable": true,
        "run": {"enable": false},
        "debug": {"enable": false},
        "updateTest": {"enable": false},
        "references": {
            "adt": {"enable": true},
            "trait": {"enable": true},
            "method": {"enable": true},
            "enumVariant": {"enable": false},
        },
        "implementations": {"enable": true},
    }})
}

/// The parameters of `textDocument/codeLens` for the file `path`.
pub fn lens_params(path: &Path) -> Value {
    json!({"textDocument": {"uri": uri_of(path)}})
}

/// The lenses the server found that are still to be resolved. A lens that already has a command, such as one that runs a
/// test, is not one of these, and neither is what is not made as it should be.
pub fn unresolved(result: &Value) -> Vec<Unresolved> {
    let Some(lenses) = result.as_array() else {
        return Vec::new();
    };
    lenses
        .iter()
        .filter(|lens| lens.get("command").is_none_or(Value::is_null) && lens.get("data").is_some())
        .filter_map(|lens| {
            let start = lens.pointer("/range/start")?;
            Some(Unresolved {
                at: Position {
                    line: start.get("line")?.as_u64()?.try_into().ok()?,
                    character: start.get("character")?.as_u64()?.try_into().ok()?,
                },
                raw: lens.clone(),
            })
        })
        .collect()
}

/// What a resolved lens counts and where. `None` for one that does not list places, or is not made as it should be.
pub fn resolved(result: &Value) -> Option<Resolved> {
    let command = result.get("command")?;
    if command.get("command")?.as_str()? != SHOW_REFERENCES {
        return None;
    }
    let start = result.pointer("/range/start")?;
    let at = Position {
        line: start.get("line")?.as_u64()?.try_into().ok()?,
        character: start.get("character")?.as_u64()?.try_into().ok()?,
    };
    let title = command.get("title")?.as_str()?;
    let kind = if title.to_ascii_lowercase().contains("implementation") {
        LensKind::Implementations
    } else {
        LensKind::Usages
    };
    // The places are the third of the arguments: the file, the position, and where they are.
    let places = command
        .pointer("/arguments/2")?
        .as_array()?
        .iter()
        .filter_map(|location| {
            let location: lsp_types::Location = serde_json::from_value(location.clone()).ok()?;
            Some(Place {
                path: path_of(location.uri.as_str())?,
                at: from_position(location.range.start),
                end: from_position(location.range.end),
            })
        })
        .collect();
    Some(Resolved { at, kind, places })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// The lenses of a file as rust-analyzer sends them: two for the struct, one for a function that has none yet resolved,
    /// and one that already has a command, which runs a test.
    fn found() -> Value {
        json!([
            {"range": {"start": {"line": 1, "character": 11}, "end": {"line": 1, "character": 16}},
             "data": {"version": 1, "kind": {"impls": {"textDocument": {"uri": "file:///p/src/main.rs"}, "position": {"line": 1, "character": 11}}}}},
            {"range": {"start": {"line": 1, "character": 11}, "end": {"line": 1, "character": 16}},
             "data": {"version": 1, "kind": {"references": {"textDocument": {"uri": "file:///p/src/main.rs"}, "position": {"line": 1, "character": 11}}}}},
            {"range": {"start": {"line": 8, "character": 7}, "end": {"line": 8, "character": 11}},
             "command": {"title": "Run Test", "command": "rust-analyzer.runSingle", "arguments": []}},
        ])
    }

    #[test]
    fn the_server_is_told_that_the_lists_of_places_can_be_shown_and_which_lenses_are_wanted() {
        let params = super::super::initialize_params(Path::new("/p"), 1);
        assert_eq!(
            params["capabilities"]["experimental"]["commands"]["commands"],
            json!([SHOW_REFERENCES]),
            "a server only makes the lenses of commands that its client says it has"
        );
        assert_eq!(
            params["capabilities"]["experimental"]["serverStatusNotification"],
            json!(true),
            "what was said before is still said"
        );
        let lens = &params["initializationOptions"]["lens"];
        assert_eq!(lens["enable"], json!(true));
        for wanted in [
            &lens["references"]["adt"]["enable"],
            &lens["references"]["trait"]["enable"],
            &lens["references"]["method"]["enable"],
            &lens["implementations"]["enable"],
        ] {
            assert_eq!(wanted, &json!(true));
        }
        for unwanted in [
            &lens["run"]["enable"],
            &lens["debug"]["enable"],
            &lens["updateTest"]["enable"],
            &lens["references"]["enumVariant"]["enable"],
        ] {
            assert_eq!(unwanted, &json!(false), "buttons that run code, and a count for every variant, are not shown");
        }
    }

    #[test]
    fn the_lenses_of_a_file_are_asked_for_by_its_address() {
        assert_eq!(
            lens_params(&PathBuf::from("/p/src/main.rs")),
            json!({"textDocument": {"uri": "file:///p/src/main.rs"}})
        );
        assert_eq!(CODE_LENS, "textDocument/codeLens");
        assert_eq!(CODE_LENS_RESOLVE, "codeLens/resolve");
    }

    #[test]
    fn the_lenses_that_are_to_be_resolved_are_the_ones_without_a_command_and_keep_what_the_server_sent() {
        let lenses = unresolved(&found());
        assert_eq!(lenses.len(), 2, "the one that runs a test is not one");
        assert_eq!(lenses[0].at, Position { line: 1, character: 11 });
        assert_eq!(lenses[0].raw, found()[0], "the lens is sent back as it came");
        assert!(unresolved(&Value::Null).is_empty(), "no lenses");
        assert!(unresolved(&json!({"not": "a list"})).is_empty());
        assert!(unresolved(&json!([{"data": 1}])).is_empty(), "a lens with no place");
        assert!(unresolved(&json!([{"range": {"start": {"line": -1, "character": 0}}, "data": 1}])).is_empty());
    }

    fn lens_with(title: &str, locations: Value) -> Value {
        json!({"range": {"start": {"line": 1, "character": 11}, "end": {"line": 1, "character": 16}},
               "command": {"title": title, "command": "rust-analyzer.showReferences",
                           "arguments": ["file:///p/src/main.rs", {"line": 1, "character": 11}, locations]}})
    }

    #[test]
    fn a_resolved_lens_of_references_lists_where_they_are() {
        let lens = lens_with(
            "2 references",
            json!([
                {"uri": "file:///p/src/main.rs", "range": {"start": {"line": 7, "character": 15}, "end": {"line": 7, "character": 20}}},
                {"uri": "file:///p/src/other.rs", "range": {"start": {"line": 3, "character": 0}, "end": {"line": 3, "character": 5}}},
            ]),
        );
        let resolved = resolved(&lens).expect("a lens that lists places");
        assert_eq!(resolved.kind, LensKind::Usages);
        assert_eq!(resolved.at, Position { line: 1, character: 11 });
        assert_eq!(
            resolved.places,
            vec![
                Place {
                    path: PathBuf::from("/p/src/main.rs"),
                    at: Position { line: 7, character: 15 },
                    end: Position { line: 7, character: 20 },
                },
                Place {
                    path: PathBuf::from("/p/src/other.rs"),
                    at: Position { line: 3, character: 0 },
                    end: Position { line: 3, character: 5 },
                },
            ]
        );
    }

    #[test]
    fn a_lens_of_implementations_is_told_by_its_title_and_one_of_none_has_no_places() {
        let implementations = lens_with(
            "1 implementation",
            json!([{"uri": "file:///p/src/main.rs", "range": {"start": {"line": 7, "character": 0}, "end": {"line": 11, "character": 1}}}]),
        );
        assert_eq!(resolved(&implementations).unwrap().kind, LensKind::Implementations);
        assert_eq!(resolved(&lens_with("3 implementations", json!([]))).unwrap().kind, LensKind::Implementations);
        let none = resolved(&lens_with("0 references", json!([]))).unwrap();
        assert_eq!((none.kind, none.places.len()), (LensKind::Usages, 0));
    }

    #[test]
    fn what_is_not_a_lens_of_places_is_none_and_a_place_that_is_not_a_file_is_left_out() {
        let run = json!({"range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 1}},
                         "command": {"title": "Run", "command": "rust-analyzer.runSingle", "arguments": []}});
        assert_eq!(resolved(&run), None, "another command");
        assert_eq!(resolved(&json!({"range": {"start": {"line": 1, "character": 0}}})), None, "no command");
        assert_eq!(resolved(&Value::Null), None);
        let mut missing = lens_with("1 reference", json!([]));
        missing["command"]["arguments"] = json!(["file:///p/src/main.rs"]);
        assert_eq!(resolved(&missing), None, "no list of places");
        let mixed = lens_with(
            "2 references",
            json!([
                {"uri": "untitled:Scratch", "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}}},
                {"uri": "file:///p/a.rs", "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}}},
                {"not": "a location"},
            ]),
        );
        assert_eq!(resolved(&mixed).unwrap().places.len(), 1, "one is in a file and is made as it should be");
    }
}
