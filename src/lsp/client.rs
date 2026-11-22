//! The language server as the interface uses it: told of the files that are open and asked what is
//! known of them. It does the little that the interface has to do for the server: run it off the
//! update that opened a file, listen for what it says, and tell it of the edits a moment after the last
//! one and not on each.
//!
//! It works on the thread of the interface, through the framework's runtime, and does nothing where
//! there is none.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use aimer::Venus;
use aimer::input::{ControllerListener, TextEditingController};
use aimer::venus::{Lifecycle, LifecycleSubscription, on_lifecycle};

use super::core::{Core, Link, Starter, Summary};
use super::items::{Answer, Hover};
use super::locate;
use super::position::Position;
use super::problems::Problem;
use super::process::{Event, Mailbox, Server};
use super::protocol::Place;
use super::session::Status;

/// How long after an edit the server is told of it, if there is not another by then.
const FLUSH_AFTER: Duration = Duration::from_millis(40);

/// The longest the app waits for the server to shut down when it quits.
const QUIT_WAIT: Duration = Duration::from_millis(300);

/// The program that is run when none is named: the server of Rust.
const DEFAULT_SERVER: &str = "rust-analyzer";

/// The language server of a workspace. Cloning it shares it.
#[derive(Clone)]
pub struct Lsp {
    shared: Rc<Shared>,
}

struct Shared {
    core: RefCell<Core>,
    /// Keeps each open file's changes coming, for as long as it is open.
    listeners: RefCell<HashMap<PathBuf, ControllerListener>>,
    /// What to call when what shows the server changed, which is not from inside an update of the workspace.
    notify: RefCell<Option<Rc<dyn Fn()>>>,
    /// Whether a flush of the edits is waiting for its moment.
    flush_waiting: Cell<bool>,
    /// The request for completion that is waiting for its answer: its number, and what is called with the answer.
    /// There is one: a request makes the one before it unwanted.
    waiting: RefCell<Option<(i64, Box<dyn FnOnce(Option<Answer>)>)>>,
    /// The request for a definition that is waiting for its answer, which is one at a time as well.
    defining: RefCell<Option<(i64, Box<dyn FnOnce(Option<Vec<Place>>)>)>>,
    /// The request for what to say of a name that is waiting for its answer, which is one at a time too.
    hovering: RefCell<Option<(i64, Box<dyn FnOnce(Option<Hover>)>)>>,
    /// The request for the rest of an item that is waiting for its answer, which is one at a time like the other.
    resolving: RefCell<Option<(i64, Box<dyn FnOnce(Option<Vec<(Range<usize>, String)>>)>)>>,
    /// The requests made by the name of the method, which any number are waiting for their answers: what is called with each.
    asking: RefCell<HashMap<i64, Box<dyn FnOnce(Option<serde_json::Value>)>>>,
    /// What is called when what the server says may have changed, which say whether what they are for is still there.
    change_hooks: RefCell<Vec<Rc<dyn Fn() -> bool>>>,
    /// What is called when the server says that its tokens are not as they were, which say whether what they are for is still there.
    semantic_refresh: RefCell<Vec<Rc<dyn Fn() -> bool>>>,
    /// Whether the server is not to be run at all.
    off: bool,
    /// Keeps the server told when the app quits, for as long as there is a server.
    quitting: RefCell<Option<LifecycleSubscription>>,
}

impl Lsp {
    /// The language server of the folder `root`. The program is the one `REFRACTOR_LSP` names, or
    /// `rust-analyzer`; `off` runs none.
    pub fn new(root: PathBuf) -> Self {
        let named = std::env::var("REFRACTOR_LSP").ok();
        let off = named.as_deref() == Some("off");
        let program = named
            .filter(|program| !program.is_empty())
            .unwrap_or_else(|| DEFAULT_SERVER.to_owned());
        // Found when the server is started, not when the app is: an app opened from the desktop has a
        // `PATH` without the folders cargo, rustup and Homebrew install into.
        let starter: Starter = Box::new(move |root| {
            let home = std::env::var_os("HOME").map(PathBuf::from);
            let program = locate::resolve(&program, std::env::var_os("PATH"), home.as_deref());
            Server::spawn(&program.to_string_lossy(), &[], root)
                .map(|server| Box::new(server) as Box<dyn Link>)
        });
        Self::with_starter(root, starter, off)
    }

    /// A language server that is run by `starter`.
    pub fn with_starter(root: PathBuf, starter: Starter, off: bool) -> Self {
        Self {
            shared: Rc::new(Shared {
                core: RefCell::new(Core::new(root, starter)),
                listeners: RefCell::new(HashMap::new()),
                notify: RefCell::new(None),
                flush_waiting: Cell::new(false),
                waiting: RefCell::new(None),
                defining: RefCell::new(None),
                hovering: RefCell::new(None),
                resolving: RefCell::new(None),
                asking: RefCell::new(HashMap::new()),
                semantic_refresh: RefCell::new(Vec::new()),
                change_hooks: RefCell::new(Vec::new()),
                off,
                quitting: RefCell::new(None),
            }),
        }
    }

