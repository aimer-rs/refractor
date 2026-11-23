use super::*;
use std::cell::Cell;

#[test]
fn opening_new_file_and_folder_prompts_creates_empty_controllers_and_notifies_watchers() {
    let prompt = TreePrompt::default();
    let changes = Rc::new(Cell::new(0));
    let counted = Rc::clone(&changes);
    prompt.on_change(Rc::new(move || {
        counted.set(counted.get() + 1);
        true
    }));

    prompt.open_new_file(PathBuf::from("/repo/src"));
    assert_eq!(changes.get(), 1);
    let asking = prompt.asking().expect("open");
    assert_eq!(
        asking.kind,
        TreePromptKind::NewFile {
            dir: PathBuf::from("/repo/src")
        }
    );
    assert_eq!(
        asking
            .controller
            .as_ref()
            .expect("controller")
            .value()
            .text(),
        ""
    );

    prompt.open_new_folder(PathBuf::from("/repo"));
    assert_eq!(changes.get(), 2);
    assert_eq!(
        prompt.asking().expect("open").kind,
        TreePromptKind::NewFolder {
            dir: PathBuf::from("/repo")
        }
    );
}

#[test]
fn opening_rename_selects_stem_for_files_and_whole_name_for_folders() {
    let prompt = TreePrompt::default();
    prompt.open_rename(PathBuf::from("/repo/main.rs"), "main.rs".into(), false);
    let asking = prompt.asking().expect("open");
    let controller = asking.controller.as_ref().expect("controller");
    assert_eq!(controller.value().text(), "main.rs");
    assert_eq!(controller.value().selection().range(), 0..4);

    prompt.open_rename(PathBuf::from("/repo/my.dir"), "my.dir".into(), true);
    let asking = prompt.asking().expect("open");
    let controller = asking.controller.as_ref().expect("controller");
    assert_eq!(controller.value().selection().range(), 0..6);
}

#[test]
fn delete_prompt_has_no_text_controller_and_complaints_clear_on_typing_or_close() {
    let prompt = TreePrompt::default();
    prompt.open_delete(PathBuf::from("/repo/a.rs"), "a.rs".into(), false);
    let asking = prompt.asking().expect("open");
    assert!(asking.controller.is_none());

    prompt.complain("Permission denied");
    assert_eq!(prompt.complaint().as_deref(), Some("Permission denied"));
    prompt.typed();
    assert_eq!(prompt.complaint(), None);

    prompt.complain("Failed");
    prompt.close();
    assert!(prompt.asking().is_none());
    assert_eq!(prompt.complaint(), None);
}

#[test]
fn close_if_same_closes_only_the_matching_prompt_instance() {
    let prompt = TreePrompt::default();
    let changes = Rc::new(Cell::new(0));
    let counted = Rc::clone(&changes);
    prompt.on_change(Rc::new(move || {
        counted.set(counted.get() + 1);
        true
    }));

    prompt.open_new_file(PathBuf::from("/repo/src"));
    let first = prompt.asking().expect("first prompt open");
    prompt.open_new_folder(PathBuf::from("/repo/src"));
    let second = prompt.asking().expect("second prompt open");
    prompt.complain("Already exists");
    let before = changes.get();

    // Dismissing the replaced first modal must not close the active second prompt.
    prompt.close_if_same(&first);
    assert!(
        prompt
            .asking()
            .is_some_and(|now| Rc::ptr_eq(&now, &second))
    );
    assert_eq!(prompt.complaint().as_deref(), Some("Already exists"));
    assert_eq!(changes.get(), before);

    // Dismissing the active modal closes it and notifies watchers.
    prompt.close_if_same(&second);
    assert!(prompt.asking().is_none());
    assert_eq!(prompt.complaint(), None);
    assert_eq!(changes.get(), before + 1);
}
