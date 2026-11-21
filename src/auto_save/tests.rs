use std::cell::RefCell;
use std::path::PathBuf;

use super::*;

/// A folder of its own for one test, which is removed with it.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("refractor-autosave-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("the temporary folder can be made");
        Self(path)
    }

    /// A document of the file `name` that has `text`, and that has been changed.
    fn dirty(&self, id: u64, name: &str) -> Rc<Document> {
        let path = self.0.join(name);
        std::fs::write(&path, "old").expect("the file is written");
        let document = Document::open(id, &path, Rc::new(|| {})).expect("opens");
        document.controller.set_text("new");
        assert!(document.is_dirty());
        document
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The service, saving with what records which document it was asked to save, by its id, and makes it unchanged as a save does.
fn service(mode: AutoSaveMode, delay: Duration) -> (Rc<AutoSave>, Rc<RefCell<Vec<u64>>>) {
    let saved: Rc<RefCell<Vec<u64>>> = Rc::default();
    let record = Rc::clone(&saved);
    let service = AutoSave::new();
    service.configure(mode, delay);
    service.set_saver(Rc::new(move |document| {
        record.borrow_mut().push(document.id);
        document.save().expect("saves");
    }));
    (service, saved)
}

#[test]
fn nothing_is_saved_by_itself_while_it_is_off() {
    let scratch = Scratch::new("off");
    let (service, saved) = service(AutoSaveMode::Off, Duration::from_millis(100));
    let document = scratch.dirty(1, "a.rs");
    assert_eq!(service.edited(&document), None, "no wait is begun");
    service.focus_lost(&[Rc::clone(&document)]);
    service.switched(Some(2), vec![Rc::clone(&document)]);
    assert!(saved.borrow().is_empty());
    assert!(document.is_dirty());
}

#[test]
fn a_change_is_saved_when_the_delay_has_passed_without_another() {
    let scratch = Scratch::new("delay");
    let (service, saved) = service(AutoSaveMode::Delay, Duration::from_millis(100));
    let document = scratch.dirty(1, "a.rs");
    let first = service.edited(&document).expect("a wait is begun");
    // Another change comes before the first wait is over: that wait is for nothing, and the later one is the one.
    let second = service.edited(&document).expect("and another");
    assert_ne!(first, second);
    service.expired(&document, first);
    assert!(saved.borrow().is_empty(), "the first wait was overtaken");
    assert!(document.is_dirty());
    service.expired(&document, second);
    assert_eq!(*saved.borrow(), [1]);
    assert!(!document.is_dirty());
    service.expired(&document, second);
    assert_eq!(*saved.borrow(), [1], "once: a document that was saved is not saved again");
}

#[test]
fn the_waits_of_different_documents_do_not_overtake_each_other() {
    let scratch = Scratch::new("two");
    let (service, saved) = service(AutoSaveMode::Delay, Duration::from_millis(100));
    let (a, b) = (scratch.dirty(1, "a.rs"), scratch.dirty(2, "b.rs"));
    let (wait_a, wait_b) = (service.edited(&a).unwrap(), service.edited(&b).unwrap());
    service.expired(&a, wait_a);
    service.expired(&b, wait_b);
    assert_eq!(*saved.borrow(), [1, 2]);
}

#[test]
fn a_wait_that_ends_after_the_way_was_changed_saves_nothing() {
    let scratch = Scratch::new("changed");
    let (service, saved) = service(AutoSaveMode::Delay, Duration::from_millis(100));
    let document = scratch.dirty(1, "a.rs");
    let wait = service.edited(&document).unwrap();
    service.configure(AutoSaveMode::Off, Duration::from_millis(100));
    service.expired(&document, wait);
    assert!(saved.borrow().is_empty(), "the user turned it off in the meantime");
    service.configure(AutoSaveMode::FocusLost, Duration::from_millis(100));
    service.expired(&document, wait);
    assert!(saved.borrow().is_empty(), "and focus lost is not a wait");
}

#[test]
fn what_is_not_changed_is_not_saved() {
    let scratch = Scratch::new("clean");
    let (service, saved) = service(AutoSaveMode::Delay, Duration::from_millis(100));
    let document = scratch.dirty(1, "a.rs");
    let wait = service.edited(&document).unwrap();
    document.save().expect("the user saved it");
    service.expired(&document, wait);
    assert!(saved.borrow().is_empty(), "nothing to write");
}

#[test]
fn a_change_is_not_saved_after_a_delay_when_the_way_is_focus_lost() {
    let scratch = Scratch::new("focus-no-wait");
    let (service, _saved) = service(AutoSaveMode::FocusLost, Duration::from_millis(100));
    assert_eq!(service.edited(&scratch.dirty(1, "a.rs")), None);
}

#[test]
fn every_change_is_saved_when_the_focus_is_lost_and_what_was_not_changed_is_left() {
    let scratch = Scratch::new("focus");
    let (service, saved) = service(AutoSaveMode::FocusLost, Duration::from_millis(100));
    let (a, b) = (scratch.dirty(1, "a.rs"), scratch.dirty(2, "b.rs"));
    let clean = Document::open(3, &scratch.0.join("a.rs"), Rc::new(|| {})).expect("opens");
    assert!(!clean.is_dirty());
    service.focus_lost(&[Rc::clone(&a), Rc::clone(&clean), Rc::clone(&b)]);
    assert_eq!(*saved.borrow(), [1, 2]);
    service.focus_lost(&[a, b]);
    assert_eq!(*saved.borrow(), [1, 2], "once");
}

#[test]
fn the_focus_is_lost_by_a_file_when_another_is_shown_and_the_one_shown_is_still_being_edited() {
    let scratch = Scratch::new("switch");
    let (service, saved) = service(AutoSaveMode::FocusLost, Duration::from_millis(100));
    let (a, b) = (scratch.dirty(1, "a.rs"), scratch.dirty(2, "b.rs"));
    let documents = vec![Rc::clone(&a), Rc::clone(&b)];
    service.switched_now(Some(1), &documents);
    assert!(saved.borrow().is_empty(), "the first one shown: no other had it");
    service.switched_now(Some(1), &documents);
    assert!(saved.borrow().is_empty(), "and the same one shown again is not a change");
    service.switched_now(Some(2), &documents);
    assert_eq!(*saved.borrow(), [1], "the one that was shown goes, and the one that is shown stays");
    assert!(b.is_dirty());
}

#[test]
fn a_tab_that_is_not_a_file_loses_the_focus_of_the_files_too() {
    let scratch = Scratch::new("settings-tab");
    let (service, saved) = service(AutoSaveMode::FocusLost, Duration::from_millis(100));
    let a = scratch.dirty(1, "a.rs");
    service.switched_now(Some(1), &[Rc::clone(&a)]);
    service.switched_now(Some(0), &[Rc::clone(&a)]);
    assert_eq!(*saved.borrow(), [1], "the settings are not a file");
    service.switched_now(None, &[a]);
}

#[test]
fn nothing_is_saved_where_there_is_nothing_to_save_with() {
    let scratch = Scratch::new("no-saver");
    let service = AutoSave::new();
    service.configure(AutoSaveMode::FocusLost, Duration::from_millis(100));
    let document = scratch.dirty(1, "a.rs");
    service.focus_lost(&[Rc::clone(&document)]);
    assert!(document.is_dirty(), "no saver, and so no save, and no panic");
}
