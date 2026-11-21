//! Code actions: what a server offers to do about the text at a place, such as the line that imports a name that is not in scope.
//!
//! `textDocument/codeAction` answers with a list of actions, each with a title and the edits that it makes. The client says that it
//! takes the actions that have edits of their own, so that it is not asked to resolve them, and the edits of a file as a list for each
//! file and not as a list of operations.

use std::path::Path;

use serde_json::{Value, json};

use super::super::position::Position;
use super::{from_position, path_of, uri_of};

/// The method that asks for the actions of a range of a file.
pub const CODE_ACTION: &str = "textDocument/codeAction";

/// An action as the server said it, with its edits in the terms of the protocol, which the text they are about is needed to turn into bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    pub title: String,
    /// Whether the server says it is the one to do, if there is one.
    pub preferred: bool,
    /// The places to put text, and the text to put there, all in the one file that was asked about.
    pub edits: Vec<((Position, Position), String)>,
}

/// What the client says of actions: that it takes those that say what they make, of the kinds that fix a problem, and prefers one.
pub fn client_capability() -> Value {
    json!({
        "dynamicRegistration": false,
        "isPreferredSupport": true,
        "codeActionLiteralSupport": {"codeActionKind": {"valueSet": ["quickfix"]}},
    })
}

/// What the client says of the edits of a workspace: the changes of documents, and no operations on files.
pub fn workspace_edit_capability() -> Value {
    json!({"documentChanges": true, "resourceOperations": []})
}

/// The parameters of `textDocument/codeAction` for the range `range` of the file `path`: only what fixes a problem is asked for.
pub fn action_params(path: &Path, range: (Position, Position)) -> Value {
    json!({
        "textDocument": {"uri": uri_of(path)},
        "range": {
            "start": {"line": range.0.line, "character": range.0.character},
            "end": {"line": range.1.line, "character": range.1.character},
        },
        "context": {"diagnostics": [], "only": ["quickfix"]},
    })
}

/// The actions the server offered that make their edits in the file `path` and nowhere else, in the order it said them. What is a
/// command with no edits, an action that has to be resolved, or one that changes another file is left out: it is not one that is done here.
pub fn actions(result: &Value, path: &Path) -> Vec<Action> {
    let Some(list) = result.as_array() else {
        return Vec::new();
    };
    list.iter().filter_map(|action| one(action, path)).collect()
}

/// The edits that `list` says to make in the file, which are all of one file: those that are not made as they should be make none.
fn edits_of(list: &Value, edits: &mut Vec<((Position, Position), String)>) -> Option<()> {
    for edit in list.as_array()? {
        let start = edit.pointer("/range/start")?;
        let end = edit.pointer("/range/end")?;
        let position = |point: &Value| -> Option<Position> {
            Some(from_position(lsp_types::Position {
                line: u32::try_from(point.get("line")?.as_u64()?).ok()?,
                character: u32::try_from(point.get("character")?.as_u64()?).ok()?,
            }))
        };
        edits.push(((position(start)?, position(end)?), edit.get("newText")?.as_str()?.to_owned()));
    }
    Some(())
}