    /// Tells the server that the file `path` is open, with the text of `controller`. `notify` is called when
    /// what shows the server changes: it is not called from inside this.
    ///
    /// The server is run, and told of the file, a moment later, and not here: this is called inside an
    /// update of the workspace.
    pub fn open(&self, path: &Path, controller: &TextEditingController, notify: Rc<dyn Fn()>) {
        let Some(venus) = Venus::current() else {
            return;
        };
        if self.shared.off {
            return;
        }
        self.shared
            .notify
            .borrow_mut()
            .get_or_insert(notify);
        self.shared
            .core
            .borrow_mut()
            .track(path.to_path_buf(), controller.clone());
        self.shared
            .quitting
            .borrow_mut()
            .get_or_insert_with(|| {
                let weak = Rc::downgrade(&self.shared);
                on_lifecycle(move |event| {
                    if event == Lifecycle::Exit
                        && let Some(shared) = weak.upgrade()
                    {
                        // Not while it is being used: a quit that finds the core busy lets the process end.
                        if let Ok(mut core) = shared.core.try_borrow_mut() {
                            core.finish(QUIT_WAIT);
                        }
                    }
                })
            });

        // Only a change of the text is one: a move of the caret changes what the controller holds too.
        let last = RefCell::new(controller.value());
        let (weak, owned, watched) = (
            Rc::downgrade(&self.shared),
            path.to_path_buf(),
            controller.clone(),
        );
        let listener = controller.listen(move |_| {
            let now = watched.value();
            if now.same_text(&last.borrow()) {
                return;
            }
            *last.borrow_mut() = now;
            if let Some(shared) = weak.upgrade() {
                Shared::changed(&shared, &owned);
            }
        });
        self.shared
            .listeners
            .borrow_mut()
            .insert(path.to_path_buf(), listener);

        let shared = Rc::clone(&self.shared);
        venus.spawn(async move { Shared::sync(&shared) });
    }

    /// Moves the project to the folder `root`, which is what the server is run on. An app that is opened by a folder dropped on its
    /// icon begins on the folder it was started in, and is given the dropped one after: a server that was run on the first is let
    /// go, and one is run on the new folder, which is told of the open files again. Where the folder is the same, nothing is done.
    ///
    /// What was waiting for an answer of the old server is called with none: that answer is not coming. Like the other requests,
    /// that is not from inside this where a request is waiting, so what is called may ask again.
    pub fn set_root(&self, root: PathBuf) {
        if !self.shared.core.borrow_mut().set_root(root) {
            return;
        }
        // Taken before any is called: what is called may ask again, which is asked of the new server.
        let waiting = self.shared.waiting.borrow_mut().take();
        let defining = self.shared.defining.borrow_mut().take();
        let hovering = self.shared.hovering.borrow_mut().take();
        let resolving = self.shared.resolving.borrow_mut().take();
        let asking: Vec<_> = self
            .shared
            .asking
            .borrow_mut()
            .drain()
            .collect();
        if let Some((_, done)) = waiting {
            done(None);
        }
        if let Some((_, done)) = defining {
            done(None);
        }
        if let Some((_, done)) = hovering {
            done(None);
        }
        if let Some((_, done)) = resolving {
            done(None);
        }
        for (_, done) in asking {
            done(None);
        }
        if let Some(venus) = Venus::current() {
            let shared = Rc::clone(&self.shared);
            venus.spawn(async move { Shared::sync(&shared) });
        }
    }

    /// Tells the server that the open file `path` was saved: the text it was changed to is told first, and then the save, which makes
    /// a server such as rust-analyzer check the project. Nothing is told of a file that is not open.
    pub fn saved(&self, path: &Path) {
        if !self
            .shared
            .listeners
            .borrow()
            .contains_key(path)
        {
            return;
        }
        self.shared.core.borrow_mut().saved(path);
    }

    /// Tells the server that the file `path` is closed.
    pub fn close(&self, path: &Path) {
        if self
            .shared
            .listeners
            .borrow_mut()
            .remove(path)
            .is_none()
        {
            return;
        }
        self.shared.core.borrow_mut().untrack(path);
    }

    /// Asks the server what could be put at the caret of the file `path`, and calls `done` with the items when it
    /// has said, or with none when it could not be asked or could not say. `done` is not called from inside this
    /// unless it is with none, and not at all if another request is made before the answer: that one is wanted
    /// and this is not.
    pub fn complete(&self, path: &Path, done: impl FnOnce(Option<Answer>) + 'static) {
        let id = {
            let mut core = self.shared.core.borrow_mut();
            if let Some((old, _)) = self.shared.waiting.borrow_mut().take() {
                core.cancel(old);
            }
            core.complete(path)
        };
        match id {
            Some(id) => *self.shared.waiting.borrow_mut() = Some((id, Box::new(done))),
            None => done(None),
        }
    }

    /// Asks the server where the name at the byte `at` of the file `path` is defined, and calls `done` with the places when it
    /// has said, or with none when it could not be asked or could not say. Like [`complete`](Self::complete), `done` is not
    /// called from inside this unless it is with none, and not at all if another is asked for before the answer.
    pub fn definition(
        &self,
        path: &Path,
        at: usize,
        done: impl FnOnce(Option<Vec<Place>>) + 'static,
    ) {
        let id = {
            let mut core = self.shared.core.borrow_mut();
            if let Some((old, _)) = self.shared.defining.borrow_mut().take() {
                core.cancel(old);
            }
            core.definition(path, at)
        };
        match id {
            Some(id) => *self.shared.defining.borrow_mut() = Some((id, Box::new(done))),
            None => done(None),
        }
    }

    /// Asks the server what the name at the byte `at` of the file `path` is, and what is written of it, and calls `done` with the
    /// words when it has said, or with none when it could not be asked, had nothing to say, or could not say. Like
    /// [`complete`](Self::complete), `done` is not called from inside this unless it is with none, and not at all if another is asked
    /// for before the answer.
    pub fn hover(&self, path: &Path, at: usize, done: impl FnOnce(Option<Hover>) + 'static) {
        let id = {
            let mut core = self.shared.core.borrow_mut();
            if let Some((old, _)) = self.shared.hovering.borrow_mut().take() {
                core.cancel(old);
            }
            core.hover(path, at)
        };
        match id {
            Some(id) => *self.shared.hovering.borrow_mut() = Some((id, Box::new(done))),
            None => done(None),
        }
    }

    /// Says that the answer to the last request for a hover is not wanted: the pointer has gone from the name. Nothing is called.
    pub fn cancel_hover(&self) {
        let old = self.shared.hovering.borrow_mut().take();
        if let Some((id, _)) = old {
            self.shared.core.borrow_mut().cancel(id);
        }
    }

