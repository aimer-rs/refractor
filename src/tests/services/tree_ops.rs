use super::*;
use crate::model::tree::Scratch;

#[test]
fn valid_and_invalid_names_are_distinguished() {
    assert_eq!(
        validate_name("  foo.rs  ", None),
        NameVerdict::Valid("foo.rs".into())
    );
    assert_eq!(validate_name("foo.rs", Some("foo.rs")), NameVerdict::Same);
    assert_eq!(
        validate_name("bar.rs", Some("foo.rs")),
        NameVerdict::Valid("bar.rs".into())
    );
    assert_eq!(
        validate_name("", None),
        NameVerdict::Invalid(NameError::EMPTY)
    );
    assert_eq!(
        validate_name("   ", None),
        NameVerdict::Invalid(NameError::EMPTY)
    );
    assert_eq!(
        validate_name(".", None),
        NameVerdict::Invalid(NameError::DOTS)
    );
    assert_eq!(
        validate_name("..", None),
        NameVerdict::Invalid(NameError::DOTS)
    );
    assert_eq!(
        validate_name("a/b", None),
        NameVerdict::Invalid(NameError::SEPARATOR)
    );
    assert_eq!(
        validate_name("a\\b", None),
        NameVerdict::Invalid(NameError::SEPARATOR)
    );
    assert_eq!(
        validate_name("a\0b", None),
        NameVerdict::Invalid(NameError::NUL)
    );
    assert_eq!(
        validate_name("日本語.rs", None),
        NameVerdict::Valid("日本語.rs".into())
    );
}

#[test]
fn rename_selection_selects_stem_for_files_and_whole_name_for_folders_and_dotfiles() {
    assert_eq!(rename_selection("main.rs", false), (0, 4));
    assert_eq!(rename_selection("archive.tar.gz", false), (0, 11));
    assert_eq!(rename_selection("Makefile", false), (0, 8));
    assert_eq!(rename_selection(".gitignore", false), (0, 10));
    assert_eq!(rename_selection(".env.local", false), (0, 4));
    assert_eq!(rename_selection("my.folder", true), (0, 9));
}

#[test]
fn target_dir_picks_folder_itself_parent_of_file_or_root() {
    let root = Path::new("/work/repo");
    assert_eq!(target_dir(root, None), root);
    assert_eq!(
        target_dir(root, Some((Path::new("/work/repo/src"), true))),
        Path::new("/work/repo/src")
    );
    assert_eq!(
        target_dir(root, Some((Path::new("/work/repo/src/main.rs"), false))),
        Path::new("/work/repo/src")
    );
}

#[test]
fn creating_files_and_folders_rejects_collisions() {
    let scratch = Scratch::new("tree-ops-create");
    let file = create_file(&scratch.0, "hello.rs").unwrap();
    assert!(file.is_file());
    assert_eq!(
        create_file(&scratch.0, "hello.rs")
            .unwrap_err()
            .kind(),
        io::ErrorKind::AlreadyExists
    );

    let folder = create_folder(&scratch.0, "sub").unwrap();
    assert!(folder.is_dir());
    assert_eq!(
        create_folder(&scratch.0, "sub")
            .unwrap_err()
            .kind(),
        io::ErrorKind::AlreadyExists
    );
}

#[test]
fn renaming_updates_disk_and_refuses_to_overwrite_existing() {
    let scratch = Scratch::new("tree-ops-rename");
    let a = scratch.file("a.rs", "fn a() {}");
    let _b = scratch.file("b.rs", "fn b() {}");

    assert_eq!(
        rename_entry(&a, "b.rs").unwrap_err().kind(),
        io::ErrorKind::AlreadyExists
    );
    let renamed = rename_entry(&a, "c.rs").unwrap();
    assert!(!a.exists());
    assert_eq!(fs::read_to_string(&renamed).unwrap(), "fn a() {}");
}

#[test]
fn deleting_removes_files_and_non_empty_folders() {
    let scratch = Scratch::new("tree-ops-delete");
    let a = scratch.file("a.rs", "text");
    let nested = scratch.file("dir/inner/b.rs", "nested");
    let dir = scratch.0.join("dir");

    delete_entry(&a).unwrap();
    assert!(!a.exists());

    delete_entry(&dir).unwrap();
    assert!(!dir.exists());
    assert!(!nested.exists());
}