fn one(action: &Value, path: &Path) -> Option<Action> {
    let title = action.get("title")?.as_str()?.to_owned();
    let edit = action.get("edit")?;
    let mut edits = Vec::new();
    // The edits as a list for each file.
    if let Some(changes) = edit.get("changes").and_then(Value::as_object) {
        for (uri, list) in changes {
            // An action that touches another file is not for this one.
            if path_of(uri).as_deref() != Some(path) {
                return None;
            }
            edits_of(list, &mut edits)?;
        }
    }
    // The edits as the changes of documents, which is how rust-analyzer says them. A change that is not the edits of a document, an
    // operation on a file, is not one that is done here.
    if let Some(changes) = edit.get("documentChanges").and_then(Value::as_array) {
        for change in changes {
            let uri = change.pointer("/textDocument/uri")?.as_str()?;
            if path_of(uri).as_deref() != Some(path) {
                return None;
            }
            edits_of(change.get("edits")?, &mut edits)?;
        }
    }
    (!edits.is_empty()).then(|| Action {
        title,
        preferred: action.get("isPreferred").and_then(Value::as_bool).unwrap_or(false),
        edits,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn at(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    fn edit(line: u32, from: u32, to: u32, text: &str) -> Value {
        json!({"range": {"start": {"line": line, "character": from}, "end": {"line": line, "character": to}}, "newText": text})
    }

    fn import() -> Value {
        json!({
            "title": "Import `std::rc::Rc`",
            "kind": "quickfix",
            "isPreferred": true,
            "edit": {"changes": {"file:///p/src/main.rs": [edit(0, 0, 0, "use std::rc::Rc;\n")]}},
        })
    }

    #[test]
    fn the_actions_of_a_range_are_asked_for_by_the_file_and_the_range_and_only_what_fixes_a_problem_is() {
        assert_eq!(CODE_ACTION, "textDocument/codeAction");
        assert_eq!(
            action_params(&PathBuf::from("/p/src/main.rs"), (at(3, 8), at(3, 10))),
            json!({
                "textDocument": {"uri": "file:///p/src/main.rs"},
                "range": {"start": {"line": 3, "character": 8}, "end": {"line": 3, "character": 10}},
                "context": {"diagnostics": [], "only": ["quickfix"]},
            })
        );
    }

    #[test]
    fn an_action_has_its_title_whether_the_server_prefers_it_and_the_edits_it_makes() {
        let found = actions(&json!([import()]), Path::new("/p/src/main.rs"));
        assert_eq!(
            found,
            vec![Action {
                title: "Import `std::rc::Rc`".to_owned(),
                preferred: true,
                edits: vec![((at(0, 0), at(0, 0)), "use std::rc::Rc;\n".to_owned())],
            }]
        );
    }

    #[test]
    fn what_is_not_said_to_be_preferred_is_not_and_the_order_is_the_servers() {
        let second = json!({"title": "Second", "edit": {"changes": {"file:///p/src/main.rs": [edit(1, 0, 1, "x")]}}});
        let found = actions(&json!([second, import()]), Path::new("/p/src/main.rs"));
        assert_eq!(found.iter().map(|action| (action.title.as_str(), action.preferred)).collect::<Vec<_>>(), [("Second", false), ("Import `std::rc::Rc`", true)]);
    }

    fn by_document_changes(uri: &str, edits: Vec<Value>) -> Value {
        json!({"title": "Import", "edit": {"documentChanges": [{"textDocument": {"uri": uri, "version": 3}, "edits": edits}]}})
    }

    #[test]
    fn the_edits_of_a_server_that_says_them_as_changes_of_documents_are_read_as_well() {
        // What rust-analyzer says: a list of the changes of documents, each with the document and its edits.
        let found = actions(&json!([by_document_changes("file:///p/src/main.rs", vec![edit(0, 0, 0, "use std::rc::Rc;\n")])]), Path::new("/p/src/main.rs"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].edits, vec![((at(0, 0), at(0, 0)), "use std::rc::Rc;\n".to_owned())]);
        // Edits that are annotated are edits, and what says more than an edit is not read as one.
        let annotated = by_document_changes(
            "file:///p/src/main.rs",
            vec![json!({"range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 2}}, "newText": "x", "annotationId": "a"})],
        );
        assert_eq!(actions(&json!([annotated]), Path::new("/p/src/main.rs"))[0].edits[0].1, "x");
    }

    #[test]
    fn changes_of_documents_in_other_files_and_operations_on_files_are_left_out() {
        let path = Path::new("/p/src/main.rs");
        let elsewhere = by_document_changes("file:///p/src/other.rs", vec![edit(0, 0, 0, "x")]);
        let create = json!({"title": "New file", "edit": {"documentChanges": [{"kind": "create", "uri": "file:///p/new.rs"}]}});
        let mixed = json!({"title": "Mixed", "edit": {"documentChanges": [
            {"textDocument": {"uri": "file:///p/src/main.rs", "version": 1}, "edits": [edit(0, 0, 0, "x")]},
            {"kind": "create", "uri": "file:///p/new.rs"}]}});
        let two = json!({"title": "Two files", "edit": {"documentChanges": [
            {"textDocument": {"uri": "file:///p/src/main.rs", "version": 1}, "edits": [edit(0, 0, 0, "x")]},
            {"textDocument": {"uri": "file:///p/src/other.rs", "version": 1}, "edits": [edit(0, 0, 0, "y")]}]}});
        assert!(actions(&json!([elsewhere, create, mixed, two]), path).is_empty());
        // The edits of one file said in two documents' changes are the edits of that file.
        let split = json!({"title": "Split", "edit": {"documentChanges": [
            {"textDocument": {"uri": "file:///p/src/main.rs", "version": 1}, "edits": [edit(0, 0, 0, "a")]},
            {"textDocument": {"uri": "file:///p/src/main.rs", "version": 1}, "edits": [edit(5, 0, 0, "b")]}]}});
        assert_eq!(actions(&json!([split]), path)[0].edits.len(), 2);
    }

    #[test]
    fn what_has_no_edits_here_is_left_out() {
        let path = Path::new("/p/src/main.rs");
        let command = json!({"title": "Run", "command": {"title": "Run", "command": "run"}});
        let unresolved = json!({"title": "Needs resolving", "data": {"id": 1}});
        let elsewhere = json!({"title": "Other file", "edit": {"changes": {"file:///p/src/other.rs": [edit(0, 0, 0, "x")]}}});
        let both = json!({"title": "Two files", "edit": {"changes": {
            "file:///p/src/main.rs": [edit(0, 0, 0, "x")], "file:///p/src/other.rs": [edit(0, 0, 0, "y")]}}});
        let no_edits = json!({"title": "Nothing", "edit": {"changes": {"file:///p/src/main.rs": []}}});
        let operations = json!({"title": "Files", "edit": {"documentChanges": [{"kind": "create", "uri": "file:///p/new.rs"}]}});
        let empty_changes = json!({"title": "Empty", "edit": {"documentChanges": []}});
        let broken = json!({"title": "Broken", "edit": {"changes": {"file:///p/src/main.rs": [{"range": {"start": {"line": -1, "character": 0}}, "newText": "x"}]}}});
        let found = actions(&json!([command, unresolved, elsewhere, both, no_edits, operations, empty_changes, broken, import()]), path);
        assert_eq!(found.iter().map(|action| action.title.as_str()).collect::<Vec<_>>(), ["Import `std::rc::Rc`"]);
        assert!(actions(&Value::Null, path).is_empty(), "the server had none to say");
        assert!(actions(&json!({"not": "a list"}), path).is_empty());
    }

    #[test]
    fn the_server_is_told_that_the_client_takes_the_actions_that_say_what_they_make_and_lists_of_edits() {
        let params = super::super::initialize_params(Path::new("/p"), 1);
        let action = &params["capabilities"]["textDocument"]["codeAction"];
        assert_eq!(action["codeActionLiteralSupport"]["codeActionKind"]["valueSet"], json!(["quickfix"]));
        assert_eq!(action["isPreferredSupport"], json!(true));
        assert!(action.get("resolveSupport").is_none(), "an action is not resolved: the edits come with it");
        assert_eq!(params["capabilities"]["workspace"]["workspaceEdit"]["documentChanges"], json!(true));
        assert!(serde_json::from_value::<lsp_types::InitializeParams>(params).is_ok(), "what the protocol has");
    }
}
