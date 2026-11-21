//! Finds the program of a language server. An app opened from the desktop does not get the `PATH` of
//! a shell, so a server installed with cargo, rustup or Homebrew is not found by its name alone.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Where servers are usually installed, besides `PATH`: folders of the machine, then folders under home.
const SYSTEM_FOLDERS: [&str; 3] = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"];
const HOME_FOLDERS: [&str; 2] = [".cargo/bin", ".local/bin"];

/// The program to run for `program`: the name itself when it holds a path or nothing better is found
/// (the system then looks in its own `PATH`), otherwise the first executable file of that name in the
/// folders of `path`, then in the usual install folders, then in the toolchains of rustup under `home`.
pub fn resolve(program: &str, path: Option<OsString>, home: Option<&Path>) -> PathBuf {
    if Path::new(program).components().count() > 1 {
        return PathBuf::from(program);
    }
    let from_path = path
        .iter()
        .flat_map(std::env::split_paths)
        .collect::<Vec<_>>();
    let system = SYSTEM_FOLDERS.iter().map(PathBuf::from);
    let under_home = home
        .into_iter()
        .flat_map(|home| HOME_FOLDERS.iter().map(move |folder| home.join(folder)));
    let toolchains = home.into_iter().flat_map(rustup_toolchain_bins);
    from_path
        .into_iter()
        .chain(system)
        .chain(under_home)
        .chain(toolchains)
        .map(|folder| folder.join(program))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| PathBuf::from(program))
}

/// The `bin` folders of the toolchains rustup installed, in the order the folders are listed.
fn rustup_toolchain_bins(home: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(home.join(".rustup/toolchains")) else {
        return Vec::new();
    };
    let mut bins: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("bin"))
        .collect();
    bins.sort();
    bins
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Scratch;

    #[test]
    fn a_program_is_found_in_path_then_in_the_install_folders_under_home() {
        let scratch = Scratch::new("locate");
        let on_path = scratch.file("path/server", "");
        let in_cargo = scratch.file("home/.cargo/bin/other", "");
        let in_toolchain = scratch.file("home/.rustup/toolchains/stable/bin/third", "");
        let path = std::env::join_paths([scratch.0.join("path")]).ok();
        let home = scratch.0.join("home");

        assert_eq!(resolve("server", path.clone(), Some(&home)), on_path);
        assert_eq!(resolve("other", path.clone(), Some(&home)), in_cargo);
        assert_eq!(resolve("third", path, Some(&home)), in_toolchain);
    }

    #[test]
    fn a_program_that_is_not_found_or_is_a_path_is_left_as_it_is() {
        assert_eq!(
            resolve("no-such-server-here", None, None),
            PathBuf::from("no-such-server-here")
        );
        assert_eq!(resolve("/x/server", None, None), PathBuf::from("/x/server"));
    }
}
