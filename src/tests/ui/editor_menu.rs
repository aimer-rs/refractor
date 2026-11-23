use super::*;

fn facts(name: bool, problem: bool) -> Facts {
    Facts {
        name: name.then_some(4..9),
        problem,
    }
}

fn flat(layout: &Layout) -> Vec<(Action, bool)> {
    layout
        .before
        .iter()
        .chain(&layout.after)
        .map(|row| (row.action, row.enabled))
        .collect()
}

#[test]
fn over_a_name_with_a_problem_every_row_can_be_chosen() {
    let layout = layout(&facts(true, true));
    assert_eq!(
        flat(&layout),
        [
            (Action::ShowContextActions, true),
            (Action::FindUsages, true),
            (Action::GoToDefinition, true),
            (Action::Rename, true),
        ]
    );
}

#[test]
fn the_context_actions_come_first_and_the_rest_after_the_clipboard_verbs() {
    let layout = layout(&facts(true, true));
    assert_eq!(
        layout
            .before
            .iter()
            .map(|row| row.action)
            .collect::<Vec<_>>(),
        [Action::ShowContextActions]
    );
    assert_eq!(
        layout
            .after
            .iter()
            .map(|row| row.action)
            .collect::<Vec<_>>(),
        [Action::FindUsages, Action::GoToDefinition, Action::Rename]
    );
}

#[test]
fn where_there_is_no_name_what_is_about_a_name_is_listed_and_cannot_be_chosen() {
    let layout = layout(&facts(false, false));
    assert_eq!(
        flat(&layout),
        [
            (Action::ShowContextActions, false),
            (Action::FindUsages, false),
            (Action::GoToDefinition, false),
            (Action::Rename, false),
        ],
        "the rows are always there, so that a menu does not change shape under the one who reads it"
    );
}

#[test]
fn the_context_actions_need_a_problem_and_the_rest_need_a_name() {
    let no_problem = layout(&facts(true, false));
    assert!(!no_problem.before[0].enabled);
    assert!(no_problem.after.iter().all(|row| row.enabled));
    // A problem on something that is not a name: the actions are of the problem and not of a name.
    let no_name = layout(&facts(false, true));
    assert!(no_name.before[0].enabled);
    assert!(no_name.after.iter().all(|row| !row.enabled));
}

#[test]
fn the_labels_are_what_the_rows_do() {
    assert_eq!(Action::ShowContextActions.label(), "Show Context Actions");
    assert_eq!(Action::FindUsages.label(), "Find Usages");
    assert_eq!(Action::GoToDefinition.label(), "Go to Definition");
    assert_eq!(Action::Rename.label(), "Rename\u{2026}");
}

#[test]
fn a_name_is_found_by_the_byte_under_the_pointer_and_else_by_the_start_of_the_selection() {
    // A text where the name is at 4..9.
    let at = |byte: usize| (4..9).contains(&byte).then_some(4..9);
    assert_eq!(
        name_of(&at, Some(6), &(0..0)),
        Some(4..9),
        "under the pointer"
    );
    assert_eq!(
        name_of(&at, Some(20), &(6..8)),
        Some(4..9),
        "the pointer was on no name, and what is selected is one"
    );
    assert_eq!(
        name_of(&at, None, &(4..9)),
        Some(4..9),
        "a finger has no pointer: the selection"
    );
    assert_eq!(name_of(&at, None, &(5..5)), Some(4..9), "or the caret");
    assert_eq!(name_of(&at, Some(20), &(30..30)), None);
    assert_eq!(name_of(&at, None, &(30..40)), None);
}

#[test]
fn only_what_a_gesture_really_does_is_hinted_as_that_gesture() {
    // Go to Definition is the jump key and a click, which the text does too.
    assert_eq!(Action::GoToDefinition.hint(true), Some("\u{2318}Click"));
    assert_eq!(Action::GoToDefinition.hint(false), Some("Ctrl+Click"));
    // The others have no key of their own to say: Alt+Return lists the fixes of a card that is open, and does not open one.
    for action in [
        Action::ShowContextActions,
        Action::FindUsages,
        Action::Rename,
    ] {
        assert_eq!(action.hint(true), None, "{action:?}");
        assert_eq!(action.hint(false), None, "{action:?}");
    }
}

#[test]
fn each_row_has_an_icon_of_its_own() {
    let sources: Vec<usize> = [
        Action::ShowContextActions,
        Action::FindUsages,
        Action::GoToDefinition,
        Action::Rename,
    ]
    .iter()
    .map(|action| action.icon().source().as_ptr() as usize)
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
