//! Inlay hints: the small notes a server has for the text of a file, such as the type of `x` in `let x = vec![1u32];`, the name
//! of a parameter at a call, or what a closing brace closes.
//!
//! A server answers `textDocument/inlayHint` with the hints of a range, each with the position it goes at and a label that is
//! a text or a list of parts of a text. The hints are not part of the file: they are drawn in the line and never edited.

use std::path::Path;

use serde_json::{Value, json};

use super::super::position::Position;
use super::uri_of;

/// The method that asks for the hints of a range of a file.
pub const INLAY_HINT: &str = "textDocument/inlayHint";

/// What a hint is a note of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HintKind {
    /// The type of a name: `: Vec<u32>`.
    Type,
    /// The name of a parameter, at a call: `count:`.
    Parameter,
    /// What the server did not say: a closing brace's, a lifetime's, a chain's.
    Other,
}

/// A hint as the server said it, with its position as the protocol has it, which the text it is about is needed to turn into
/// a byte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reported {
    /// Where it is drawn: before the character at this position.
    pub at: Position,
    /// What it says, with the parts of a label joined.
    pub label: String,
    pub kind: HintKind,
    /// Whether it is drawn a little apart from the text before it.
    pub pad_left: bool,
    /// Whether it is drawn a little apart from the text after it.
    pub pad_right: bool,
}

/// What a client says it can do about hints: nothing that needs the server to be asked again about one, so the labels are
/// taken as they come.
pub fn client_capability() -> Value {
    json!({"dynamicRegistration": false})
}

/// The parameters of `textDocument/inlayHint` for the range `start..end` of the file `path`. The range is in the text: the
/// protocol says that a position past the end is the end, but a server such as rust-analyzer answers an error to one.
pub fn hint_params(path: &Path, start: Position, end: Position) -> Value {
    json!({
        "textDocument": {"uri": uri_of(path)},
        "range": {
            "start": {"line": start.line, "character": start.character},
            "end": {"line": end.line, "character": end.character},
        },
    })
}

/// The hints a server sent, in the order it sent them. What is not made as it should be is left out.
pub fn hints(result: &Value) -> Vec<Reported> {
    let Some(hints) = result.as_array() else {
        return Vec::new();
    };
    hints.iter().filter_map(reported).collect()
}

/// One hint, or none for what has no position or says nothing.
fn reported(hint: &Value) -> Option<Reported> {
    let position = hint.get("position")?;
    let label = label_of(hint.get("label")?)?;
    Some(Reported {
        at: Position {
            line: position
                .get("line")?
                .as_u64()?
                .try_into()
                .ok()?,
            character: position
                .get("character")?
                .as_u64()?
                .try_into()
                .ok()?,
        },
        label,
        kind: match hint.get("kind").and_then(Value::as_u64) {
            Some(1) => HintKind::Type,
            Some(2) => HintKind::Parameter,
            _ => HintKind::Other,
        },
        pad_left: hint
            .get("paddingLeft")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        pad_right: hint
            .get("paddingRight")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

/// What a label says: the text, or the parts of it joined. None for one that says nothing.
fn label_of(label: &Value) -> Option<String> {
    let text = match label {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part.get("value")?.as_str())
            .collect(),
        _ => return None,
    };
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::*;

    fn hint(line: u32, character: u32, label: Value, kind: Option<u8>) -> Value {
        let mut hint = json!({"position": {"line": line, "character": character}, "label": label});
        if let Some(kind) = kind {
            hint["kind"] = json!(kind);
        }
        hint
    }

    #[test]
    fn the_hints_of_a_range_are_asked_for_by_the_file_and_the_lines() {
        assert_eq!(INLAY_HINT, "textDocument/inlayHint");
        assert_eq!(
            hint_params(
                &PathBuf::from("/p/src/main.rs"),
                Position {
                    line: 10,
                    character: 0
                },
                Position {
                    line: 41,
                    character: 3
                }
            ),
            json!({
                "textDocument": {"uri": "file:///p/src/main.rs"},
                "range": {"start": {"line": 10, "character": 0}, "end": {"line": 41, "character": 3}},
            })
        );
    }

    #[test]
    fn a_type_hint_has_its_label_its_place_and_its_padding() {
        let mut sent = hint(3, 9, json!(": Vec<u32>"), Some(1));
        sent["paddingLeft"] = json!(true);
        let found = hints(&json!([sent]));
        assert_eq!(
            found,
            vec![Reported {
                at: Position {
                    line: 3,
                    character: 9
                },
                label: ": Vec<u32>".to_owned(),
                kind: HintKind::Type,
                pad_left: true,
                pad_right: false,
            }]
        );
    }

    #[test]
    fn the_parts_of_a_label_are_joined_and_the_kinds_are_told_apart() {
        let parts = json!([{"value": "count"}, {"value": ":", "tooltip": "ignored"}]);
        let found = hints(&json!([
            hint(1, 4, parts, Some(2)),
            hint(2, 0, json!("}"), None),
            hint(2, 1, json!("x"), Some(9)),
        ]));
        assert_eq!(
            found
                .iter()
                .map(|hint| hint.label.as_str())
                .collect::<Vec<_>>(),
            ["count:", "}", "x"]
        );
        assert_eq!(
            found
                .iter()
                .map(|hint| hint.kind)
                .collect::<Vec<_>>(),
            [HintKind::Parameter, HintKind::Other, HintKind::Other],
            "no kind, and a kind that is not known, are other"
        );
    }

    #[test]
    fn what_is_not_made_as_it_should_be_is_left_out() {
        assert!(hints(&json!(null)).is_empty(), "the server had none to say");
        assert!(hints(&json!({"not": "a list"})).is_empty());
        let found = hints(&json!([
            {"label": "no position"},
            hint(0, 0, json!(""), Some(1)),
            hint(0, 0, json!([]), Some(1)),
            {"position": {"line": -1, "character": 0}, "label": "x"},
            {"position": {"line": 0, "character": 0}},
            hint(5, 2, json!("ok"), Some(1)),
        ]));
        assert_eq!(
            found.len(),
            1,
            "a hint that says nothing is nothing to draw"
        );
        assert_eq!(found[0].label, "ok");
    }

    #[test]
    fn the_server_is_told_that_hints_can_be_drawn_and_what_the_client_does_not_do_with_them() {
        let params = super::super::initialize_params(Path::new("/p"), 1);
        assert_eq!(
            params["capabilities"]["textDocument"]["inlayHint"],
            client_capability()
        );
        assert_eq!(client_capability()["dynamicRegistration"], json!(false));
        assert!(
            client_capability()
                .get("resolveSupport")
                .is_none(),
            "a label is taken as it comes: there is nothing to resolve"
        );
    }

    #[test]
    fn what_the_server_can_hint_is_read_from_what_it_answers_to_initialize() {
        let said = |capabilities: Value| {
            super::super::initialize_result(&json!({"capabilities": capabilities}))
                .unwrap()
                .inlay_hints
        };
        assert!(said(json!({"inlayHintProvider": true})));
        assert!(said(
            json!({"inlayHintProvider": {"resolveProvider": true}})
        ));
        assert!(!said(json!({"inlayHintProvider": false})));
        assert!(!said(json!({})));
    }
}
