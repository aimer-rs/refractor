use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use super::*;

/// A folder of its own for one test, which is removed with it.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("refractor-filesync-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("the temporary folder can be made");
        Self(path)
    }

    fn open(&self, id: u64, name: &str, text: &str) -> (Rc<Document>, PathBuf) {
        let path = self.0.join(name);
        std::fs::write(&path, text).expect("the file is written");
        (
            Document::open(id, &path, Rc::new(|| {})).expect("opens"),
            path,
        )
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn elsewhere(path: &Path, text: &str) {
    std::fs::write(path, text).expect("another program writes the file");
}

#[test]
fn a_file_that_nothing_changed_is_left_alone() {
    let scratch = Scratch::new("same");
    let (document, _) = scratch.open(1, "a.rs", "one");
    assert_eq!(sync_document(&document), Synced::Same);
    assert_eq!(document.controller.value().text(), "one");
}

#[test]
fn a_file_that_something_else_changed_is_read_again_when_the_document_has_no_changes_of_its_own() {
    let scratch = Scratch::new("reload");
    let (document, path) = scratch.open(1, "a.rs", "one");
    elsewhere(&path, "two, from a formatter");
    assert_eq!(sync_document(&document), Synced::Reloaded);
    assert_eq!(document.controller.value().text(), "two, from a formatter");
    assert!(!document.is_dirty());
    assert_eq!(sync_document(&document), Synced::Same, "once");
}

#[test]
fn a_file_that_something_else_changed_and_the_document_changed_too_is_not_read_and_the_conflict_is_told_once()
 {
    let scratch = Scratch::new("conflict");
    let (document, path) = scratch.open(1, "a.rs", "one");
    document.controller.set_text("mine");
    elsewhere(&path, "theirs, which is longer");
    assert_eq!(sync_document(&document), Synced::Conflict);
    assert_eq!(
        document.controller.value().text(),
        "mine",
        "what the user wrote is not thrown away"
    );
    assert!(document.is_dirty());
    assert_eq!(
        sync_document(&document),
        Synced::Same,
        "told once, and not every time it is looked at"
    );
    // Another change of the file is another conflict.
    elsewhere(&path, "theirs, again and again");
    assert_eq!(sync_document(&document), Synced::Conflict);
}

#[test]
fn a_file_that_is_gone_is_told_once_and_the_text_stays() {
    let scratch = Scratch::new("gone");
    let (document, path) = scratch.open(1, "a.rs", "one");
    std::fs::remove_file(&path).unwrap();
    assert_eq!(sync_document(&document), Synced::Deleted);
    assert_eq!(
        document.controller.value().text(),
        "one",
        "to be saved again, if it is wanted"
    );
    assert_eq!(sync_document(&document), Synced::Same);
    // A document that has changes says the same, and keeps them.
    let (changed, other) = scratch.open(2, "b.rs", "one");
    changed.controller.set_text("mine");
    std::fs::remove_file(&other).unwrap();
    assert_eq!(sync_document(&changed), Synced::Deleted);
    assert!(changed.is_dirty());
    // And the file comes back, as another program made it.
    elsewhere(&other, "back!");
    assert_eq!(sync_document(&changed), Synced::Conflict);
}

#[test]
fn a_file_that_came_back_is_read_when_the_document_has_no_changes() {
    let scratch = Scratch::new("back");
    let (document, path) = scratch.open(1, "a.rs", "one");
    std::fs::remove_file(&path).unwrap();
    assert_eq!(sync_document(&document), Synced::Deleted);
    elsewhere(&path, "one, written again and longer");
    assert_eq!(sync_document(&document), Synced::Reloaded);
    assert_eq!(
        document.controller.value().text(),
        "one, written again and longer"
    );
}

#[test]
fn a_file_that_is_no_longer_text_is_not_read_and_is_told_once() {
    let scratch = Scratch::new("binary");
    let (document, path) = scratch.open(1, "a.rs", "one");
    std::fs::write(&path, [0xff, 0xfe, 0x00, 0x01]).unwrap();
    assert!(matches!(sync_document(&document), Synced::Failed(_)));
    assert_eq!(document.controller.value().text(), "one");
    assert_eq!(sync_document(&document), Synced::Same);
}

#[test]
fn a_change_made_by_the_save_of_the_editor_is_not_one() {
    let scratch = Scratch::new("own-save");
    let (document, _) = scratch.open(1, "a.rs", "one");
    document.controller.set_text("two");
    document.save().expect("saves");
    assert_eq!(sync_document(&document), Synced::Same);
}

#[test]
fn what_looks_at_the_files_looks_at_each_and_tells_what_was_found_of_those_that_were_not_the_same()
{
    let scratch = Scratch::new("all");
    let (a, a_path) = scratch.open(1, "a.rs", "one");
    let (b, _) = scratch.open(2, "b.rs", "one");
    let (c, c_path) = scratch.open(3, "c.rs", "one");
    c.controller.set_text("mine");
    elsewhere(&a_path, "two!");
    elsewhere(&c_path, "three!!");
    let told: RefCell<Vec<(u64, Synced)>> = RefCell::default();
    look_at(&[a, b, c], &|document, found| {
        told.borrow_mut().push((document.id, found))
    });
    assert_eq!(
        *told.borrow(),
        vec![(1, Synced::Reloaded), (3, Synced::Conflict)],
        "the one that was the same is not told"
    );
}

#[test]
fn the_loop_that_looks_ends_when_what_it_is_for_is_gone() {
    let sync = FileSync::new();
    let looked = Rc::new(Cell::new(0));
    let counted = Rc::clone(&looked);
    // No runtime, no timers: it begins nothing, and is not begun twice.
    sync.start(
        Rc::new(Vec::new),
        Rc::new(move |_, _| counted.set(counted.get() + 1)),
    );
    sync.start(Rc::new(Vec::new), Rc::new(|_, _| {}));
    assert_eq!(looked.get(), 0);
}