    /// The byte of `text` that the position `line`, `character` of the server is, counted as the server counts.
    pub fn byte_of(&self, text: &aimer::input::input::TextSnapshot, at: Position) -> usize {
        self.shared.core.borrow().byte_of(text, at)
    }

    /// Asks the server for the rest of an item of the file `path`, as `item` says to ask for it, and calls `done` with
    /// the edits that putting the item in asks for, or with none when it could not be asked or could not say. Like
    /// [`complete`](Self::complete), `done` is not called from inside this unless it is with none, and not at all if
    /// another is asked for before the answer.
    pub fn resolve(
        &self,
        path: &Path,
        item: &serde_json::Value,
        done: impl FnOnce(Option<Vec<(Range<usize>, String)>>) + 'static,
    ) {
        let id = {
            let mut core = self.shared.core.borrow_mut();
            if let Some((old, _)) = self.shared.resolving.borrow_mut().take() {
                core.cancel(old);
            }
            core.resolve(path, item)
        };
        match id {
            Some(id) => *self.shared.resolving.borrow_mut() = Some((id, Box::new(done))),
            None => done(None),
        }
    }

    /// Makes the request `method` with `params`, whatever it is, and calls `done` with what the server said, or with none
    /// where it said an error or could not be asked. Any number are waiting at once, unlike the other requests, which are one
    /// at a time. `done` is not called from inside this unless it is with none. The number that is returned is the request's,
    /// for [`cancel`](Self::cancel): none where it could not be asked.
    pub fn ask(
        &self,
        method: &str,
        params: serde_json::Value,
        done: impl FnOnce(Option<serde_json::Value>) + 'static,
    ) -> Option<i64> {
        let id = self
            .shared
            .core
            .borrow_mut()
            .ask(method, params);
        match id {
            Some(id) => {
                self.shared
                    .asking
                    .borrow_mut()
                    .insert(id, Box::new(done));
            }
            None => done(None),
        }
        id
    }

    /// Where the byte `byte` of `text` is, counted as the server counts.
    pub(crate) fn position_of(
        &self,
        text: &aimer::input::input::TextSnapshot,
        byte: usize,
    ) -> Position {
        self.shared
            .core
            .borrow()
            .position_of(text, byte)
    }

    /// What the server calls the kinds and the modifiers of its tokens, if it has the tokens of a file to give.
    pub(crate) fn legend(&self) -> Option<std::rc::Rc<super::protocol::Legend>> {
        self.shared.core.borrow().legend()
    }

    pub(super) fn core_diagnostics(
        &self,
        path: &Path,
    ) -> Option<(
        Vec<super::session::Diagnostic>,
        aimer::input::input::TextSnapshot,
    )> {
        self.shared
            .core
            .borrow()
            .current_diagnostics(path)
    }

    pub(super) fn add_change_hook(&self, hook: Rc<dyn Fn() -> bool>) {
        self.shared.change_hooks.borrow_mut().push(hook);
    }

    pub(super) fn add_semantic_refresh(&self, hook: Rc<dyn Fn() -> bool>) {
        self.shared
            .semantic_refresh
            .borrow_mut()
            .push(hook);
    }

    /// Whether the server is ready and has hints to give.
    pub(crate) fn can_hint(&self) -> bool {
        self.shared.core.borrow().can_hint()
    }

    /// Says that the answer to the request `id` made by [`ask`](Self::ask) is not wanted: `done` is not called, and the server
    /// is told, which lets it stop working it out.
    pub fn cancel(&self, id: i64) {
        if self
            .shared
            .asking
            .borrow_mut()
            .remove(&id)
            .is_some()
        {
            self.shared.core.borrow_mut().cancel(id);
        }
    }

    /// The text of the file `path` as the app has it, if it is open.
    pub fn snapshot(&self, path: &Path) -> Option<aimer::input::input::TextSnapshot> {
        self.shared.core.borrow().snapshot(path)
    }

    /// The problems the server has found in the file `path`, worst first.
    pub fn problems(&self, path: &Path) -> Vec<Problem> {
        self.shared.core.borrow().problems(path)
    }

    /// What there is to say of the server and of the file `path`.
    pub fn summary(&self, path: &Path) -> Summary {
        self.shared.core.borrow().summary(path)
    }
}

impl Shared {
    /// Runs the server if it is to be, tells it of the files it does not know, and listens to it.
    fn sync(this: &Rc<Self>) {
        let synced = this.core.borrow_mut().sync();
        if let (Some(mailbox), Some(venus)) = (synced.started, Venus::current()) {
            let shared = Rc::clone(this);
            venus.spawn(async move { Shared::listen(&shared, mailbox).await });
        }
        if synced.changed {
            this.shown_changed();
        }
    }

    /// The text of `path` changed: it is told to the server when there has been no other change for a
    /// moment.
    fn changed(this: &Rc<Self>, path: &Path) {
        let first = this.core.borrow_mut().mark_dirty(path);
        if !first || this.flush_waiting.replace(true) {
            return;
        }
        let Some(venus) = Venus::current() else {
            this.flush_waiting.set(false);
            return;
        };
        let shared = Rc::clone(this);
        let waited = venus.sleep(FLUSH_AFTER);
        venus.spawn(async move {
            // A runtime that has no timers answers at once, and the flush is not put off.
            let _ = waited.await;
            shared.flush_waiting.set(false);
            shared.core.borrow_mut().flush();
        });
    }

