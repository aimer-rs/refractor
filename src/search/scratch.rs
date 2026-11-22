//! A folder in the temporary directory for the tests of what reads folders; it is removed when it is dropped.

use std::path::{Path, PathBuf};

pub struct Scratch(PathBuf);

impl Scratch {
    /// A new empty folder. `name` makes it differ from the folders of the tests that run at the same time.
    pub fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("refractor-search-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temporary directory");
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Writes the file `path` of the folder, with its folders, and answers where it is.
    pub fn file(&self, path: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
        std::fs::write(&path, bytes).expect("written");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
