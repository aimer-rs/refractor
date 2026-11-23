//! A search that is going on: what was typed, which tab is shown, what each source has found so far, and which hit is chosen.
//!
//! The work is not done while the window is drawn or a key is handled: the index of the files is made once off the UI thread,
//! what is typed goes through it off the UI thread, the text of the files is read off it after the typing has paused, and
//! the language server is asked for the definitions. What each finds is put in the list as it arrives, and a result that is
//! for what was typed before is dropped. Without a runtime to run them on (as in the tests) each runs where it is asked.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use aimer::Venus;

use super::files::split_position;
use super::{
    Action, FileHit, FileIndex, Found, Hit, Parts, Pattern, Preview, Tab, Target, TextSearch,
};
use crate::ui::components::popup::Redraw;
use crate::services::lsp::Lsp;

/// What a search lists, how much of each source is asked for, and how long typing must pause for the files to be read.
pub struct Limits;

impl Limits {
    /// How many files, definitions and lines of text are kept of what a source finds.
    pub const FILES: usize = 100;
    pub const TEXT: usize = 100;
    /// How long the typing pauses before the files are read for the text: a key typed cancels the reading that is going on, and
    /// a word that is typed fast is not searched for one letter at a time.
    pub const TEXT_PAUSE: Duration = Duration::from_millis(180);
    /// How few letters are searched for in the files: the text of one is in nearly every line.
    pub const TEXT_LETTERS: usize = 2;
}

struct State {
    query: String,
    tab: Tab,
    selected: usize,
    parts: Parts,
    hits: Rc<Vec<Hit>>,
    /// Counts the times what was typed or the tab changed, so what comes back for an older one is known.
    generation: u64,
    /// Set to stop the reading of the files that is going on.
    cancel: Arc<AtomicBool>,
    /// The request for the definitions that is waiting.
    asking: Option<i64>,
    index: Option<Arc<FileIndex>>,
    /// What each source is still doing, for the list to say so.
    indexing: bool,
    reading: bool,
    asked: bool,
    /// Why the definitions are not listed: the server is not there, or has not said.
    server_note: bool,
    shift: bool,
    preview: Option<(Target, Option<Rc<Preview>>)>,
}

pub struct Search {
    lsp: Lsp,
    state: RefCell<State>,
    redraw: Redraw,
    /// Set when the search is dropped, to stop what is running off the UI thread.
    stop: Arc<AtomicBool>,
}

impl Drop for Search {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.state
            .get_mut()
            .cancel
            .store(true, Ordering::Relaxed);
        if let Some(id) = self.state.get_mut().asking.take() {
            self.lsp.cancel(id);
        }
    }
}

/// Where the choice goes from `selected` after a key, among `len` hits: down or up, round the ends.
pub fn step(selected: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    (selected as isize + delta).rem_euclid(len as isize) as usize
}

