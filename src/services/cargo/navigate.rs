//! Where a click in a manifest goes: the manifest of the crate that a `path` names, the manifest of a member of the workspace, the file a
//! target or the package names, and what a feature turns on.

use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use super::scan::{Entry, EntryKind, Kind, Segment, Value};

/// Where a click goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A file, which is opened.
    File(PathBuf),
    /// A byte of the manifest itself.
    Here(usize),
}

/// A place in the text that can be gone from, and where it goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Jump {
    pub span: Range<usize>,
    pub target: Target,
}

/// The tables whose keys are targets: they name the file of their source in `path`.
const TARGETS: [&str; 5] = ["lib", "bin", "example", "test", "bench"];
const DEPENDENCIES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// Which of the segments the dependency tables end at, as the index of the name of the table: `dependencies`, `workspace.dependencies` and
/// `target.<cfg>.dependencies`.
fn dependency_table(names: &[&str]) -> Option<usize> {
    match names {
        [first, ..] if DEPENDENCIES.contains(first) => Some(0),
        ["workspace", "dependencies", ..] => Some(1),
        ["target", _, third, ..] if DEPENDENCIES.contains(third) => Some(2),
        _ => None,
    }
}

/// The text of a string between its quotes, and where it is; none for one that is not closed or is written with three quotes.
fn inner(value: &Value) -> Option<(&str, Range<usize>)> {
    if !matches!(value.kind, Kind::String) {
        return None;
    }
    let (start, end) = (value.span.start + 1, value.span.end.checked_sub(1)?);
    (start <= end && value.text.len() == end - start).then_some((value.text.as_str(), start..end))
}

/// `path` with its `.` and `..` taken out by reading it, so that a file is the same file whichever way it was named, and is not opened
/// twice. A link is not followed, which would ask the disk.
fn clean(path: PathBuf) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir if out.file_name().is_some() => {
                out.pop();
            }
            // The root has no parent: a `..` at it stays at it.
            Component::ParentDir if out.has_root() => {}
            other => out.push(other),
        }
    }
    out
}

fn names(entry: &Entry) -> Vec<&str> {
    entry
        .segments
        .iter()
        .map(|segment| segment.name.as_str())
        .collect()
}

fn name_span(segment: &Segment) -> Option<Range<usize>> {
    segment.span.clone()
}

/// The place where the crate `name` is declared as a dependency.
fn dependency(entries: &[Entry], name: &str) -> Option<usize> {
    entries.iter().find_map(|entry| {
        let path = names(entry);
        let at = dependency_table(&path)? + 1;
        (path.get(at) == Some(&name))
            .then(|| {
                entry.segments[at]
                    .span
                    .as_ref()
                    .map(|span| span.start)
            })
            .flatten()
    })
}

/// Where the feature `name` is defined, or else the dependency it is the implicit feature of.
fn feature(entries: &[Entry], name: &str) -> Option<usize> {
    // `dep:serde` and `serde/derive` are about the dependency.
    let crate_name = name.strip_prefix("dep:").or_else(|| {
        name.split_once('/')
            .map(|(left, _)| left.trim_end_matches('?'))
    });
    if let Some(crate_name) = crate_name {
        return dependency(entries, crate_name);
    }
    entries
        .iter()
        .find_map(|entry| {
            let path = names(entry);
            (entry.kind == EntryKind::Key
                && path.len() == 2
                && path[0] == "features"
                && path[1] == name)
                .then(|| {
                    entry.segments[1]
                        .span
                        .as_ref()
                        .map(|span| span.start)
                })
                .flatten()
        })
        .or_else(|| dependency(entries, name))
}

