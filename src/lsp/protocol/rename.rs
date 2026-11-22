//! Renaming a name everywhere it is used, and asking where it is used: `textDocument/rename` answers with the edits of every file that has
//! the name in it, and `textDocument/references` with the places.
//!
//! The edits of a rename are as many files as use the name, and are said as the changes of documents or as a list for each file: both are
//! read, and what is more than an edit of text, the renaming of a file, is an error and not left out: a rename that did only some of what
//! the server said would leave the code that does not build.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::super::position::Position;
use super::code_action::edits_of;
use super::{path_of, uri_of};

/// The method that asks for the edits that rename a name.
pub const RENAME: &str = "textDocument/rename";
/// The method that asks for the places a name is used.
pub const REFERENCES: &str = "textDocument/references";

/// The edits of a rename: for each file that has the name, the places to put text and the text to put there, in the terms of the protocol.
/// A file is there once, with the edits of all its changes, in the order the server said them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkspaceEdit {
    pub files: Vec<(PathBuf, Vec<((Position, Position), String)>)>,
}

impl WorkspaceEdit {
    /// How many places are changed, in all the files.
    pub fn places(&self) -> usize {
        self.files
            .iter()
            .map(|(_, edits)| edits.len())
            .sum()
    }

    fn add(&mut self, path: PathBuf, mut edits: Vec<((Position, Position), String)>) {
        match self
            .files
            .iter_mut()
            .find(|(known, _)| *known == path)
        {
            Some((_, known)) => known.append(&mut edits),
            None => self.files.push((path, edits)),
        }
    }
}

/// The parameters of `textDocument/rename`: the name at `at` in the file `path`, to be called `new_name`.
pub fn rename_params(path: &Path, at: Position, new_name: &str) -> Value {
    json!({
        "textDocument": {"uri": uri_of(path)},
        "position": {"line": at.line, "character": at.character},
        "newName": new_name,
    })
}

/// The parameters of `textDocument/references` for the name at `at` in the file `path`. Where the name is defined is not a use of it, and is
/// not asked for: it is where the list is opened from.
pub fn references_params(path: &Path, at: Position) -> Value {
    json!({
        "textDocument": {"uri": uri_of(path)},
        "position": {"line": at.line, "character": at.character},
        "context": {"includeDeclaration": false},
    })
}