impl Search {
    /// A search over the folder `root`, which starts reading the names of its files at once.
    pub fn new(root: PathBuf, lsp: Lsp) -> Rc<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let search = Rc::new(Self {
            lsp,
            state: RefCell::new(State {
                query: String::new(),
                tab: Tab::All,
                selected: 0,
                parts: Parts {
                    root: root.clone(),
                    ..Parts::default()
                },
                hits: Rc::default(),
                generation: 0,
                cancel: Arc::new(AtomicBool::new(false)),
                asking: None,
                index: None,
                indexing: true,
                reading: false,
                asked: false,
                server_note: false,
                shift: false,
                preview: None,
            }),
            redraw: Redraw::default(),
            stop,
        });
        search.index(root);
        search
    }

    pub fn redraw(&self) -> &Redraw {
        &self.redraw
    }

    pub fn query(&self) -> String {
        self.state.borrow().query.clone()
    }

    pub fn tab(&self) -> Tab {
        self.state.borrow().tab
    }

    pub fn hits(&self) -> Rc<Vec<Hit>> {
        Rc::clone(&self.state.borrow().hits)
    }

    pub fn selected(&self) -> usize {
        self.state.borrow().selected
    }

    /// The hit that is chosen, if there is a list.
    pub fn current(&self) -> Option<Hit> {
        let state = self.state.borrow();
        state.hits.get(state.selected).cloned()
    }

    /// Whether something is still being looked for, which the list says.
    pub fn busy(&self) -> bool {
        let state = self.state.borrow();
        state.reading || state.asked || (state.indexing && state.tab.wants_files())
    }

    /// Why a list that is empty is empty, when it is not that nothing fits: the language server cannot be asked.
    pub fn note(&self) -> Option<&'static str> {
        let state = self.state.borrow();
        (state.server_note && state.tab.wants_symbols() && !state.query.trim().is_empty())
            .then_some("The language server is not running: open a Rust file to start it")
    }

    /// Moves the choice, round the ends of the list.
    pub fn move_selection(&self, delta: isize) {
        {
            let mut state = self.state.borrow_mut();
            state.selected = step(state.selected, state.hits.len(), delta);
        }
        self.redraw.fire();
    }

    /// Chooses the hit at `index`, if it is there.
    pub fn select(&self, index: usize) {
        {
            let mut state = self.state.borrow_mut();
            if index >= state.hits.len() || state.selected == index {
                return;
            }
            state.selected = index;
        }
        self.redraw.fire();
    }

    /// Whether the key that shows the preview is held.
    pub fn shift(&self) -> bool {
        self.state.borrow().shift
    }

    pub fn set_shift(&self, held: bool) {
        {
            let mut state = self.state.borrow_mut();
            if state.shift == held {
                return;
            }
            state.shift = held;
        }
        self.redraw.fire();
    }

    /// The lines around what the chosen hit goes to, read the first time they are asked for and kept for as long as the same
    /// hit is chosen. None where there is nothing to show.
    pub fn preview(&self) -> Option<Rc<Preview>> {
        let target = self.current()?.target;
        if let Some((seen, preview)) = &self.state.borrow().preview
            && *seen == target
        {
            return preview.clone();
        }
        let preview = Preview::of(&target).map(Rc::new);
        self.state.borrow_mut().preview = Some((target, preview.clone()));
        preview
    }

    /// Shows another tab, and looks again for what is typed.
    pub fn set_tab(self: &Rc<Self>, tab: Tab) {
        {
            let mut state = self.state.borrow_mut();
            if state.tab == tab {
                return;
            }
            state.tab = tab;
        }
        self.restart();
    }

    /// Looks for what is typed, in place of what was typed before.
    pub fn set_query(self: &Rc<Self>, query: &str) {
        {
            let mut state = self.state.borrow_mut();
            if state.query == query {
                return;
            }
            state.query = query.to_owned();
        }
        self.restart();
    }

    /// Reads the names of the files of the folder, for what is typed to be looked for in them.
    fn index(self: &Rc<Self>, root: PathBuf) {
        let stop = Arc::clone(&self.stop);
        let Some(venus) = Venus::current() else {
            let index = FileIndex::build(&root, FileIndex::LIMIT, &stop);
            self.indexed(index);
            return;
        };
        let (this, blocking) = (Rc::downgrade(self), venus.clone());
        venus.spawn(async move {
            let index = blocking
                .spawn_blocking(move || FileIndex::build(&root, FileIndex::LIMIT, &stop))
                .await;
            if let Some(this) = this.upgrade() {
                this.indexed(index);
            }
        });
    }

    fn indexed(self: &Rc<Self>, index: FileIndex) {
        let generation = {
            let mut state = self.state.borrow_mut();
            state.index = Some(Arc::new(index));
            state.indexing = false;
            state.generation
        };
        if self.state.borrow().tab.wants_files() {
            self.find_files(generation);
        }
        self.redraw.fire();
    }

    /// Starts again for what is typed and the tab: what was going on for the one before is stopped.
    fn restart(self: &Rc<Self>) {
        let (generation, tab, query, pattern, asking) = {
            let mut state = self.state.borrow_mut();
            state.generation += 1;
            state.cancel.store(true, Ordering::Relaxed);
            state.cancel = Arc::new(AtomicBool::new(false));
            state.reading = false;
            state.asked = false;
            state.server_note = false;
            state.selected = 0;
            let (typed, at) = split_position(&state.query);
            let typed = typed.to_owned();
            let pattern = Pattern::new(&typed);
            state.parts = Parts {
                root: state.parts.root.clone(),
                pattern: pattern.clone(),
                at,
                ..Parts::default()
            };
            (
                state.generation,
                state.tab,
                typed,
                pattern,
                state.asking.take(),
            )
        };
        if let Some(id) = asking {
            self.lsp.cancel(id);
        }
        if pattern.is_empty() {
            self.publish();
            return;
        }
        if tab.wants_actions() {
            self.state.borrow_mut().parts.actions = Action::find(&pattern);
        }
        self.publish();
        if tab.wants_files() {
            self.find_files(generation);
        }
        if tab.wants_symbols() {
            self.ask_symbols(generation, &query);
        }
        if tab.wants_text()
            && query
                .chars()
                .filter(|c| !c.is_whitespace())
                .count()
                >= Limits::TEXT_LETTERS
        {
            self.read_text(generation, query);
        }
    }

    /// Puts what the sources have found in the list.
    fn publish(&self) {
        {
            let mut state = self.state.borrow_mut();
            state.hits = Rc::new(state.tab.combine(&state.parts));
            state.selected = state
                .selected
                .min(state.hits.len().saturating_sub(1));
            state.preview = None;
        }
        self.redraw.fire();
    }

    /// Takes what a source found, if it is for what is typed now.
    fn arrived(&self, generation: u64, apply: impl FnOnce(&mut State)) {
        {
            let mut state = self.state.borrow_mut();
            if state.generation != generation {
                return;
            }
            apply(&mut state);
        }
        self.publish();
    }

    fn find_files(self: &Rc<Self>, generation: u64) {
        let (index, pattern) = {
            let state = self.state.borrow();
            (state.index.clone(), state.parts.pattern.clone())
        };
        let Some(index) = index else {
            // The index is not made: the files are looked for when it is.
            return;
        };
        let Some(venus) = Venus::current() else {
            let files = index.find(&pattern, Limits::FILES);
            self.arrived(generation, |state| state.parts.files = files);
            return;
        };
        let (this, blocking) = (Rc::downgrade(self), venus.clone());
        venus.spawn(async move {
            let files = blocking
                .spawn_blocking(move || index.find(&pattern, Limits::FILES))
                .await;
            Self::deliver(
                &this,
                generation,
                Some(files),
                |state, files: Vec<FileHit>| state.parts.files = files,
            );
        });
    }

    fn deliver<T>(
        this: &Weak<Self>,
        generation: u64,
        found: Option<T>,
        apply: impl FnOnce(&mut State, T),
    ) {
        if let (Some(this), Some(found)) = (this.upgrade(), found) {
            this.arrived(generation, |state| apply(state, found));
        }
    }

    /// The symbols that are defined in the folder: the server also knows those of the libraries it depends on, which are
    /// in the cargo registry or the toolchain, and are not what is looked for in the project.
    fn inside(
        root: &std::path::Path,
        mut symbols: Vec<crate::services::lsp::Symbol>,
    ) -> Vec<crate::services::lsp::Symbol> {
        symbols.retain(|symbol| symbol.place.path.starts_with(root));
        symbols
    }

    fn ask_symbols(self: &Rc<Self>, generation: u64, query: &str) {
        self.state.borrow_mut().asked = true;
        let this = Rc::downgrade(self);
        let id = self
            .lsp
            .workspace_symbols(query, move |answer| {
                let Some(this) = this.upgrade() else { return };
                this.arrived(generation, |state| {
                    state.asked = false;
                    state.asking = None;
                    match answer {
                        Some(symbols) => {
                            state.parts.symbols = Self::inside(&state.parts.root, symbols)
                        }
                        None => state.server_note = true,
                    }
                });
            });
        let mut state = self.state.borrow_mut();
        // The answer may have come already, when the server could not be asked.
        if state.generation == generation && state.asked {
            state.asking = id;
        }
    }

    fn read_text(self: &Rc<Self>, generation: u64, query: String) {
        let (root, cancel) = {
            let mut state = self.state.borrow_mut();
            state.reading = true;
            (state.parts.root.clone(), Arc::clone(&state.cancel))
        };
        let search = move || TextSearch::new(&query).run(&root, Limits::TEXT, &cancel);
        let Some(venus) = Venus::current() else {
            let found = search();
            self.arrived(generation, |state| {
                state.reading = false;
                state.parts.text = found.hits;
            });
            return;
        };
        let (this, blocking, paused) = (
            Rc::downgrade(self),
            venus.clone(),
            venus.sleep(Limits::TEXT_PAUSE),
        );
        venus.spawn(async move {
            // A runtime that has no timers answers at once.
            let _ = paused.await;
            // Typing went on in the pause: this is not what is wanted any more.
            let Some(current) = this.upgrade() else {
                return;
            };
            if current.state.borrow().generation != generation {
                return;
            }
            drop(current);
            let found: Found = blocking.spawn_blocking(search).await;
            Self::deliver(&this, generation, Some(found), |state, found| {
                state.reading = false;
                state.parts.text = found.hits;
            });
        });
    }
}

#[cfg(test)]
#[path = "../../tests/services/search/model.rs"]
mod model_tests;