/// Where a click at `offset` of a manifest goes, where it is on a string that names a place. `dir` is the folder of the manifest.
pub fn jump_at(entries: &[Entry], offset: usize, dir: &Path) -> Option<Jump> {
    for entry in entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::Key)
    {
        let Some(value) = &entry.value else {
            continue;
        };
        let path = names(entry);
        let contains = |span: &Range<usize>| span.start <= offset && offset <= span.end;
        match &value.kind {
            Kind::String => {
                let Some((text, span)) = inner(value).filter(|(_, span)| contains(span)) else {
                    continue;
                };
                if text.is_empty() {
                    continue;
                }
                let target = match path.as_slice() {
                    [.., "path"] if dependency_table(&path).is_some() => {
                        dir.join(text).join("Cargo.toml")
                    }
                    [first, .., "path"] if TARGETS.contains(first) => dir.join(text),
                    ["package", "build" | "readme" | "license-file"] => dir.join(text),
                    _ => continue,
                };
                return Some(Jump {
                    span,
                    target: Target::File(clean(target)),
                });
            }
            Kind::Array(items) => {
                let members = path == ["workspace", "members"];
                let features = path.len() == 2 && path[0] == "features";
                if !members && !features {
                    continue;
                }
                for item in items {
                    let Some((text, span)) = inner(item).filter(|(_, span)| contains(span)) else {
                        continue;
                    };
                    let target = if members {
                        // A pattern names many folders, and a click is on none of them.
                        if text.contains(['*', '?', '[']) {
                            continue;
                        }
                        Target::File(clean(dir.join(text).join("Cargo.toml")))
                    } else {
                        let Some(at) = feature(entries, text) else {
                            continue;
                        };
                        Target::Here(at)
                    };
                    return Some(Jump { span, target });
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::cargo::scan::Scan;

    const TEXT: &str = "[package]\nname = \"app\"\nbuild = \"build.rs\"\n\n[workspace]\nmembers = [\"crates/a\", \"tools/*\"]\n\n[dependencies]\nlocal = { path = \"../local\", version = \"1\" }\nserde = \"1\"\nrand = { version = \"0.8\", optional = true }\n\n[features]\ndefault = [\"fast\", \"serde/derive\", \"rand\", \"nope\", \"dep:rand\"]\nfast = []\n\n[lib]\npath = \"src/lib.rs\"\n\n[dependencies.other]\npath = \"vendor/other\"\n";

    fn jump(needle: &str, extra: usize) -> Option<Jump> {
        let at = TEXT.find(needle).expect("in the text") + extra;
        jump_at(&Scan::of(TEXT).entries, at, Path::new("/work/app"))
    }

    fn text_of(jump: &Jump) -> &str {
        &TEXT[jump.span.clone()]
    }

    #[test]
    fn a_path_dependency_goes_to_the_manifest_of_the_crate_it_names() {
        let found = jump("../local", 3).expect("a jump");
        assert_eq!(text_of(&found), "../local");
        assert_eq!(
            found.target,
            Target::File(PathBuf::from("/work/local/Cargo.toml"))
        );
        let table = jump("vendor/other", 0).expect("a jump");
        assert_eq!(
            table.target,
            Target::File(PathBuf::from("/work/app/vendor/other/Cargo.toml"))
        );
    }

    #[test]
    fn a_path_is_cleaned_of_dots_by_reading_it() {
        assert_eq!(clean(PathBuf::from("/a/./b/../c")), PathBuf::from("/a/c"));
        assert_eq!(
            clean(PathBuf::from("/a/../../b")),
            PathBuf::from("/b"),
            "the root has no parent"
        );
        assert_eq!(clean(PathBuf::from("a/../../b")), PathBuf::from("../b"));
    }

    #[test]
    fn a_member_a_target_and_a_build_script_go_to_their_files_and_a_pattern_goes_nowhere() {
        let member = jump("crates/a", 2).expect("a jump");
        assert_eq!(
            member.target,
            Target::File(PathBuf::from("/work/app/crates/a/Cargo.toml"))
        );
        assert_eq!(jump("tools/*", 2), None);
        let lib = jump("src/lib.rs", 1).expect("a jump");
        assert_eq!(
            lib.target,
            Target::File(PathBuf::from("/work/app/src/lib.rs"))
        );
        let build = jump("build.rs", 1).expect("a jump");
        assert_eq!(
            build.target,
            Target::File(PathBuf::from("/work/app/build.rs"))
        );
    }

    #[test]
    fn a_feature_goes_to_where_it_is_defined_or_to_the_dependency_it_turns_on() {
        let fast = jump("\"fast\", ", 2).expect("a jump");
        assert_eq!(text_of(&fast), "fast");
        assert_eq!(
            fast.target,
            Target::Here(TEXT.find("fast = []").expect("defined"))
        );
        let derive = jump("serde/derive", 1).expect("a jump");
        assert_eq!(
            derive.target,
            Target::Here(TEXT.find("serde = ").expect("declared"))
        );
        let rand = jump("\"rand\", ", 2).expect("a jump");
        assert_eq!(
            rand.target,
            Target::Here(TEXT.find("rand = {").expect("declared")),
            "an implicit feature"
        );
        let dep = jump("dep:rand", 5).expect("a jump");
        assert_eq!(dep.target, rand.target);
        assert_eq!(jump("nope", 1), None, "a feature that is not there");
    }

    #[test]
    fn a_click_that_is_not_on_a_place_goes_nowhere() {
        assert_eq!(jump("app", 1), None, "the name of the package");
        assert_eq!(jump("version = \"0.8\"", 12), None, "a version");
        assert_eq!(jump_at(&[], 0, Path::new("/")), None);
        assert_eq!(
            jump_at(&Scan::of("[lib]\npath = \"").entries, 13, Path::new("/")),
            None,
            "not closed"
        );
        assert_eq!(
            jump_at(&Scan::of("[lib]\npath = \"\"").entries, 14, Path::new("/")),
            None,
            "empty"
        );
    }
}
