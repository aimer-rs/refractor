//! Making the edits of a rename in the documents of the workspace: the files that are open are changed where they are open, and the files
//! that are not are opened to be changed, so that every change of a rename can be undone, and is saved as any other.
//!
//! It is two steps, and not one: the workspace gets the rename ready inside an update of it, and the edits are made after, outside it. A
//! document that is changed tells the workspace, which is an update, and an update inside an update is an error that ends the app.

use std::ops::Range;
use std::rc::Rc;

use super::Workspace;
use crate::model::document::Document;
use crate::services::lsp::{CodeFix, Rename};

/// What a rename did: how many places it changed, and in how many files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Renamed {
    pub places: usize,
    pub files: usize,
}

/// A rename that is ready to be made: the documents it changes, and what to put in each. It is made by [`RenamePlan::apply`], which needs no
/// part of the workspace, because it must not be made inside an update of it: a document that becomes changed tells the workspace, which is an
/// update of its own, and one that began inside another is an error.
pub struct RenamePlan {
    fixes: Vec<(Rc<Document>, CodeFix)>,
    places: usize,
}

impl RenamePlan {
    /// Makes the edits, each file as one edit that one undo takes back, with the caret and the selection staying with the text they were with.
    /// All of them were checked when the plan was made; a file that is changed since is not, and says so, and is as it was.
    pub fn apply(&self) -> Result<Renamed, String> {
        for (document, fix) in &self.fixes {
            if !fix.apply(&document.controller) {
                return Err(format!(
                    "Could not rename in {}: it changed while the rename was made",
                    document.name
                ));
            }
        }
        Ok(Renamed {
            places: self.places,
            files: self.fixes.len(),
        })
    }
}

impl Workspace {
    /// Gets a rename ready: the files it changes are opened, and the edits are checked, but none is made, for that is [`RenamePlan::apply`]'s,
    /// outside this. A file that is not open is opened in a tab that is not saved, and the tab that was shown is shown again.
    ///
    /// All or nothing: there is no plan where the file that was asked in has been changed since, a file cannot be opened, or the edits of one
    /// overlap; and the reason is said.
    pub fn plan_rename(
        &mut self,
        rename: &Rename,
        notify: Rc<dyn Fn()>,
    ) -> Result<RenamePlan, String> {
        let (origin, asked) = (&rename.of.0, &rename.of.1);
        if let Some(document) = self
            .documents
            .iter()
            .find(|document| document.path == *origin)
            && document
                .controller
                .snapshot()
                .diff(asked)
                .is_some()
        {
            return Err(
                "The text changed while the language server was renaming: try again".to_owned(),
            );
        }
        let shown = self.active;
        // Every file is opened and every set of edits is checked before any is made.
        let mut fixes = Vec::with_capacity(rename.edit.files.len());
        let mut failure = None;
        for (path, edits) in &rename.edit.files {
            if let Err(error) = self.open(path, Rc::clone(&notify)) {
                failure = Some(format!(
                    "Could not open {} to rename in it: {error}",
                    path.display()
                ));
                break;
            }
            let document = self
                .documents
                .iter()
                .find(|document| document.path == *path)
                .expect("a file that was opened is open");
            let text = document.controller.snapshot();
            let bytes: Vec<(Range<usize>, String)> = edits
                .iter()
                .map(|((from, to), new_text)| {
                    let start = self.lsp.byte_of(&text, *from);
                    (
                        start..self.lsp.byte_of(&text, *to).max(start),
                        new_text.clone(),
                    )
                })
                .collect();
            if overlapping(&bytes) {
                failure = Some(format!(
                    "The edits of the rename overlap in {}",
                    document.name
                ));
                break;
            }
            fixes.push((
                Rc::clone(document),
                CodeFix {
                    title: "Rename".to_owned(),
                    preferred: true,
                    edits: bytes,
                    text,
                },
            ));
        }
        // Whatever was opened for it is no longer what is shown.
        self.active = shown
            .filter(|id| {
                self.documents
                    .iter()
                    .any(|document| document.id == *id)
            })
            .or(self.active);
        if let Some(failure) = failure {
            self.message = Some(failure.clone());
            return Err(failure);
        }
        Ok(RenamePlan {
            fixes,
            places: rename.edit.places(),
        })
    }

    /// Records how a rename went, which is said in the status bar: how many places in how many files, or why not.
    pub fn report_rename(&mut self, result: Result<Renamed, String>) {
        self.message = Some(match result {
            Ok(Renamed { places, files: 1 }) => format!("Renamed {places} places"),
            Ok(Renamed { places, files }) => format!(
                "Renamed {places} places in {files} files: the files that were not open are open, not saved"
            ),
            Err(why) => why,
        });
        self.touch();
    }
}

/// Whether any two of `edits` touch the same bytes: a place that is replaced twice has no one result.
fn overlapping(edits: &[(Range<usize>, String)]) -> bool {
    let mut sorted: Vec<&Range<usize>> = edits.iter().map(|(range, _)| range).collect();
    sorted.sort_by_key(|range| (range.start, range.end));
    sorted
        .windows(2)
        .any(|pair| pair[0].end > pair[1].start)
}

#[cfg(test)]
#[path = "../../tests/model/workspace/rename.rs"]
mod tests;
