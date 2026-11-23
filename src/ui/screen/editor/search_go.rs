//! What choosing a hit of the search does: goes to the file, the line or the definition, or does the action.

use crate::services::search::{ActionId, Hit, Target};
use crate::model::workspace::Workspace;
use std::rc::Rc;

/// What is left to do after a hit was applied to the workspace, which the workspace cannot do by itself: it is told by what
/// reads the disk, which is not done inside an update of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Later {
    Nothing,
    /// Save the file that is shown.
    Save,
    /// Read the folder again for what is on disk, and what git says of it.
    Reload,
    /// Open the Git Commit modal.
    GitCommit,
    /// Open the Git Push modal.
    GitPush,
}

/// Does what choosing `hit` asks of the workspace: opens the file it is in and shows where it is, or does the action.
/// A file that cannot be opened is not gone to, and the workspace is as it was.
pub fn apply(space: &mut Workspace, hit: &Hit, notify: Rc<dyn Fn()>) -> Later {
    match &hit.target {
        Target::File { path, at: None } => {
            let _ = space.open(path, notify);
        }
        Target::File {
            path,
            at: Some((line, column)),
        } => {
            // What is marked is the text that was found, which is shown selected as the definition of a name is.
            let len = hit
                .marked
                .first()
                .map_or(0, |marked| marked.len());
            let _ = space.go_to_line(path, *line, *column, len, notify);
        }
        Target::Place(place) => {
            let _ = space.go_to_place(place, notify);
        }
        Target::Action(action) => match action {
            ActionId::OpenSettings => space.open_settings(),
            ActionId::ToggleProblems => space.toggle_problems(),
            ActionId::CollapseFolders => space.collapse_all(),
            ActionId::SaveFile => return Later::Save,
            ActionId::ReloadFolder => {
                space.refresh();
                return Later::Reload;
            }
            ActionId::GitCommit => return Later::GitCommit,
            ActionId::GitPush => return Later::GitPush,
        },
    }
    Later::Nothing
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::lsp::{Place, Position};
    use crate::services::search::Kind;
    use crate::model::tree::Scratch;

    fn notify() -> Rc<dyn Fn()> {
        Rc::new(|| {})
    }

    fn hit(target: Target, marked: std::ops::Range<usize>) -> Hit {
        Hit {
            kind: Kind::Text,
            title: String::new(),
            marked: vec![marked],
            detail: String::new(),
            target,
            score: 0,
        }
    }

    #[test]
    fn a_file_is_opened_and_shown() {
        let dir = Scratch::new("go-file");
        let a = dir.file("a.txt", "one\n");
        let mut space = Workspace::new(dir.0.clone());
        let later = apply(
            &mut space,
            &hit(
                Target::File {
                    path: a.clone(),
                    at: None,
                },
                0..0,
            ),
            notify(),
        );
        assert_eq!(later, Later::Nothing);
        assert_eq!(space.active_document().expect("shown").path, a);
    }

    #[test]
    fn a_line_is_shown_with_the_text_that_was_found_marked() {
        let dir = Scratch::new("go-line");
        let a = dir.file("a.txt", "one\ntwo needle\n");
        let mut space = Workspace::new(dir.0.clone());
        let target = Target::File {
            path: a,
            at: Some((2, 4)),
        };
        apply(&mut space, &hit(target, 0..6), notify());
        let active = space.active_document().expect("shown");
        assert_eq!(active.controller.take_locate(), Some(4 + 4..4 + 10));
    }

    #[test]
    fn a_definition_is_shown_where_the_server_said() {
        let dir = Scratch::new("go-place");
        let a = dir.file("a.rs", "fn main() {}\n");
        let mut space = Workspace::new(dir.0.clone());
        let place = Place {
            path: a,
            at: Position {
                line: 0,
                character: 3,
            },
            end: Position {
                line: 0,
                character: 7,
            },
        };
        apply(&mut space, &hit(Target::Place(place), 0..0), notify());
        assert_eq!(
            space
                .active_document()
                .expect("shown")
                .controller
                .take_locate(),
            Some(3..7)
        );
    }

    #[test]
    fn a_file_that_is_gone_changes_nothing() {
        let dir = Scratch::new("go-gone");
        let mut space = Workspace::new(dir.0.clone());
        let target = Target::File {
            path: dir.0.join("gone.txt"),
            at: Some((1, 0)),
        };
        assert_eq!(
            apply(&mut space, &hit(target, 0..1), notify()),
            Later::Nothing
        );
        assert!(space.documents.is_empty());
    }

    #[test]
    fn the_actions_of_the_workspace_are_done_and_the_ones_that_read_the_disk_are_left_for_later() {
        let dir = Scratch::new("go-actions");
        let mut space = Workspace::new(dir.0.clone());
        let act =
            |space: &mut Workspace, id| apply(space, &hit(Target::Action(id), 0..0), notify());
        assert_eq!(act(&mut space, ActionId::OpenSettings), Later::Nothing);
        assert!(space.settings_open);
        assert!(!space.problems_open);
        act(&mut space, ActionId::ToggleProblems);
        assert!(space.problems_open);
        act(&mut space, ActionId::ToggleProblems);
        assert!(!space.problems_open);
        assert_eq!(act(&mut space, ActionId::SaveFile), Later::Save);
        assert_eq!(act(&mut space, ActionId::ReloadFolder), Later::Reload);
        assert_eq!(act(&mut space, ActionId::CollapseFolders), Later::Nothing);
    }
}
