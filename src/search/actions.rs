//! The things the editor can be told to do, which can be found by name like a file.

use super::{Match, Pattern};

/// What an action does. What it does to the window is not known here: the screen that has the workspace carries it out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionId {
    OpenSettings,
    ToggleProblems,
    SaveFile,
    CollapseFolders,
    ReloadFolder,
}

/// An action as it is listed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Action {
    pub id: ActionId,
    pub title: &'static str,
    /// Where it is among the things the editor does, written beside it.
    pub group: &'static str,
}

impl Action {
    /// Every action, in the order they are listed when they fit equally well.
    pub const ALL: [Action; 5] = [
        Action { id: ActionId::OpenSettings, title: "Open Settings", group: "Editor" },
        Action { id: ActionId::SaveFile, title: "Save File", group: "File" },
        Action { id: ActionId::ToggleProblems, title: "Toggle Problems Panel", group: "View" },
        Action { id: ActionId::CollapseFolders, title: "Collapse All Folders", group: "View" },
        Action { id: ActionId::ReloadFolder, title: "Reload Folder from Disk", group: "File" },
    ];

    /// The actions whose titles fit `pattern`, the best first, with where in the title the letters are. None for no letters.
    pub fn find(pattern: &Pattern) -> Vec<(Action, Match)> {
        if pattern.is_empty() {
            return Vec::new();
        }
        let mut fits: Vec<(Action, Match)> = Self::ALL
            .iter()
            .filter_map(|action| pattern.matches(action.title).map(|found| (*action, found)))
            .collect();
        // A stable sort: the ones that fit equally well stay in the order of the list.
        fits.sort_by_key(|(_, found)| std::cmp::Reverse(found.score));
        fits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_action_is_found_by_the_letters_of_its_title() {
        let found = Action::find(&Pattern::new("settings"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0.id, ActionId::OpenSettings);
        assert_eq!(&found[0].0.title[found[0].1.ranges[0].clone()], "Settings");
        let found = Action::find(&Pattern::new("tpp"));
        assert_eq!(found[0].0.id, ActionId::ToggleProblems);
    }

    #[test]
    fn no_letters_and_letters_that_fit_nothing_find_no_action() {
        assert!(Action::find(&Pattern::new("")).is_empty());
        assert!(Action::find(&Pattern::new("zzzz")).is_empty());
    }

    #[test]
    fn the_better_fit_comes_first_and_the_titles_are_all_different() {
        let found = Action::find(&Pattern::new("all"));
        assert_eq!(found[0].0.id, ActionId::CollapseFolders);
        let mut titles: Vec<_> = Action::ALL.iter().map(|action| action.title).collect();
        titles.sort_unstable();
        titles.dedup();
        assert_eq!(titles.len(), Action::ALL.len());
    }
}
