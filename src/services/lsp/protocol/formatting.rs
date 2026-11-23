//! Document formatting: asking the server for the edits that format a whole file (`textDocument/formatting`).

use std::path::Path;

use serde_json::{Value, json};

use super::super::position::Position;
use super::code_action::edits_of;
use super::uri_of;

/// The method that asks for the edits that format a file.
pub const FORMATTING: &str = "textDocument/formatting";

/// What the client says of formatting in `initialize`.
pub fn client_capability() -> Value {
    json!({
        "dynamicRegistration": false,
    })
}

/// The parameters of `textDocument/formatting` for the file `path`, indented with `tab_spaces` spaces.
pub fn formatting_params(path: &Path, tab_spaces: usize) -> Value {
    json!({
        "textDocument": {"uri": uri_of(path)},
        "options": {
            "tabSize": tab_spaces,
            "insertSpaces": true,
        },
    })
}

/// The edits the server answered with, in the order it said them. `null` is an answer of no edits; what is neither `null` nor a
/// well-formed list of text edits is `None`.
pub fn formatting_edits(result: &Value) -> Option<Vec<((Position, Position), String)>> {
    if result.is_null() {
        return Some(Vec::new());
    }
    let mut edits = Vec::new();
    edits_of(result, &mut edits)?;
    Some(edits)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn at(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    #[test]
    fn the_formatting_request_names_the_file_the_tab_size_and_spaces() {
        assert_eq!(FORMATTING, "textDocument/formatting");
        assert_eq!(
            formatting_params(&PathBuf::from("/p/src/main.rs"), 4),
            json!({
                "textDocument": {"uri": "file:///p/src/main.rs"},
                "options": {
                    "tabSize": 4,
                    "insertSpaces": true,
                },
            })
        );
    }

    #[test]
    fn formatting_edits_are_read_from_a_list_of_text_edits_or_null_and_reject_bad_answers() {
        let edit = json!({
            "range": {
                "start": {"line": 0, "character": 8},
                "end": {"line": 0, "character": 8},
            },
            "newText": " ",
        });
        assert_eq!(
            formatting_edits(&json!([edit])),
            Some(vec![((at(0, 8), at(0, 8)), " ".to_owned())])
        );
        assert_eq!(formatting_edits(&Value::Null), Some(Vec::new()));
        assert_eq!(formatting_edits(&json!([])), Some(Vec::new()));
        assert_eq!(formatting_edits(&json!({"not": "a list"})), None);
        assert_eq!(
            formatting_edits(
                &json!([{"range": {"start": {"line": -1, "character": 0}}, "newText": "x"}])
            ),
            None
        );
    }

    #[test]
    fn the_server_is_told_that_the_client_asks_for_formatting() {
        let params = super::super::initialize_params(Path::new("/p"), 1);
        assert_eq!(
            params["capabilities"]["textDocument"]["formatting"]["dynamicRegistration"],
            json!(false)
        );
        assert!(
            serde_json::from_value::<lsp_types::InitializeParams>(params).is_ok(),
            "what the protocol has"
        );
    }
}
