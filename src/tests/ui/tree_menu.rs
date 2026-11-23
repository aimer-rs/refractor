use super::*;
use aimer::shortcut::KeyChord;

#[test]
fn on_a_file_or_folder_every_row_except_paste_without_clipboard_can_be_chosen() {
    let rows = layout(1, true);
    assert_eq!(rows.len(), Action::ALL.len());
    assert!(rows.iter().all(|row| row.enabled));

    let without_clip: Vec<(Action, bool)> = layout(1, false)
        .into_iter()
        .map(|row| (row.action, row.enabled))
        .collect();
    assert_eq!(
        without_clip,
        [
            (Action::NewFile, true),
            (Action::NewFolder, true),
            (Action::Cut, true),
            (Action::Copy, true),
            (Action::Paste, false),
            (Action::CopyPath, true),
            (Action::ShowInFileManager, true),
            (Action::Rename, true),
            (Action::Delete, true),
        ]
    );
}

#[test]
fn on_a_multi_selection_rename_is_disabled_while_bulk_actions_stay_enabled() {
    let rows: Vec<(Action, bool)> = layout(3, false)
        .into_iter()
        .map(|row| (row.action, row.enabled))
        .collect();
    assert_eq!(
        rows,
        [
            (Action::NewFile, true),
            (Action::NewFolder, true),
            (Action::Cut, true),
            (Action::Copy, true),
            (Action::Paste, false),
            (Action::CopyPath, true),
            (Action::ShowInFileManager, true),
            (Action::Rename, false),
            (Action::Delete, true),
        ]
    );
}

#[test]
fn on_the_blank_space_what_acts_on_an_entry_is_listed_and_cannot_be_chosen() {
    let rows: Vec<(Action, bool)> = layout(0, false)
        .into_iter()
        .map(|row| (row.action, row.enabled))
        .collect();
    assert_eq!(
        rows,
        [
            (Action::NewFile, true),
            (Action::NewFolder, true),
            (Action::Cut, false),
            (Action::Copy, false),
            (Action::Paste, false),
            (Action::CopyPath, true),
            (Action::ShowInFileManager, true),
            (Action::Rename, false),
            (Action::Delete, false),
        ],
        "the rows stay in place, and only what needs an entry or clipboard is disabled"
    );
    let with_clip = layout(0, true);
    assert!(
        with_clip
            .iter()
            .find(|row| row.action == Action::Paste)
            .expect("Paste row")
            .enabled,
        "Paste is enabled on blank space when the file clipboard has an entry"
    );
}

#[test]
fn the_groups_are_set_apart_by_lines_before_the_clipboard_the_file_manager_and_the_mutations() {
    let separated: Vec<Action> = layout(1, true)
        .into_iter()
        .filter(|row| row.separated)
        .map(|row| row.action)
        .collect();
    assert_eq!(
        separated,
        [Action::Cut, Action::ShowInFileManager, Action::Rename]
    );
}

#[test]
fn the_labels_name_finder_on_a_mac_and_explorer_elsewhere() {
    assert_eq!(Action::NewFile.label(true), "New File");
    assert_eq!(Action::NewFolder.label(true), "New Folder");
    assert_eq!(Action::Cut.label(true), "Cut");
    assert_eq!(Action::Copy.label(true), "Copy");
    assert_eq!(Action::Paste.label(true), "Paste");
    assert_eq!(Action::CopyPath.label(true), "Copy Path");
    assert_eq!(Action::ShowInFileManager.label(true), "Show in Finder");
    assert_eq!(Action::ShowInFileManager.label(false), "Show in Explorer");
    assert_eq!(Action::Rename.label(true), "Rename\u{2026}");
    assert_eq!(Action::Delete.label(true), "Delete");
}

#[test]
fn every_chord_parses_and_every_row_has_a_hint_for_both_platforms() {
    for action in Action::ALL {
        if let Some(chord) = action.chord() {
            assert!(KeyChord::parse(chord).is_ok(), "{action:?}: {chord}");
        }
        assert!(
            action
                .hint(true)
                .is_some_and(|hint| !hint.is_empty()),
            "{action:?} mac"
        );
        assert!(
            action
                .hint(false)
                .is_some_and(|hint| !hint.is_empty()),
            "{action:?} non-mac"
        );
    }
    assert_eq!(Action::Cut.chord(), None, "Edit owns Cut");
    assert_eq!(Action::Copy.chord(), None, "Edit owns Copy");
    assert_eq!(Action::Paste.chord(), None, "Edit owns Paste");
}

#[test]
fn only_the_file_manager_and_mutation_groups_have_icons_and_each_is_distinct() {
    let has_icon: Vec<(Action, bool)> = Action::ALL
        .into_iter()
        .map(|action| (action, action.icon().is_some()))
        .collect();
    assert_eq!(
        has_icon,
        [
            (Action::NewFile, false),
            (Action::NewFolder, false),
            (Action::Cut, false),
            (Action::Copy, false),
            (Action::Paste, false),
            (Action::CopyPath, false),
            (Action::ShowInFileManager, true),
            (Action::Rename, true),
            (Action::Delete, true),
        ]
    );
    let sources: Vec<usize> = Action::ALL
        .iter()
        .filter_map(|action| {
            action
                .icon()
                .map(|icon| icon.source().as_ptr() as usize)
        })
        .collect();
    let mut distinct = sources.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(
        distinct.len(),
        sources.len(),
        "no two rows are drawn with the same picture"
    );
}
