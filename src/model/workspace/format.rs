//! Formatting the active document of the workspace according to the user's [`Formatter`] setting:
//! trying the language server first and falling back to the external formatter (`Auto`), asking the
//! language server only (`Lsp`), or running the external formatter (`External`).
//!
//! As with saving and renaming, the edits are applied to the document's controller outside of an
//! update of the workspace: changing a document's text notifies the workspace, which is an update
//! of its own.

use std::rc::Rc;

use aimer::{ProviderHandle, Venus};

use super::Workspace;
use crate::model::document::{Document, Language};
use crate::services::lsp::{Lsp, format_external};
use crate::services::settings::Formatter;

impl Workspace {
    /// The document that `Code > Reformat Code` acts on, and the workspace's language server: `None`
    /// when no file is open or when the settings tab is the one shown.
    pub fn active_document_for_format(&self) -> Option<(Rc<Document>, Lsp)> {
        if self.settings_active() {
            return None;
        }
        self.active_document()
            .map(|document| (Rc::clone(document), self.lsp.clone()))
    }

    /// Records how formatting `name` went: clears any status bar message when it succeeded, and
    /// says why in the status bar when it could not be formatted.
    pub fn report_format(&mut self, name: &str, result: Result<(), String>) {
        self.message = result
            .err()
            .map(|why| format!("Could not format {name}: {why}"));
    }
}

/// Formats `document` according to `formatter` (`Auto`, `Lsp`, or `External`) with `tab_spaces` spaces per indent,
/// and calls `report` with `Ok(())` when it succeeded or `Err(why)` when it could not be formatted.
pub fn format_document(
    document: Rc<Document>,
    lsp: Lsp,
    formatter: Formatter,
    tab_spaces: usize,
    report: impl FnOnce(Result<(), String>) + 'static,
) {
    match formatter {
        Formatter::Lsp => {
            if document.language != Language::Rust {
                report(Err(format!(
                    "No language server is running for {}",
                    document.language.label()
                )));
                return;
            }
            let path = document.path.clone();
            lsp.format(&path, tab_spaces, move |edit| match edit {
                Some(edit) => {
                    let _ = edit.apply(&document.controller);
                    report(Ok(()));
                }
                None => report(Err(
                    "the language server could not format this file".to_owned()
                )),
            });
        }
        Formatter::External => {
            run_external(document, tab_spaces, report);
        }
        Formatter::Auto => {
            if document.language != Language::Rust {
                run_external(document, tab_spaces, report);
                return;
            }
            let path = document.path.clone();
            lsp.format(&path, tab_spaces, move |edit| match edit {
                Some(edit) => {
                    let _ = edit.apply(&document.controller);
                    report(Ok(()));
                }
                None => run_external(document, tab_spaces, report),
            });
        }
    }
}

fn run_external(
    document: Rc<Document>,
    tab_spaces: usize,
    report: impl FnOnce(Result<(), String>) + 'static,
) {
    let language = document.language;
    let path = document.path.clone();
    let text = document.controller.snapshot();
    match Venus::current() {
        Some(venus) => {
            let blocking = venus.clone();
            venus.spawn(async move {
                let result = blocking
                    .spawn_blocking(move || format_external(language, &path, text, tab_spaces))
                    .await;
                match result {
                    Ok(edit) => {
                        let _ = edit.apply(&document.controller);
                        report(Ok(()));
                    }
                    Err(why) => report(Err(why)),
                }
            });
        }
        None => match format_external(language, &path, text, tab_spaces) {
            Ok(edit) => {
                let _ = edit.apply(&document.controller);
                report(Ok(()));
            }
            Err(why) => report(Err(why)),
        },
    }
}

/// Formats the currently active document of `store` (if a file tab is shown), and reports any failure in the status bar.
pub fn format_active_document(
    store: &ProviderHandle<Workspace>,
    formatter: Formatter,
    tab_spaces: usize,
) {
    let Some((document, lsp)) = store.read().active_document_for_format() else {
        return;
    };
    let store = store.clone();
    let name = document.name.clone();
    format_document(document, lsp, formatter, tab_spaces, move |result| {
        store.update(|space| space.report_format(&name, result));
    });
}

#[cfg(test)]
#[path = "../../tests/model/workspace/format.rs"]
mod tests;