/// What a server answered to a rename: the edits, or why there are none. A name that cannot be renamed there, which the server says
/// as nothing or as an error, is the same to the user: there is nothing to do.
pub fn workspace_edit(result: &Value) -> Result<WorkspaceEdit, String> {
    if result.is_null() {
        return Err("There is nothing to rename there".to_owned());
    }
    let mut found = WorkspaceEdit::default();
    let bad = || "The server's answer to the rename could not be read".to_owned();
    if let Some(changes) = result.get("changes").and_then(Value::as_object) {
        for (uri, list) in changes {
            let path = path_of(uri).ok_or_else(bad)?;
            let mut edits = Vec::new();
            edits_of(list, &mut edits).ok_or_else(bad)?;
            found.add(path, edits);
        }
    }
    if let Some(changes) = result
        .get("documentChanges")
        .and_then(Value::as_array)
    {
        for change in changes {
            // The renaming of a file, or the making of one: more than an edit of text, and not done here.
            if let Some(kind) = change.get("kind").and_then(Value::as_str) {
                return Err(format!(
                    "The rename needs to {kind} a file, which this editor does not do"
                ));
            }
            let uri = change
                .pointer("/textDocument/uri")
                .and_then(Value::as_str)
                .ok_or_else(bad)?;
            let path = path_of(uri).ok_or_else(bad)?;
            let mut edits = Vec::new();
            edits_of(change.get("edits").ok_or_else(bad)?, &mut edits).ok_or_else(bad)?;
            found.add(path, edits);
        }
    }
    if found.places() == 0 {
        return Err("There is nothing to rename there".to_owned());
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    fn edit(line: u32, from: u32, to: u32, text: &str) -> Value {
        json!({"range": {"start": {"line": line, "character": from}, "end": {"line": line, "character": to}}, "newText": text})
    }

    #[test]
    fn a_rename_and_a_request_for_the_places_are_asked_by_the_file_and_the_place() {
        assert_eq!(RENAME, "textDocument/rename");
        assert_eq!(REFERENCES, "textDocument/references");
        assert_eq!(
            rename_params(Path::new("/p/src/a b.rs"), at(3, 8), "Fresh"),
            json!({"textDocument": {"uri": "file:///p/src/a%20b.rs"}, "position": {"line": 3, "character": 8}, "newName": "Fresh"})
        );
        assert_eq!(
            references_params(Path::new("/p/src/a.rs"), at(0, 4)),
            json!({"textDocument": {"uri": "file:///p/src/a.rs"}, "position": {"line": 0, "character": 4}, "context": {"includeDeclaration": false}})
        );
    }

    #[test]
    fn the_edits_of_a_rename_are_read_for_each_file_whether_said_as_changes_or_as_documents() {
        let by_file = json!({"changes": {
            "file:///p/src/a.rs": [edit(1, 4, 9, "Fresh"), edit(7, 0, 5, "Fresh")],
            "file:///p/src/b.rs": [edit(2, 2, 7, "Fresh")]}});
        let found = workspace_edit(&by_file).expect("the edits");
        assert_eq!(found.files.len(), 2);
        assert_eq!(found.places(), 3);
        let by_document = json!({"documentChanges": [
            {"textDocument": {"uri": "file:///p/src/a.rs", "version": 3}, "edits": [edit(1, 4, 9, "Fresh")]},
            {"textDocument": {"uri": "file:///p/src/b.rs", "version": null}, "edits": [edit(2, 2, 7, "Fresh")]}]});
        let found = workspace_edit(&by_document).expect("the edits");
        assert_eq!(
            found.files,
            vec![
                (
                    PathBuf::from("/p/src/a.rs"),
                    vec![((at(1, 4), at(1, 9)), "Fresh".to_owned())]
                ),
                (
                    PathBuf::from("/p/src/b.rs"),
                    vec![((at(2, 2), at(2, 7)), "Fresh".to_owned())]
                ),
            ]
        );
    }

    #[test]
    fn the_changes_of_one_file_that_are_said_twice_are_the_edits_of_one_file_in_the_order_they_were_said()
     {
        let split = json!({"documentChanges": [
            {"textDocument": {"uri": "file:///p/src/a.rs", "version": 1}, "edits": [edit(0, 0, 1, "x")]},
            {"textDocument": {"uri": "file:///p/src/b.rs", "version": 1}, "edits": [edit(0, 0, 1, "y")]},
            {"textDocument": {"uri": "file:///p/src/a.rs", "version": 1}, "edits": [edit(5, 0, 1, "z")]}]});
        let found = workspace_edit(&split).expect("the edits");
        assert_eq!(found.files.len(), 2);
        assert_eq!(
            found.files[0]
                .1
                .iter()
                .map(|(_, text)| text.as_str())
                .collect::<Vec<_>>(),
            ["x", "z"]
        );
    }

    #[test]
    fn nothing_to_rename_and_an_answer_that_is_not_edits_are_errors_with_a_reason() {
        assert!(
            workspace_edit(&Value::Null)
                .unwrap_err()
                .contains("nothing to rename")
        );
        assert!(
            workspace_edit(&json!({}))
                .unwrap_err()
                .contains("nothing to rename")
        );
        assert!(
            workspace_edit(&json!({"changes": {}}))
                .unwrap_err()
                .contains("nothing to rename")
        );
        assert!(
            workspace_edit(&json!({"changes": {"file:///p/a.rs": "not a list"}}))
                .unwrap_err()
                .contains("could not be read")
        );
        assert!(
            workspace_edit(&json!({"changes": {"http://x/a.rs": [edit(0, 0, 1, "x")]}}))
                .unwrap_err()
                .contains("could not be read")
        );
        assert!(
            workspace_edit(&json!({"documentChanges": [{"edits": []}]}))
                .unwrap_err()
                .contains("could not be read")
        );
    }

    #[test]
    fn what_rust_analyzer_really_says_of_a_rename_in_two_files_is_read() {
        // As it answered renaming a struct that is used in a file that is not open: the file that is not open has no version.
        let said = json!({"documentChanges": [
            {"textDocument": {"version": null, "uri": "file:///p/src/other.rs"}, "edits": [edit(0, 11, 16, "Fresh"), edit(2, 15, 20, "Fresh")]},
            {"textDocument": {"version": 1, "uri": "file:///p/src/main.rs"}, "edits": [edit(2, 7, 12, "Fresh"), edit(7, 12, 17, "Fresh")]}]});
        let found = workspace_edit(&said).expect("the edits");
        assert_eq!(found.places(), 4);
        assert_eq!(
            found
                .files
                .iter()
                .map(|(path, _)| path.as_path())
                .collect::<Vec<_>>(),
            [Path::new("/p/src/other.rs"), Path::new("/p/src/main.rs")]
        );
    }

    #[test]
    fn the_renaming_of_a_file_is_an_error_and_not_a_rename_that_does_only_a_part() {
        let rename = json!({"documentChanges": [
            {"textDocument": {"uri": "file:///p/src/a.rs", "version": 1}, "edits": [edit(0, 0, 1, "x")]},
            {"kind": "rename", "oldUri": "file:///p/src/a.rs", "newUri": "file:///p/src/b.rs"}]});
        let error = workspace_edit(&rename).unwrap_err();
        assert!(error.contains("rename a file"), "{error}");
        let create = json!({"documentChanges": [{"kind": "create", "uri": "file:///p/new.rs"}]});
        assert!(
            workspace_edit(&create)
                .unwrap_err()
                .contains("create a file")
        );
    }
}