    /// Takes what the server says as it says it, until it is gone.
    async fn listen(this: &Rc<Self>, mailbox: Arc<Mailbox<Event>>) {
        loop {
            std::future::poll_fn(|cx| mailbox.poll_ready(cx)).await;
            // A server that was let go for another folder: what it said is of no use, and nothing would ever take it from
            // its mailbox, so this would be woken for it again and again.
            if !this.core.borrow().listens_to(&mailbox) {
                return;
            }
            let (changed, answers, resolved, places, hovers, replies, refreshes) = {
                let mut core = this.core.borrow_mut();
                (
                    core.receive(),
                    core.take_answers(),
                    core.take_resolved(),
                    core.take_places(),
                    core.take_hovers(),
                    core.take_replies(),
                    core.take_semantic_refreshes(),
                )
            };
            for _ in 0..refreshes {
                // Not while anything is borrowed: what is called may ask for the tokens, and add what is called.
                let hooks: Vec<_> = this.semantic_refresh.borrow().clone();
                let alive: Vec<bool> = hooks.iter().map(|hook| hook()).collect();
                let mut kept = alive.iter();
                this.semantic_refresh
                    .borrow_mut()
                    .retain(|_| kept.next().copied().unwrap_or(true));
            }
            for (id, reply) in replies {
                // Not while anything is borrowed: what is called may ask again.
                let done = this.asking.borrow_mut().remove(&id);
                if let Some(done) = done {
                    done(reply);
                }
            }
            for (id, said) in hovers {
                let done = {
                    let mut hovering = this.hovering.borrow_mut();
                    if hovering
                        .as_ref()
                        .is_some_and(|(wanted, _)| *wanted == id)
                    {
                        hovering.take()
                    } else {
                        None
                    }
                };
                if let Some((_, done)) = done {
                    done(said);
                }
            }
            for (id, found) in places {
                let done = {
                    let mut defining = this.defining.borrow_mut();
                    if defining
                        .as_ref()
                        .is_some_and(|(wanted, _)| *wanted == id)
                    {
                        defining.take()
                    } else {
                        None
                    }
                };
                if let Some((_, done)) = done {
                    done(found);
                }
            }
            for (id, edits) in resolved {
                let done = {
                    let mut resolving = this.resolving.borrow_mut();
                    if resolving
                        .as_ref()
                        .is_some_and(|(wanted, _)| *wanted == id)
                    {
                        resolving.take()
                    } else {
                        None
                    }
                };
                if let Some((_, done)) = done {
                    done(edits);
                }
            }
            for (id, answer) in answers {
                // Not while anything is borrowed: what is called may ask again.
                let done = {
                    let mut waiting = this.waiting.borrow_mut();
                    if waiting
                        .as_ref()
                        .is_some_and(|(wanted, _)| *wanted == id)
                    {
                        waiting.take()
                    } else {
                        None
                    }
                };
                if let Some((_, done)) = done {
                    done(answer);
                }
            }
            if changed {
                this.shown_changed();
                // Not while anything is borrowed: what is called may ask for what changed.
                let hooks: Vec<_> = this.change_hooks.borrow().clone();
                if !hooks.is_empty() {
                    let alive: Vec<bool> = hooks.iter().map(|hook| hook()).collect();
                    let mut kept = alive.iter();
                    this.change_hooks
                        .borrow_mut()
                        .retain(|_| kept.next().copied().unwrap_or(true));
                }
            }
            let over = this
                .core
                .borrow()
                .summary(Path::new(""))
                .status
                .is_some_and(|status| matches!(status, Status::Failed(_) | Status::Stopped));
            if over {
                return;
            }
        }
    }

