use crate::workspace::{Workspace, notifier, refresh_status};
use aimer::events::file_open::{self, FileGuard};
use aimer::{
    AnyWidget, BuildContext, ChildBuilder, ProviderHandle, State, StateView, StatefulWidget, Venus,
    Widget,
};
use std::path::{Path, PathBuf};

/// What a file may be called to be opened in the editor.
const SOURCE_EXTENSIONS: [&str; 5] = ["rs", "toml", "md", "markdown", "txt"];

/// Opens the files the app is asked to open: one dropped on the app's icon, chosen in "Open With", or
/// double-clicked. The kinds of file the system offers the app are declared under `[[documents]]` in
/// `Aimer.toml`.
///
/// It draws only its child. The file that *launched* the app arrived before there was a tree to take
/// it, so it is claimed here as the first screen is built.
#[derive(Clone, StatefulWidget)]
pub struct FileOpener {
    child: ChildBuilder,
}

impl FileOpener {
    pub fn new(child: impl Widget + 'static) -> Self {
        Self {
            child: ChildBuilder::from_widget(child),
        }
    }
}

struct FileOpenerState {
    child: ChildBuilder,
    /// Keeps the handler registered for as long as the app has this screen.
    registration: Option<FileGuard>,
}

impl State<FileOpener> for FileOpenerState {
    fn create(widget: FileOpener) -> Self {
        Self {
            child: widget.child,
            registration: None,
        }
    }

    fn init_state(&mut self) {
        let store = ProviderHandle::<Workspace>::of(BuildContext::current().get());

        let handler_store = store.clone();
        self.registration = Some(file_open::register(move |paths: &[PathBuf]| {
            open(&handler_store, paths)
        }));

        // With nothing open, the folder's own entry point is: an editor that starts on an empty page
        // has nothing to show of itself.
        let notify = notifier(&store);
        store.update(|space| {
            let main = space
                .root
                .join("src")
                .join("main.rs");
            if space.documents.is_empty() && main.is_file() {
                let _ = space.open(&main, notify);
            }
        });

        refresh_status(&store);

        // The store is not changed while the tree is still being built: the file that launched the app
        // is claimed once this frame is done.
        if let Some(venus) = Venus::current() {
            venus.spawn(async move {
                file_open::claim_waiting_files(|paths| open(&store, paths));
            });
        }
    }

    fn adopt_config_from(&mut self, new: Self) {
        self.child = new.child;
    }

    fn build(this: StateView<Self>, _ctx: &BuildContext) -> impl Widget {
        AnyWidget::from(this.child.clone().boxed())
    }
}

/// Opens what `paths` hold: a folder becomes the folder of the tree, and a file that is a source file
/// opens in a tab. Returns whether any was taken. A path that is neither is left for whatever else
/// may take it.
fn open(store: &ProviderHandle<Workspace>, paths: &[PathBuf]) -> bool {
    let mut taken = false;
    for path in paths {
        if path.is_dir() {
            store.update(|space| space.set_root(path.clone()));
            refresh_status(store);
            taken = true;
        } else if is_source(path) {
            let notify = notifier(store);
            store.update(|space| taken |= space.open(path, notify).is_ok());
        }
    }
    taken
}

fn is_source(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            SOURCE_EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_files_are_what_the_app_offers_to_open() {
        assert!(is_source(Path::new("/a/main.rs")));
        assert!(is_source(Path::new("/a/Cargo.TOML")));
        assert!(is_source(Path::new("c.txt")));
        assert!(!is_source(Path::new("/a/picture.png")));
        assert!(!is_source(Path::new("/a/rs")));
    }
}