#[test]
fn copying_files_and_folders_deduplicates_names_and_refuses_self_copy() {
    let scratch = Scratch::new("tree-ops-copy");
    let a = scratch.file("a.rs", "hello");

    let first = copy_entry(&a, &scratch.0).unwrap();
    assert_eq!(first.file_name().unwrap(), "a copy.rs");
    assert_eq!(fs::read_to_string(&first).unwrap(), "hello");

    let second = copy_entry(&a, &scratch.0).unwrap();
    assert_eq!(second.file_name().unwrap(), "a copy 2.rs");

    let nested = scratch.file("dir/sub/x.txt", "deep");
    let dir = scratch.0.join("dir");
    let dir_copy = copy_entry(&dir, &scratch.0).unwrap();
    assert_eq!(dir_copy.file_name().unwrap(), "dir copy");
    assert_eq!(
        fs::read_to_string(dir_copy.join("sub/x.txt")).unwrap(),
        "deep"
    );
    assert!(nested.exists());

    assert_eq!(
        copy_entry(&dir, &dir).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(
        copy_entry(&dir, &dir.join("sub"))
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
fn moving_relocates_entries_and_refuses_overwrite_or_self_move() {
    let scratch = Scratch::new("tree-ops-move");
    let a = scratch.file("a.rs", "moved");
    let target = create_folder(&scratch.0, "dest").unwrap();

    let moved = move_entry(&a, &target).unwrap();
    assert!(!a.exists());
    assert_eq!(moved, target.join("a.rs"));
    assert_eq!(fs::read_to_string(&moved).unwrap(), "moved");

    // Moving to the same directory is a no-op.
    assert_eq!(move_entry(&moved, &target).unwrap(), moved);

    // Moving over an existing file fails.
    let other = scratch.file("a.rs", "other");
    assert_eq!(
        move_entry(&other, &target).unwrap_err().kind(),
        io::ErrorKind::AlreadyExists
    );

    // Moving a folder into itself fails.
    let sub = create_folder(&target, "child").unwrap();
    assert_eq!(
        move_entry(&target, &sub).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
fn can_drop_rejects_same_parent_self_descendant_and_collisions() {
    let scratch = Scratch::new("tree-ops-can-drop");
    let a = scratch.file("a.rs", "");
    let sub = create_folder(&scratch.0, "sub").unwrap();
    let deep = create_folder(&sub, "deep").unwrap();
    let _existing = scratch.file("sub/b.rs", "");
    let b = scratch.file("b.rs", "");

    assert!(can_drop(&a, &sub), "file into another folder");
    assert!(can_drop(&a, &deep), "file into nested folder");
    assert!(!can_drop(&a, &scratch.0), "already in parent folder");
    assert!(!can_drop(&sub, &sub), "folder into itself");
    assert!(!can_drop(&sub, &deep), "folder into its descendant");
    assert!(!can_drop(&b, &sub), "name collision in destination");
}

#[test]
fn prune_descendants_drops_nested_children_and_duplicates() {
    let paths = vec![
        PathBuf::from("/repo/src"),
        PathBuf::from("/repo/src/main.rs"),
        PathBuf::from("/repo/src/deep/mod.rs"),
        PathBuf::from("/repo/README.md"),
        PathBuf::from("/repo/src"),
    ];
    assert_eq!(
        prune_descendants(&paths),
        vec![PathBuf::from("/repo/src"), PathBuf::from("/repo/README.md")]
    );
}

#[test]
fn can_drop_all_requires_non_empty_and_every_source_valid() {
    let scratch = Scratch::new("tree-ops-can-drop-all");
    let a = scratch.file("a.rs", "");
    let b = scratch.file("b.rs", "");
    let sub = create_folder(&scratch.0, "sub").unwrap();

    assert!(!can_drop_all(&[], &sub));
    assert!(can_drop_all(&[a.clone(), b.clone()], &sub));
    assert!(
        !can_drop_all(&[a, sub.clone()], &sub),
        "folder cannot drop into itself"
    );
}

#[test]
fn remap_path_replaces_exact_and_nested_prefixes_only() {
    let old = Path::new("/repo/src");
    let new = Path::new("/repo/lib");
    assert_eq!(
        remap_path(Path::new("/repo/src"), old, new),
        Some(PathBuf::from("/repo/lib"))
    );
    assert_eq!(
        remap_path(Path::new("/repo/src/main.rs"), old, new),
        Some(PathBuf::from("/repo/lib/main.rs"))
    );
    assert_eq!(remap_path(Path::new("/repo/src2/main.rs"), old, new), None);
}