    fn shown_changed(&self) {
        let notify = self.notify.borrow().clone();
        if let Some(notify) = notify {
            notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::protocol;
    use crate::lsp::transport::{self, Incoming};
    use serde_json::{Value, json};

    const FILE: &str = "/project/src/main.rs";

    use crate::lsp::fake::{Fake, installed, lsp_with, settle};

    #[test]
    fn it_does_nothing_where_there_is_no_runtime() {
        let _ = Venus::uninstall();
        let fake = Fake::new();
        let lsp = lsp_with(&fake);
        lsp.open(
            Path::new(FILE),
            &TextEditingController::with_text("x"),
            Rc::new(|| {}),
        );
        assert_eq!(lsp.summary(Path::new(FILE)).status, None);
        assert!(fake.methods().is_empty());
    }

    #[test]
    fn opening_a_file_runs_the_server_a_moment_later_and_not_inside_the_open() {
        let venus = installed();
        let fake = Fake::new();
        let lsp = lsp_with(&fake);
        let changes = Rc::new(Cell::new(0));
        let counted = Rc::clone(&changes);
        lsp.open(
            Path::new(FILE),
            &TextEditingController::with_text("fn main() {}\n"),
            Rc::new(move || counted.set(counted.get() + 1)),
        );
        assert_eq!(
            lsp.summary(Path::new(FILE)).status,
            None,
            "nothing is run inside the open"
        );
        assert_eq!(
            changes.get(),
            0,
            "and nothing is told to the interface inside it"
        );

        settle(&venus);
        assert_eq!(lsp.summary(Path::new(FILE)).status, Some(Status::Starting));
        assert_eq!(
            fake.methods()
                .iter()
                .map(|(method, _)| method.as_str())
                .collect::<Vec<_>>(),
            ["initialize"]
        );
        assert_eq!(
            changes.get(),
            1,
            "the interface is told that the server is starting"
        );
        let _ = Venus::uninstall();
    }

    #[test]
    fn what_the_server_says_is_taken_when_it_says_it_and_the_interface_is_told() {
        let venus = installed();
        let fake = Fake::new();
        let lsp = lsp_with(&fake);
        let changes = Rc::new(Cell::new(0));
        let counted = Rc::clone(&changes);
        lsp.open(
            Path::new(FILE),
            &TextEditingController::with_text("let x = 1;\n"),
            Rc::new(move || counted.set(counted.get() + 1)),
        );
        settle(&venus);
        let _ = fake.methods();
        let id = 1;
        fake.mailbox
            .push(Event::Message(Incoming::Response {
                id,
                result: Ok(
                    json!({"capabilities": {"textDocumentSync": 2, "positionEncoding": "utf-8"}}),
                ),
            }));
        settle(&venus);
        assert_eq!(lsp.summary(Path::new(FILE)).status, Some(Status::Ready));
        assert_eq!(
            fake.methods()
                .iter()
                .map(|(method, _)| method.as_str())
                .collect::<Vec<_>>(),
            ["initialized", "textDocument/didOpen"]
        );

        fake.mailbox.push(Event::Message(Incoming::Notification {
            method: protocol::PUBLISH_DIAGNOSTICS.into(),
            params: json!({"uri": "file:///project/src/main.rs", "version": 1, "diagnostics": [{"range": {"start": {"line": 0, "character": 4}, "end": {"line": 0, "character": 5}}, "severity": 1, "message": "bad"}]}),
        }));
        settle(&venus);
        assert_eq!(lsp.summary(Path::new(FILE)).errors, 1);
        assert!(
            changes.get() >= 3,
            "told when it started, was ready and had diagnostics: {}",
            changes.get()
        );
        let _ = Venus::uninstall();
    }

    /// A server that has said it is ready and completes, with `text` open in the file.
    fn completing(venus: &Rc<Venus>, fake: &Fake, text: &str) -> (Lsp, TextEditingController) {
        let lsp = lsp_with(fake);
        let controller = TextEditingController::with_text(text);
        lsp.open(Path::new(FILE), &controller, Rc::new(|| {}));
        settle(venus);
        let _ = fake.methods();
        fake.mailbox.push(Event::Message(Incoming::Response {
            id: 1,
            result: Ok(json!({"capabilities": {"textDocumentSync": 2, "positionEncoding": "utf-8", "completionProvider": {}}})),
        }));
        settle(venus);
        let _ = fake.methods();
        (lsp, controller)
    }

    /// A server for each folder it is asked to run on, from `fakes` in turn, and the folders it was asked for.
    fn lsp_on_each_root(fakes: Vec<Fake>, first: &str) -> (Lsp, Rc<RefCell<Vec<PathBuf>>>) {
        let roots: Rc<RefCell<Vec<PathBuf>>> = Rc::default();
        let (noted, fakes) = (
            Rc::clone(&roots),
            RefCell::new(std::collections::VecDeque::from(fakes)),
        );
        let starter: Starter = Box::new(move |root| {
            noted.borrow_mut().push(root.to_path_buf());
            let fake = fakes
                .borrow_mut()
                .pop_front()
                .expect("a fake for each start");
            Ok(Box::new(fake) as Box<dyn Link>)
        });
        (
            Lsp::with_starter(PathBuf::from(first), starter, false),
            roots,
        )
    }

    #[test]
    fn a_new_root_runs_the_server_on_it_and_what_waited_on_the_old_one_is_given_nothing() {
        let venus = installed();
        let (old, new) = (Fake::new(), Fake::new());
        let (lsp, roots) = lsp_on_each_root(vec![old.clone(), new.clone()], "/home");
        let controller = TextEditingController::with_text("let x = Ve");
        lsp.open(Path::new(FILE), &controller, Rc::new(|| {}));
        settle(&venus);
        old.mailbox.push(Event::Message(Incoming::Response {
            id: 1,
            result: Ok(json!({"capabilities": {"textDocumentSync": 2, "positionEncoding": "utf-8", "completionProvider": {}}})),
        }));
        settle(&venus);
        let _ = old.methods();
        controller.select_bytes(10, 10);
        let got: Rc<RefCell<Vec<Option<usize>>>> = Rc::default();
        let into = Rc::clone(&got);
        lsp.complete(Path::new(FILE), move |answer| {
            into.borrow_mut()
                .push(answer.map(|a| a.items.len()))
        });
        assert_eq!(*roots.borrow(), [PathBuf::from("/home")]);

        lsp.set_root(PathBuf::from("/project"));
        settle(&venus);

        assert_eq!(
            *roots.borrow(),
            [PathBuf::from("/home"), PathBuf::from("/project")],
            "run again on the folder that was chosen"
        );
        assert_eq!(
            *got.borrow(),
            vec![None],
            "the old server's answer is not coming"
        );
        let told = new.methods();
        assert_eq!(
            told.iter()
                .map(|(method, _)| method.as_str())
                .collect::<Vec<_>>(),
            ["initialize"]
        );
        assert_eq!(told[0].1["rootUri"], json!("file:///project"));

        // A word from the old server is not heard: its mailbox is not the one in use.
        old.mailbox
            .push(Event::Message(Incoming::Notification {
                method: "$/progress".into(),
                params: json!({}),
            }));
        settle(&venus);
        let _ = Venus::uninstall();
    }

    #[test]
    fn the_same_root_is_not_a_reason_to_run_the_server_again() {
        let venus = installed();
        let (only, extra) = (Fake::new(), Fake::new());
        let (lsp, roots) = lsp_on_each_root(vec![only, extra], "/project");
        lsp.open(
            Path::new(FILE),
            &TextEditingController::with_text("x"),
            Rc::new(|| {}),
        );
        settle(&venus);
        lsp.set_root(PathBuf::from("/project"));
        settle(&venus);
        assert_eq!(roots.borrow().len(), 1);
        let _ = Venus::uninstall();
    }

    fn items(labels: &[&str]) -> Value {
        json!({"isIncomplete": false, "items": labels.iter().map(|label| json!({"label": label})).collect::<Vec<_>>()})
    }

    #[test]
    fn the_answer_to_a_request_for_completion_is_given_to_what_asked_once() {
        let venus = installed();
        let fake = Fake::new();
        let (lsp, controller) = completing(&venus, &fake, "let x = Ve");
        controller.select_bytes(10, 10);
        let got: Rc<RefCell<Vec<Option<Vec<String>>>>> = Rc::default();
        let into = Rc::clone(&got);
        lsp.complete(Path::new(FILE), move |answer| {
            into.borrow_mut().push(answer.map(|a| {
                a.items
                    .into_iter()
                    .map(|item| item.label)
                    .collect()
            }))
        });
        let told = fake.methods();
        assert_eq!(
            told.iter()
                .map(|(method, _)| method.as_str())
                .collect::<Vec<_>>(),
            ["textDocument/completion"]
        );
        assert_eq!(
            told[0].1["position"],
            json!({"line": 0, "character": 10}),
            "at the caret of the file"
        );
        assert!(
            got.borrow().is_empty(),
            "the answer is not there yet, and what asked is not called inside the ask"
        );
        fake.mailbox
            .push(Event::Message(Incoming::Response {
                id: 2,
                result: Ok(items(&["Vec", "VecDeque"])),
            }));
        settle(&venus);
        assert_eq!(
            *got.borrow(),
            vec![Some(vec!["Vec".to_owned(), "VecDeque".to_owned()])]
        );
        let _ = Venus::uninstall();
    }

    #[test]
    fn a_request_makes_the_one_before_it_unwanted_which_is_cancelled_and_not_answered() {
        let venus = installed();
        let fake = Fake::new();
        let (lsp, _) = completing(&venus, &fake, "ab");
        let got: Rc<RefCell<Vec<String>>> = Rc::default();
        let (first, second) = (Rc::clone(&got), Rc::clone(&got));
        lsp.complete(Path::new(FILE), move |_| {
            first.borrow_mut().push("first".into())
        });
        lsp.complete(Path::new(FILE), move |answer| {
            second
                .borrow_mut()
                .push(format!("second {}", answer.map_or(0, |a| a.items.len())))
        });
        let told = fake.methods();
        assert_eq!(
            told.iter()
                .map(|(method, _)| method.as_str())
                .collect::<Vec<_>>(),
            [
                "textDocument/completion",
                "$/cancelRequest",
                "textDocument/completion"
            ]
        );
        assert_eq!(told[1].1, json!({"id": 2}));
        // The server answers both, the one that was cancelled with what it had.
        fake.mailbox
            .push(Event::Message(Incoming::Response {
                id: 2,
                result: Ok(items(&["late"])),
            }));
        fake.mailbox
            .push(Event::Message(Incoming::Response {
                id: 3,
                result: Ok(items(&["a", "b"])),
            }));
        settle(&venus);
        assert_eq!(*got.borrow(), vec!["second 2".to_owned()]);
        let _ = Venus::uninstall();
    }

    #[test]
    fn a_saved_file_is_told_after_the_edits_that_were_made_before_it_and_one_that_is_not_open_is_not_told()
     {
        let venus = installed();
        let fake = Fake::new();
        let lsp = lsp_with(&fake);
        let controller = TextEditingController::with_text("a\n");
        lsp.open(Path::new(FILE), &controller, Rc::new(|| {}));
        settle(&venus);
        fake.mailbox
            .push(Event::Message(Incoming::Response {
                id: 1,
                result: Ok(
                    json!({"capabilities": {"textDocumentSync": 2, "positionEncoding": "utf-8"}}),
                ),
            }));
        settle(&venus);
        let _ = fake.methods();
        // The text was changed and not yet told, and the file is saved: the server is told of the text, and then of the save, so that
        // what it checks is what is on disk.
        controller.set_text("b\n");
        lsp.saved(Path::new(FILE));
        let told: Vec<String> = fake
            .methods()
            .into_iter()
            .map(|(method, _)| method)
            .collect();
        assert_eq!(told, ["textDocument/didChange", "textDocument/didSave"]);
        lsp.saved(Path::new("/project/not_open.rs"));
        assert!(fake.methods().is_empty());
        let _ = Venus::uninstall();
    }

    #[test]
    fn a_hover_is_asked_for_and_given_once_a_newer_request_makes_the_older_unwanted_and_a_cancelled_one_is_not_given()
     {
        let venus = installed();
        let fake = Fake::new();
        let lsp = lsp_with(&fake);
        let controller = TextEditingController::with_text("fn a() {}\nfn b() { a(); }\n");
        lsp.open(Path::new(FILE), &controller, Rc::new(|| {}));
        settle(&venus);
        let _ = fake.methods();
        fake.mailbox.push(Event::Message(Incoming::Response {
            id: 1,
            result: Ok(json!({"capabilities": {"textDocumentSync": 2, "positionEncoding": "utf-8", "hoverProvider": true}})),
        }));
        settle(&venus);
        let _ = fake.methods();
        let got: Rc<RefCell<Vec<String>>> = Rc::default();
        let (first, second) = (Rc::clone(&got), Rc::clone(&got));
        let at = controller.value().text().find("a();").unwrap() + 1;
        lsp.hover(Path::new(FILE), at, move |_| {
            first.borrow_mut().push("first".into())
        });
        lsp.hover(Path::new(FILE), at, move |said: Option<Hover>| {
            second.borrow_mut().push(format!(
                "second {:?}",
                said.map(|said| (said.markdown, said.range))
            ))
        });
        let told = fake.methods();
        assert_eq!(
            told.iter()
                .map(|(method, _)| method.as_str())
                .collect::<Vec<_>>(),
            [
                "textDocument/hover",
                "$/cancelRequest",
                "textDocument/hover"
            ]
        );
        assert_eq!(told[2].1["position"], json!({"line": 1, "character": 10}));
        let words = |text: &str| json!({"contents": {"kind": "markdown", "value": text}, "range": {"start": {"line": 1, "character": 9}, "end": {"line": 1, "character": 10}}});
        fake.mailbox
            .push(Event::Message(Incoming::Response {
                id: 2,
                result: Ok(words("old")),
            }));
        fake.mailbox
            .push(Event::Message(Incoming::Response {
                id: 3,
                result: Ok(words("`a`")),
            }));
        settle(&venus);
        assert_eq!(
            *got.borrow(),
            vec![format!(
                "second {:?}",
                Some(("`a`".to_owned(), Some(19..20)))
            )]
        );
        // Asked and then not wanted: nothing is called, and the server is told.
        let third = Rc::clone(&got);
        lsp.hover(Path::new(FILE), at, move |_| {
            third.borrow_mut().push("third".into())
        });
        lsp.cancel_hover();
        assert_eq!(
            fake.methods()
                .iter()
                .map(|(method, _)| method.as_str())
                .collect::<Vec<_>>(),
            ["textDocument/hover", "$/cancelRequest"]
        );
        fake.mailbox
            .push(Event::Message(Incoming::Response {
                id: 4,
                result: Ok(words("late")),
            }));
        settle(&venus);
        assert_eq!(got.borrow().len(), 1, "the cancelled one is not given");
        // Asked where it cannot be: at once, with none.
        let lone = lsp_with(&Fake::new());
        let said: Rc<RefCell<Vec<bool>>> = Rc::default();
        let into = Rc::clone(&said);
        lone.hover(Path::new(FILE), 0, move |hover| {
            into.borrow_mut().push(hover.is_none())
        });
        assert_eq!(*said.borrow(), vec![true]);
        let _ = Venus::uninstall();
    }

    #[test]
    fn a_definition_is_asked_for_and_given_once_and_a_newer_request_makes_the_older_unwanted() {
        let venus = installed();
        let fake = Fake::new();
        let lsp = lsp_with(&fake);
        let controller = TextEditingController::with_text("fn a() {}\nfn b() { a(); }\n");
        lsp.open(Path::new(FILE), &controller, Rc::new(|| {}));
        settle(&venus);
        let _ = fake.methods();
        fake.mailbox.push(Event::Message(Incoming::Response {
            id: 1,
            result: Ok(json!({"capabilities": {"textDocumentSync": 2, "positionEncoding": "utf-8", "definitionProvider": true}})),
        }));
        settle(&venus);
        let _ = fake.methods();
        let got: Rc<RefCell<Vec<String>>> = Rc::default();
        let (first, second) = (Rc::clone(&got), Rc::clone(&got));
        let at = controller.value().text().find("a();").unwrap() + 1;
        lsp.definition(Path::new(FILE), at, move |_| {
            first.borrow_mut().push("first".into())
        });
        lsp.definition(Path::new(FILE), at, move |places| {
            second.borrow_mut().push(format!(
                "second {:?}",
                places.map(|places| places
                    .iter()
                    .map(|p| (p.at.line, p.at.character))
                    .collect::<Vec<_>>())
            ))
        });
        let told = fake.methods();
        assert_eq!(
            told.iter()
                .map(|(method, _)| method.as_str())
                .collect::<Vec<_>>(),
            [
                "textDocument/definition",
                "$/cancelRequest",
                "textDocument/definition"
            ]
        );
        assert_eq!(
            told[2].1["position"],
            json!({"line": 1, "character": 10}),
            "one into the name of the call, on its line"
        );
        let place = |line: u32| json!({"uri": "file:///project/src/main.rs", "range": {"start": {"line": line, "character": 3}, "end": {"line": line, "character": 4}}});
        fake.mailbox
            .push(Event::Message(Incoming::Response {
                id: 2,
                result: Ok(place(9)),
            }));
        fake.mailbox
            .push(Event::Message(Incoming::Response {
                id: 3,
                result: Ok(place(0)),
            }));
        settle(&venus);
        assert_eq!(*got.borrow(), vec!["second Some([(0, 3)])".to_owned()]);
        // Asked where it cannot be: at once, with none.
        let lone = lsp_with(&Fake::new());
        let said: Rc<RefCell<Vec<bool>>> = Rc::default();
        let into = Rc::clone(&said);
        lone.definition(Path::new(FILE), 0, move |places| {
            into.borrow_mut().push(places.is_none())
        });
        assert_eq!(*said.borrow(), vec![true]);
        // And a position of the server is a byte of a text.
        let text = aimer::input::input::TextSnapshot::from_text("\u{e9}x\n");
        assert_eq!(
            lsp.byte_of(
                &text,
                Position {
                    line: 0,
                    character: 2
                }
            ),
            2,
            "the accent is two bytes, and the server counts bytes"
        );
        let _ = Venus::uninstall();
    }

    #[test]
    fn the_rest_of_an_item_is_asked_for_and_given_once_and_a_newer_request_makes_the_older_unwanted()
     {
        let venus = installed();
        let fake = Fake::new();
        let (lsp, _) = completing(&venus, &fake, "mod a;\nCirc");
        let got: Rc<RefCell<Vec<String>>> = Rc::default();
        let (first, second) = (Rc::clone(&got), Rc::clone(&got));
        let item = |label: &str| json!({"label": label, "data": {"imports": [1]}});
        lsp.resolve(Path::new(FILE), &item("Circle"), move |_| {
            first.borrow_mut().push("first".into())
        });
        lsp.resolve(Path::new(FILE), &item("Square"), move |edits| {
            second
                .borrow_mut()
                .push(format!("second {:?}", edits))
        });
        let told = fake.methods();
        assert_eq!(
            told.iter()
                .map(|(method, _)| method.as_str())
                .collect::<Vec<_>>(),
            [
                "completionItem/resolve",
                "$/cancelRequest",
                "completionItem/resolve"
            ]
        );
        assert_eq!(told[2].1["label"], "Square");
        let edit = |text: &str| json!({"label": "x", "additionalTextEdits": [{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}, "newText": text}]});
        fake.mailbox
            .push(Event::Message(Incoming::Response {
                id: 2,
                result: Ok(edit("late")),
            }));
        fake.mailbox
            .push(Event::Message(Incoming::Response {
                id: 3,
                result: Ok(edit("use a::Square;\n")),
            }));
        settle(&venus);
        assert_eq!(
            *got.borrow(),
            vec!["second Some([(0..0, \"use a::Square;\\n\")])".to_owned()]
        );
        // Asked where it cannot be: at once, with none.
        let lone = lsp_with(&Fake::new());
        let said: Rc<RefCell<Vec<bool>>> = Rc::default();
        let into = Rc::clone(&said);
        lone.resolve(Path::new(FILE), &item("x"), move |edits| {
            into.borrow_mut().push(edits.is_none())
        });
        assert_eq!(*said.borrow(), vec![true]);
        let _ = Venus::uninstall();
    }

    #[test]
    fn a_request_that_cannot_be_made_is_answered_with_none_at_once() {
        let venus = installed();
        let fake = Fake::new();
        let lsp = lsp_with(&fake);
        let got: Rc<RefCell<Vec<bool>>> = Rc::default();
        let into = Rc::clone(&got);
        // Not even run yet: a file that is not open.
        lsp.complete(Path::new(FILE), move |answer| {
            into.borrow_mut().push(answer.is_none())
        });
        assert_eq!(*got.borrow(), vec![true]);
        // The server is starting: it has not said it can.
        lsp.open(
            Path::new(FILE),
            &TextEditingController::with_text("x"),
            Rc::new(|| {}),
        );
        settle(&venus);
        let into = Rc::clone(&got);
        lsp.complete(Path::new(FILE), move |answer| {
            into.borrow_mut().push(answer.is_none())
        });
        assert_eq!(*got.borrow(), vec![true, true]);
        let _ = Venus::uninstall();
    }

    #[test]
    fn the_server_is_told_of_an_edit_before_it_is_asked_what_follows_it() {
        let venus = installed();
        let fake = Fake::new();
        let (lsp, controller) = completing(&venus, &fake, "let x = V");
        controller.replace_range(9..9, "ec");
        // The edit is a moment from being told: the request does not wait for the moment.
        lsp.complete(Path::new(FILE), |_| {});
        let told = fake.methods();
        assert_eq!(
            told.iter()
                .map(|(method, _)| method.as_str())
                .collect::<Vec<_>>(),
            ["textDocument/didChange", "textDocument/completion"]
        );
        assert_eq!(told[1].1["position"], json!({"line": 0, "character": 11}));
        settle(&venus);
        assert!(
            fake.methods().is_empty(),
            "and the edit is not told again at the moment"
        );
        let _ = Venus::uninstall();
    }

    #[test]
    fn edits_are_told_to_the_server_once_after_the_last_of_them() {
        let venus = installed();
        let fake = Fake::new();
        let lsp = lsp_with(&fake);
        let controller = TextEditingController::with_text("one\ntwo\n");
        lsp.open(Path::new(FILE), &controller, Rc::new(|| {}));
        settle(&venus);
        fake.mailbox
            .push(Event::Message(Incoming::Response {
                id: 1,
                result: Ok(
                    json!({"capabilities": {"textDocumentSync": 2, "positionEncoding": "utf-8"}}),
                ),
            }));
        settle(&venus);
        let _ = fake.methods();

        // Three edits and some moves of the caret, one after another.
        controller.set_text("ONE\ntwo\n");
        controller.select(1, 1);
        controller.set_text("ONE\nTWO\n");
        controller.select(2, 2);
        controller.set_text("ONE\nTWO\nTHREE\n");
        settle(&venus);
        let told = fake.methods();
        assert_eq!(
            told.iter()
                .map(|(method, _)| method.as_str())
                .collect::<Vec<_>>(),
            ["textDocument/didChange"],
            "one change for the three edits"
        );
        assert_eq!(told[0].1["textDocument"]["version"], 2);

        // A move of the caret is not a change of the text.
        controller.select(0, 0);
        controller.select(3, 3);
        settle(&venus);
        assert!(fake.methods().is_empty());
        let _ = Venus::uninstall();
    }

    #[test]
    fn a_closed_file_is_closed_to_the_server_and_its_changes_are_no_longer_followed() {
        let venus = installed();
        let fake = Fake::new();
        let lsp = lsp_with(&fake);
        let controller = TextEditingController::with_text("a\n");
        lsp.open(Path::new(FILE), &controller, Rc::new(|| {}));
        settle(&venus);
        fake.mailbox
            .push(Event::Message(Incoming::Response {
                id: 1,
                result: Ok(
                    json!({"capabilities": {"textDocumentSync": 2, "positionEncoding": "utf-8"}}),
                ),
            }));
        settle(&venus);
        let _ = fake.methods();
        lsp.close(Path::new(FILE));
        assert_eq!(
            fake.methods()
                .iter()
                .map(|(method, _)| method.as_str())
                .collect::<Vec<_>>(),
            ["textDocument/didClose"]
        );
        controller.set_text("b\n");
        settle(&venus);
        assert!(fake.methods().is_empty());
        lsp.close(Path::new(FILE));
        let _ = Venus::uninstall();
    }

    #[test]
    fn a_server_that_cannot_be_run_is_told_to_the_interface_and_nothing_panics() {
        let venus = installed();
        let starter: Starter = Box::new(|_| {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no such program",
            ))
        });
        let lsp = Lsp::with_starter(PathBuf::from("/project"), starter, false);
        let changes = Rc::new(Cell::new(0));
        let counted = Rc::clone(&changes);
        lsp.open(
            Path::new(FILE),
            &TextEditingController::with_text("x"),
            Rc::new(move || counted.set(counted.get() + 1)),
        );
        settle(&venus);
        assert!(matches!(
            lsp.summary(Path::new(FILE)).status,
            Some(Status::Failed(_))
        ));
        assert_eq!(changes.get(), 1);
        let _ = Venus::uninstall();
    }

    #[test]
    fn a_server_that_is_off_is_not_run() {
        let venus = installed();
        let fake = Fake::new();
        let lsp = {
            let fake = fake.clone();
            let starter: Starter = Box::new(move |_| Ok(Box::new(fake.clone()) as Box<dyn Link>));
            Lsp::with_starter(PathBuf::from("/project"), starter, true)
        };
        lsp.open(
            Path::new(FILE),
            &TextEditingController::with_text("x"),
            Rc::new(|| {}),
        );
        settle(&venus);
        assert_eq!(lsp.summary(Path::new(FILE)).status, None);
        assert!(fake.methods().is_empty());
        let _ = Venus::uninstall();
    }
}
