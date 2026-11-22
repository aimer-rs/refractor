//! What keeps a session and a server together, without anything of the interface: the documents that are
//! open, the server that is run when the first of them is, and the messages that go between. The
//! interface calls it on the thread it is made for and says when to flush and when to listen, so that
//! the whole of what it does can be tried with a server that is made of a script.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use aimer::input::TextEditingController;
use serde_json::Value;

use super::items::{Answer, Hover};
use super::position::{self, Position};
use super::problems::{Problem, problems_of};
use super::process::{Event, Mailbox, Server};
use super::protocol::Place;
use super::session::{Session, Status};

/// How a server is spoken to and heard.
pub trait Link {
    /// Leaves the body of a message for the server.
    fn send(&self, body: Vec<u8>);
    /// Where what the server did is left.
    fn mailbox(&self) -> Arc<Mailbox<Event>>;
    /// The server's process number.
    fn id(&self) -> u32;
}

impl Link for Server {
    fn send(&self, body: Vec<u8>) {
        Server::send(self, body);
    }

    fn mailbox(&self) -> Arc<Mailbox<Event>> {
        Server::mailbox(self)
    }

    fn id(&self) -> u32 {
        Server::id(self)
    }
}

/// How a server is run for the folder `root`.
pub type Starter = Box<dyn Fn(&Path) -> io::Result<Box<dyn Link>>>;

/// What there is to say of the server and of one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    /// How the server stands; `None` while none was asked for.
    pub status: Option<Status>,
    /// What the server is busy with, such as loading the project, while it is.
    pub progress: Option<String>,
    pub errors: usize,
    pub warnings: usize,
}

impl Summary {
    /// What the status bar says of it, if it says anything.
    pub fn label(&self) -> Option<String> {
        let plural = |count: usize, noun: &str| {
            format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
        };
        Some(match self.status.as_ref()? {
            Status::Starting => "Language server starting\u{2026}".to_owned(),
            Status::Ready => {
                let mut problems = Vec::new();
                if self.errors > 0 {
                    problems.push(plural(self.errors, "error"));
                }
                if self.warnings > 0 {
                    problems.push(plural(self.warnings, "warning"));
                }
                let busy = self
                    .progress
                    .as_ref()
                    .map(|job| format!("{job}\u{2026}"));
                let problems = (!problems.is_empty()).then(|| problems.join(", "));
                match (busy, problems) {
                    (None, None) => "Language server: no problems".to_owned(),
                    (busy, problems) => format!(
                        "Language server: {}",
                        [busy, problems]
                            .into_iter()
                            .flatten()
                            .collect::<Vec<_>>()
                            .join(" \u{b7} ")
                    ),
                }
            }
            Status::Failed(why) => format!("Language server failed: {why}"),
            Status::Stopped => "Language server stopped".to_owned(),
        })
    }
}

/// What `sync` did.
pub struct Synced {
    /// The mailbox of a server that was run now, which is to be listened to.
    pub started: Option<Arc<Mailbox<Event>>>,
    /// Whether what shows the server changed.
    pub changed: bool,
}

/// The documents, the server and the session.
pub struct Core {
    root: PathBuf,
    starter: Starter,
    link: Option<Box<dyn Link>>,
    session: Option<Session>,
    /// Why the server could not be run, if it could not: it is not run again.
    start_error: Option<String>,
    documents: HashMap<PathBuf, TextEditingController>,
    /// The documents the session was told are open.
    opened: HashSet<PathBuf>,
    /// The documents that changed since they were last flushed.
    dirty: BTreeSet<PathBuf>,
    /// The revision of the session that was last given out as changed.
    seen_revision: u64,
}

impl Core {
    pub fn new(root: PathBuf, starter: Starter) -> Self {
        Self {
            root,
            starter,
            link: None,
            session: None,
            start_error: None,
            documents: HashMap::new(),
            opened: HashSet::new(),
            dirty: BTreeSet::new(),
            seen_revision: 0,
        }
    }

    /// Moves the project to the folder `root`. A server that is running was run on the old folder and cannot be moved, so it
    /// is let go, and the next [`Self::sync`] runs one on the new folder and tells it of the open files again. A server that
    /// could not be run is tried again: it may be that the old folder was why. False, and nothing done, where it is the
    /// folder already.
    pub fn set_root(&mut self, root: PathBuf) -> bool {
        if self.root == root {
            return false;
        }
        // Said to shut down but not waited for: the folder is changing under the user's hands. What is let go ends the process.
        self.finish(Duration::ZERO);
        self.session = None;
        self.start_error = None;
        self.opened.clear();
        self.dirty.clear();
        self.seen_revision = 0;
        self.root = root;
        true
    }

    /// Whether `mailbox` is the one of the server that is running. What listens to the mailbox of a server that was let go
    /// is to stop: nothing takes what that server says, so it would never be done with it.
    pub fn listens_to(&self, mailbox: &Arc<Mailbox<Event>>) -> bool {
        self.link
            .as_ref()
            .is_some_and(|link| Arc::ptr_eq(&link.mailbox(), mailbox))
    }

    /// Remembers that the file `path` is open with the text of `controller`. Nothing is run or sent
    /// until [`Self::sync`].
    pub fn track(&mut self, path: PathBuf, controller: TextEditingController) {
        self.documents.insert(path, controller);
    }

    /// Runs the server if there are documents and it was not, and tells the session of the documents it
    /// does not know.
    pub fn sync(&mut self) -> Synced {
        let (mut started, mut changed) = (None, false);
        if self.documents.is_empty() {
            return Synced { started, changed };
        }
        if self.link.is_none() && self.start_error.is_none() {
            match (self.starter)(&self.root) {
                Ok(link) => {
                    self.session = Some(Session::new(&self.root, link.id()));
                    started = Some(link.mailbox());
                    self.link = Some(link);
                }
                Err(error) => {
                    self.start_error =
                        Some(format!("the language server could not be run: {error}"))
                }
            }
            changed = true;
        }
        if let Some(session) = &mut self.session {
            let mut unknown: Vec<PathBuf> = self
                .documents
                .keys()
                .filter(|path| !self.opened.contains(*path))
                .cloned()
                .collect();
            unknown.sort();
            for path in unknown {
                session.open(&path, &self.documents[&path].snapshot());
                self.opened.insert(path);
            }
        }
        self.pump();
        Synced { started, changed }
    }

    /// The file `path` was saved. What was changed in it and not yet told is told first, so that the server checks what is on disk.
    pub fn saved(&mut self, path: &Path) {
        self.flush();
        if let Some(session) = &mut self.session {
            session.saved(path);
        }
        self.pump();
    }

    /// The file `path` is closed.
    pub fn untrack(&mut self, path: &Path) {
        self.documents.remove(path);
        self.dirty.remove(path);
        self.opened.remove(path);
        if let Some(session) = &mut self.session {
            session.close(path);
        }
        self.pump();
    }

    /// The text of `path` changed. True if that is the first change since the last flush, which is when
    /// a flush is to be arranged.
    pub fn mark_dirty(&mut self, path: &Path) -> bool {
        if !self.documents.contains_key(path) {
            return false;
        }
        let first = self.dirty.is_empty();
        self.dirty.insert(path.to_path_buf());
        first
    }

    /// Tells the server of the changes since the last flush, one change for each file.
    pub fn flush(&mut self) {
        let dirty = std::mem::take(&mut self.dirty);
        if let Some(session) = &mut self.session {
            for path in dirty
                .iter()
                .filter(|path| self.opened.contains(*path))
            {
                if let Some(controller) = self.documents.get(path) {
                    session.changed(path, &controller.snapshot());
                }
            }
        }
        self.pump();
    }

    /// Takes what the server did, answers what it asked, and says whether what shows it changed.
    pub fn receive(&mut self) -> bool {
        let (Some(link), Some(session)) = (&self.link, &mut self.session) else {
            return false;
        };
        for event in link.mailbox().drain() {
            match event {
                Event::Message(message) => session.receive(message),
                Event::Gone(why) => session.server_gone(&why),
                // What is not a message is not answered: there is no id to answer to.
                Event::Malformed(_) => {}
            }
        }
        self.pump();
        let revision = self
            .session
            .as_ref()
            .map_or(0, Session::revision);
        std::mem::replace(&mut self.seen_revision, revision) != revision
    }

    /// What there is to say of the server and of the file `path`.
    pub fn summary(&self, path: &Path) -> Summary {
        if let Some(why) = &self.start_error {
            return Summary {
                status: Some(Status::Failed(why.clone())),
                progress: None,
                errors: 0,
                warnings: 0,
            };
        }
        match &self.session {
            Some(session) => {
                let (errors, warnings) = session.counts_of(path);
                Summary {
                    status: Some(session.status().clone()),
                    progress: session.progress_label(),
                    errors,
                    warnings,
                }
            }
            None => Summary {
                status: None,
                progress: None,
                errors: 0,
                warnings: 0,
            },
        }
    }

    /// The problems the server has found in the file `path`, worst first, for the text as it is now. None for a
    /// file that is not open, and none that were found in a text that has been changed since.
    pub fn problems(&self, path: &Path) -> Vec<Problem> {
        let (Some(session), Some(controller)) = (&self.session, self.documents.get(path)) else {
            return Vec::new();
        };
        let value = controller.value();
        let diagnostics = session.diagnostics(path, &value.snapshot());
        problems_of(&diagnostics, &value)
    }

    /// Asks the server what could be put at the caret of the file `path`. The server is told of the edits first, so
    /// that it is asked about the text as it is. None where it cannot be asked.
    pub fn complete(&mut self, path: &Path) -> Option<i64> {
        self.flush();
        let caret = self
            .documents
            .get(path)?
            .value()
            .selection()
            .range()
            .end;
        let id = self.session.as_mut()?.complete(path, caret)?;
        self.pump();
        Some(id)
    }

    /// Asks the server where the name at the byte `at` of the file `path` is defined. The server is told of the edits first.
    pub fn definition(&mut self, path: &Path, at: usize) -> Option<i64> {
        self.flush();
        let id = self.session.as_mut()?.definition(path, at)?;
        self.pump();
        Some(id)
    }

    /// Asks the server what the name at the byte `at` of the file `path` is, and what is written of it. The server is told of the
    /// edits first.
    pub fn hover(&mut self, path: &Path, at: usize) -> Option<i64> {
        self.flush();
        let id = self.session.as_mut()?.hover(path, at)?;
        self.pump();
        Some(id)
    }

    /// Where the byte `byte` of `text` is as the server counts, which is in UTF-16 until it has said.
    pub fn position_of(&self, text: &aimer::input::input::TextSnapshot, byte: usize) -> Position {
        match &self.session {
            Some(session) => session.position_of(text, byte),
            None => position::position_of(text, byte, position::Encoding::Utf16),
        }
    }

    /// The diagnostics of `path` that are about the text the server has, and that text.
    pub fn current_diagnostics(
        &self,
        path: &Path,
    ) -> Option<(
        Vec<super::session::Diagnostic>,
        aimer::input::input::TextSnapshot,
    )> {
        self.session.as_ref()?.current_diagnostics(path)
    }

    /// What the server calls the kinds and the modifiers of its tokens, if it has the tokens of a file to give and is ready.
    pub fn legend(&self) -> Option<std::rc::Rc<super::protocol::Legend>> {
        self.session
            .as_ref()
            .filter(|session| session.can_semantic())
            .and_then(Session::legend)
    }

    /// How many times the server said that its tokens are not as they were, since this was last asked.
    pub fn take_semantic_refreshes(&mut self) -> u64 {
        self.session
            .as_mut()
            .map_or(0, Session::take_semantic_refreshes)
    }

    /// Whether the server is ready and has hints to give.
    pub fn can_hint(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(Session::can_hint)
    }

    /// Makes the request `method` with `params`, whatever it is. The server is told of the edits first. None where it cannot
    /// be asked.
    pub fn ask(&mut self, method: &str, params: Value) -> Option<i64> {
        self.flush();
        let id = self.session.as_mut()?.ask(method, params)?;
        self.pump();
        Some(id)
    }

    /// The answers to the requests made by [`ask`](Self::ask) that came, which are not given again.
    pub fn take_replies(&mut self) -> Vec<(i64, Option<Value>)> {
        self.session
            .as_mut()
            .map_or_else(Vec::new, Session::take_replies)
    }

    /// The text of the file `path` as it is now, if it is one that is open.
    pub fn snapshot(&self, path: &Path) -> Option<aimer::input::input::TextSnapshot> {
        self.documents
            .get(path)
            .map(TextEditingController::snapshot)
    }

    /// The answers to the requests for a hover that came, which are not given again.
    pub fn take_hovers(&mut self) -> Vec<(i64, Option<Hover>)> {
        self.session
            .as_mut()
            .map_or_else(Vec::new, Session::take_hovers)
    }

    /// The answers to the requests for a definition that came, which are not given again.
    pub fn take_places(&mut self) -> Vec<(i64, Option<Vec<Place>>)> {
        self.session
            .as_mut()
            .map_or_else(Vec::new, Session::take_places)
    }

    /// The byte of `text` that the position of the server is, counted as the server counts, which is in UTF-16 until it has
    /// said.
    pub fn byte_of(&self, text: &aimer::input::input::TextSnapshot, at: Position) -> usize {
        match &self.session {
            Some(session) => session.byte_of(text, at),
            None => position::byte_of(text, at, position::Encoding::Utf16),
        }
    }

    /// Asks the server for the rest of an item, as `item` says to ask for it. The server is told of the edits first.
    pub fn resolve(&mut self, path: &Path, item: &Value) -> Option<i64> {
        self.flush();
        let id = self.session.as_mut()?.resolve(path, item)?;
        self.pump();
        Some(id)
    }

    /// The answers to the requests for the rest of an item that came, which are not given again.
    pub fn take_resolved(&mut self) -> Vec<(i64, Option<Vec<(Range<usize>, String)>>)> {
        self.session
            .as_mut()
            .map_or_else(Vec::new, Session::take_resolved)
    }

    /// Says that the answer to the request `id` is not wanted.
    pub fn cancel(&mut self, id: i64) {
        if let Some(session) = &mut self.session {
            session.cancel(id);
        }
        self.pump();
    }

    /// The answers to the requests for completion that came, which are not given again.
    pub fn take_answers(&mut self) -> Vec<(i64, Option<Answer>)> {
        self.session
            .as_mut()
            .map_or_else(Vec::new, Session::take_answers)
    }

    /// Ends the conversation as the app quits: asks the server to shut down, waits for it to say it has for
    /// as long as `wait` and no longer, and lets it go, which ends the process if it has not ended.
    pub fn finish(&mut self, wait: Duration) {
        if self.link.is_none() {
            return;
        }
        if let Some(session) = &mut self.session {
            session.shut_down();
        }
        self.pump();
        let deadline = Instant::now() + wait;
        loop {
            self.receive();
            let over = self.session.as_ref().is_none_or(|session| {
                matches!(session.status(), Status::Stopped | Status::Failed(_))
            });
            if over || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        self.link = None;
    }

    /// Sends what the session has to send.
    fn pump(&mut self) {
        if let (Some(link), Some(session)) = (&self.link, &mut self.session) {
            for body in session.take_outgoing() {
                link.send(body);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::protocol;
    use crate::lsp::transport::{self, Incoming};
    use serde_json::{Value, json};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::time::Duration;

    const FILE: &str = "/project/src/main.rs";

    /// A server that is a log of what it was sent and a mailbox the test fills.
    #[derive(Clone)]
    struct Fake {
        sent: Rc<RefCell<Vec<Vec<u8>>>>,
        mailbox: Arc<Mailbox<Event>>,
        starts: Rc<Cell<u32>>,
        /// How many of the fake's copies have been dropped.
        dropped: Rc<Cell<u32>>,
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            self.dropped.set(self.dropped.get() + 1);
        }
    }

    impl Fake {
        fn new() -> Self {
            Self {
                sent: Rc::default(),
                mailbox: Arc::new(Mailbox::new()),
                starts: Rc::default(),
                dropped: Rc::default(),
            }
        }

        fn starter(&self) -> Starter {
            let fake = self.clone();
            Box::new(move |_| {
                fake.starts.set(fake.starts.get() + 1);
                Ok(Box::new(fake.clone()) as Box<dyn Link>)
            })
        }

        /// What was sent since the last time asked, as `(method, params, id)`.
        fn take(&self) -> Vec<(String, Value, Option<i64>)> {
            self.sent
                .borrow_mut()
                .drain(..)
                .map(|body| match transport::parse(&body).expect("a message") {
                    Incoming::Request { id, method, params } => (method, params, id.as_i64()),
                    Incoming::Notification { method, params } => (method, params, None),
                    other => panic!("{other:?}"),
                })
                .collect()
        }

        fn says(&self, message: Incoming) {
            self.mailbox.push(Event::Message(message));
        }
    }

    impl Link for Fake {
        fn send(&self, body: Vec<u8>) {
            self.sent.borrow_mut().push(body);
        }

        fn mailbox(&self) -> Arc<Mailbox<Event>> {
            Arc::clone(&self.mailbox)
        }

        fn id(&self) -> u32 {
            4242
        }
    }

    fn methods(sent: &[(String, Value, Option<i64>)]) -> Vec<&str> {
        sent.iter()
            .map(|(method, _, _)| method.as_str())
            .collect()
    }

    fn core_with(fake: &Fake) -> Core {
        Core::new(PathBuf::from("/project"), fake.starter())
    }

    fn answer_initialize(fake: &Fake, core: &mut Core) {
        let id = fake.take()[0]
            .2
            .expect("initialize is a request");
        fake.says(Incoming::Response {
            id,
            result: Ok(
                json!({"capabilities": {"textDocumentSync": 2, "positionEncoding": "utf-8"}}),
            ),
        });
        assert!(core.receive());
    }

    #[test]
    fn nothing_is_run_until_a_document_is_open_and_then_the_server_is_run_once() {
        let fake = Fake::new();
        let mut core = core_with(&fake);
        let synced = core.sync();
        assert!(synced.started.is_none() && !synced.changed);
        assert_eq!(fake.starts.get(), 0);
        assert_eq!(core.summary(Path::new(FILE)).status, None);

        core.track(
            PathBuf::from(FILE),
            TextEditingController::with_text("fn main() {}\n"),
        );
        let synced = core.sync();
        assert!(
            synced.started.is_some() && synced.changed,
            "the server is run, and what shows it changed"
        );
        assert_eq!(fake.starts.get(), 1);
        let first = fake.take();
        assert_eq!(methods(&first), ["initialize"]);
        assert_eq!(first[0].1["processId"], 4242);
        assert_eq!(core.summary(Path::new(FILE)).status, Some(Status::Starting));

        core.track(
            PathBuf::from("/project/src/other.rs"),
            TextEditingController::with_text("x"),
        );
        let synced = core.sync();
        assert!(synced.started.is_none(), "the same server");
        assert_eq!(fake.starts.get(), 1);
    }

    /// Starts a server on each folder it is asked for, from `fakes` in turn, and notes the folders.
    fn rooted_starter(fakes: Vec<Fake>, roots: &Rc<RefCell<Vec<PathBuf>>>) -> Starter {
        let fakes = RefCell::new(std::collections::VecDeque::from(fakes));
        let roots = Rc::clone(roots);
        Box::new(move |root| {
            roots.borrow_mut().push(root.to_path_buf());
            let fake = fakes
                .borrow_mut()
                .pop_front()
                .expect("a fake for each start");
            Ok(Box::new(fake) as Box<dyn Link>)
        })
    }

    #[test]
    fn a_new_root_runs_the_server_again_on_the_new_folder_with_the_open_files() {
        let (first, second) = (Fake::new(), Fake::new());
        let roots = Rc::new(RefCell::new(Vec::new()));
        let mut core = Core::new(
            PathBuf::from("/home"),
            rooted_starter(vec![first.clone(), second.clone()], &roots),
        );
        core.track(PathBuf::from(FILE), TextEditingController::with_text("x"));
        let started = core
            .sync()
            .started
            .expect("run on the folder it began with");
        assert!(Arc::ptr_eq(&started, &first.mailbox));

        assert!(core.set_root(PathBuf::from("/project")), "another folder");
        assert!(
            first.dropped.get() >= 1,
            "the server of the old folder is let go"
        );
        let started = core
            .sync()
            .started
            .expect("run again, on the new folder");
        assert!(Arc::ptr_eq(&started, &second.mailbox));
        assert!(!core.listens_to(&first.mailbox) && core.listens_to(&second.mailbox));
        assert_eq!(
            *roots.borrow(),
            [PathBuf::from("/home"), PathBuf::from("/project")]
        );
        assert_eq!(
            methods(&second.take()),
            ["initialize"],
            "a new conversation, not a continuation of the old one"
        );
    }

    #[test]
    fn the_same_root_leaves_the_server_alone() {
        let fake = Fake::new();
        let mut core = core_with(&fake);
        core.track(PathBuf::from(FILE), TextEditingController::with_text("x"));
        core.sync();
        assert!(!core.set_root(PathBuf::from("/project")));
        assert!(core.sync().started.is_none());
        assert_eq!(fake.starts.get(), 1);
    }

    #[test]
    fn a_new_root_lets_a_server_that_could_not_be_run_be_run_again() {
        let calls = Rc::new(Cell::new(0));
        let counted = Rc::clone(&calls);
        let starter: Starter = Box::new(move |_| {
            counted.set(counted.get() + 1);
            Err(io::Error::new(io::ErrorKind::NotFound, "no such program"))
        });
        let mut core = Core::new(PathBuf::from("/home"), starter);
        core.track(PathBuf::from(FILE), TextEditingController::with_text("x"));
        core.sync();
        core.sync();
        assert_eq!(calls.get(), 1, "not run again on the same folder");
        assert!(core.set_root(PathBuf::from("/project")));
        core.sync();
        assert_eq!(calls.get(), 2, "run again on the new one");
    }

    #[test]
    fn a_server_that_was_never_run_just_takes_the_new_root() {
        let fake = Fake::new();
        let roots = Rc::new(RefCell::new(Vec::new()));
        let mut core = Core::new(
            PathBuf::from("/home"),
            rooted_starter(vec![fake.clone()], &roots),
        );
        assert!(core.set_root(PathBuf::from("/project")));
        core.track(PathBuf::from(FILE), TextEditingController::with_text("x"));
        core.sync();
        assert_eq!(*roots.borrow(), [PathBuf::from("/project")]);
    }

    #[test]
    fn a_server_that_cannot_be_run_is_said_so_once_and_not_run_again() {
        let calls = Rc::new(Cell::new(0));
        let counted = Rc::clone(&calls);
        let starter: Starter = Box::new(move |_| {
            counted.set(counted.get() + 1);
            Err(io::Error::new(io::ErrorKind::NotFound, "no such program"))
        });
        let mut core = Core::new(PathBuf::from("/project"), starter);
        core.track(PathBuf::from(FILE), TextEditingController::with_text("x"));
        let synced = core.sync();
        assert!(synced.started.is_none() && synced.changed);
        let Some(Status::Failed(why)) = core.summary(Path::new(FILE)).status else {
            panic!("a failure")
        };
        assert!(why.contains("no such program"), "{why}");
        let synced = core.sync();
        assert!(!synced.changed && synced.started.is_none());
        assert_eq!(calls.get(), 1, "not run again");
    }

    #[test]
    fn when_the_server_answers_it_is_told_of_the_documents_as_they_are_then() {
        let fake = Fake::new();
        let mut core = core_with(&fake);
        let controller = TextEditingController::with_text("fn main() {}\n");
        core.track(PathBuf::from(FILE), controller.clone());
        let _ = core.sync();
        // Typed before the server answers.
        controller.set_text("fn main() { let x = 1; }\n");
        assert!(core.mark_dirty(Path::new(FILE)));
        core.flush();
        let before = fake.take();
        assert_eq!(
            methods(&before),
            ["initialize"],
            "nothing but initialize before the answer"
        );

        let id = before[0].2.unwrap();
        fake.says(Incoming::Response {
            id,
            result: Ok(
                json!({"capabilities": {"textDocumentSync": 2, "positionEncoding": "utf-8"}}),
            ),
        });
        assert!(core.receive());
        let after = fake.take();
        assert_eq!(methods(&after), ["initialized", "textDocument/didOpen"]);
        assert_eq!(
            after[1].1["textDocument"]["text"],
            "fn main() { let x = 1; }\n"
        );
        assert_eq!(core.summary(Path::new(FILE)).status, Some(Status::Ready));
    }

    #[test]
    fn a_document_opened_after_the_server_is_ready_is_told_at_the_next_sync() {
        let fake = Fake::new();
        let mut core = core_with(&fake);
        core.track(PathBuf::from(FILE), TextEditingController::with_text("a\n"));
        let _ = core.sync();
        answer_initialize(&fake, &mut core);
        let _ = fake.take();
        core.track(
            PathBuf::from("/project/src/b.rs"),
            TextEditingController::with_text("b\n"),
        );
        let _ = core.sync();
        let told = fake.take();
        assert_eq!(methods(&told), ["textDocument/didOpen"]);
        assert_eq!(told[0].1["textDocument"]["uri"], "file:///project/src/b.rs");
        let _ = core.sync();
        assert!(fake.take().is_empty(), "told once");
    }

    #[test]
    fn edits_are_one_change_when_they_are_flushed_and_flushing_is_asked_for_once() {
        let fake = Fake::new();
        let mut core = core_with(&fake);
        let controller = TextEditingController::with_text("one\ntwo\n");
        core.track(PathBuf::from(FILE), controller.clone());
        let _ = core.sync();
        answer_initialize(&fake, &mut core);
        let _ = fake.take();

        controller.set_text("ONE\ntwo\n");
        assert!(
            core.mark_dirty(Path::new(FILE)),
            "the first change asks for a flush"
        );
        controller.set_text("ONE\nTWO\n");
        assert!(
            !core.mark_dirty(Path::new(FILE)),
            "the next does not: one is already asked for"
        );
        assert!(fake.take().is_empty(), "nothing is sent until the flush");
        core.flush();
        let told = fake.take();
        assert_eq!(methods(&told), ["textDocument/didChange"]);
        assert_eq!(told[0].1["textDocument"]["version"], 2);
        assert_eq!(told[0].1["contentChanges"][0]["text"], "ONE\nTWO");
        core.flush();
        assert!(fake.take().is_empty(), "flushed once");
        assert!(
            core.mark_dirty(Path::new(FILE)),
            "and a flush is asked for again by the next change"
        );
        core.flush();
        let _ = fake.take();
        assert!(
            !core.mark_dirty(Path::new("/not/tracked.rs")),
            "a file that is not open asks for nothing"
        );
        core.flush();
        assert!(fake.take().is_empty());
    }

    #[test]
    fn a_closed_document_is_closed_to_the_server_and_its_diagnostics_go() {
        let fake = Fake::new();
        let mut core = core_with(&fake);
        core.track(
            PathBuf::from(FILE),
            TextEditingController::with_text("let x = 1;\n"),
        );
        let _ = core.sync();
        answer_initialize(&fake, &mut core);
        let _ = fake.take();
        fake.says(Incoming::Notification {
            method: protocol::PUBLISH_DIAGNOSTICS.into(),
            params: json!({"uri": "file:///project/src/main.rs", "version": 1, "diagnostics": [
                {"range": {"start": {"line": 0, "character": 4}, "end": {"line": 0, "character": 5}}, "severity": 1, "message": "bad"},
                {"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 3}}, "severity": 2, "message": "meh"}]}),
        });
        assert!(core.receive(), "diagnostics are something to show");
        assert_eq!(
            core.summary(Path::new(FILE)),
            Summary {
                status: Some(Status::Ready),
                progress: None,
                errors: 1,
                warnings: 1
            }
        );
        core.untrack(Path::new(FILE));
        assert_eq!(methods(&fake.take()), ["textDocument/didClose"]);
        assert_eq!(
            (
                core.summary(Path::new(FILE)).errors,
                core.summary(Path::new(FILE)).warnings
            ),
            (0, 0)
        );
    }

    #[test]
    fn what_the_server_says_that_shows_nothing_is_not_a_change_and_a_request_is_answered() {
        let fake = Fake::new();
        let mut core = core_with(&fake);
        core.track(PathBuf::from(FILE), TextEditingController::with_text("x\n"));
        let _ = core.sync();
        answer_initialize(&fake, &mut core);
        let _ = fake.take();
        fake.says(Incoming::Notification {
            method: "$/progress".into(),
            params: json!({"token": 1}),
        });
        fake.mailbox
            .push(Event::Malformed("not a message".into()));
        assert!(!core.receive(), "nothing to show");
        fake.says(Incoming::Request {
            id: json!(9),
            method: "client/registerCapability".into(),
            params: json!({}),
        });
        assert!(!core.receive());
        let answered = fake.sent.borrow().len();
        assert_eq!(answered, 1, "the request is answered at once");
    }

    #[test]
    fn a_server_that_goes_is_a_failure_that_is_shown_with_its_reason() {
        let fake = Fake::new();
        let mut core = core_with(&fake);
        core.track(PathBuf::from(FILE), TextEditingController::with_text("x\n"));
        let _ = core.sync();
        answer_initialize(&fake, &mut core);
        fake.mailbox.push(Event::Gone(
            "the server closed its output (exit status: 101)".into(),
        ));
        assert!(core.receive());
        assert_eq!(
            core.summary(Path::new(FILE)).status,
            Some(Status::Failed(
                "the server closed its output (exit status: 101)".into()
            ))
        );
        // Nothing more is sent to it.
        let _ = fake.take();
        core.track(
            PathBuf::from("/project/b.rs"),
            TextEditingController::with_text("y"),
        );
        let _ = core.sync();
        assert!(fake.take().is_empty());
    }

    fn publish_for_file(fake: &Fake, version: i32, diagnostics: Value) {
        fake.says(Incoming::Notification {
            method: protocol::PUBLISH_DIAGNOSTICS.into(),
            params: json!({"uri": "file:///project/src/main.rs", "version": version, "diagnostics": diagnostics}),
        });
    }

    fn reported(line: u32, from: u32, to: u32, severity: u32, message: &str) -> Value {
        json!({"range": {"start": {"line": line, "character": from}, "end": {"line": line, "character": to}}, "severity": severity, "message": message, "code": format!("C{severity}")})
    }

    #[test]
    fn the_problems_of_a_file_are_listed_worst_first_with_where_they_are() {
        use crate::lsp::problems::Weight;
        let fake = Fake::new();
        let mut core = core_with(&fake);
        let controller =
            TextEditingController::with_text("let a = 1;\nlet b = \u{e9}x;\nlet c = 3;\n");
        core.track(PathBuf::from(FILE), controller.clone());
        let _ = core.sync();
        answer_initialize(&fake, &mut core);
        let _ = fake.take();
        assert!(
            core.problems(Path::new(FILE)).is_empty(),
            "nothing is published yet"
        );
        // In bytes, for the server was told it may: the \u{e9} is two bytes, so the x is at byte 10 of line 1.
        publish_for_file(
            &fake,
            1,
            json!([
                reported(2, 4, 5, 4, "a hint"),
                reported(1, 10, 11, 1, "an error\nwith a second line"),
                reported(0, 0, 3, 2, "a warning"),
                reported(1, 4, 5, 3, "some information")
            ]),
        );
        assert!(core.receive());
        let problems = core.problems(Path::new(FILE));
        let seen: Vec<(Weight, usize, usize, &str, Option<&str>)> = problems
            .iter()
            .map(|p| (p.weight, p.line, p.column, p.headline(), p.code.as_deref()))
            .collect();
        assert_eq!(
            seen,
            vec![
                (Weight::Error, 2, 10, "an error", Some("C1")),
                (Weight::Warning, 1, 1, "a warning", Some("C2")),
                (Weight::Weak, 2, 5, "some information", Some("C3")),
                (Weight::Weak, 3, 5, "a hint", Some("C4")),
            ],
            "the column counts the \u{e9} as one character, though it is two bytes, as the status bar does"
        );
        assert!(
            core.problems(Path::new("/not/open.rs"))
                .is_empty()
        );
    }

    #[test]
    fn the_problems_of_a_text_that_has_been_changed_since_are_not_listed() {
        let fake = Fake::new();
        let mut core = core_with(&fake);
        let controller = TextEditingController::with_text("let a = 1;\n");
        core.track(PathBuf::from(FILE), controller.clone());
        let _ = core.sync();
        answer_initialize(&fake, &mut core);
        let _ = fake.take();
        publish_for_file(&fake, 1, json!([reported(0, 4, 5, 1, "bad")]));
        core.receive();
        assert_eq!(core.problems(Path::new(FILE)).len(), 1);
        controller.set_text("// c\nlet a = 1;\n");
        core.mark_dirty(Path::new(FILE));
        core.flush();
        assert!(
            core.problems(Path::new(FILE)).is_empty(),
            "what was said is about the text before"
        );
        publish_for_file(&fake, 2, json!([reported(1, 4, 5, 1, "still bad")]));
        core.receive();
        let problems = core.problems(Path::new(FILE));
        assert_eq!(
            (problems.len(), problems[0].line),
            (1, 2),
            "the new publication is about this text"
        );
    }

    #[test]
    fn the_status_bar_says_how_the_server_stands_what_it_is_busy_with_and_how_many_problems_the_file_has()
     {
        let summary = |status: Option<Status>, errors, warnings| {
            Summary {
                status,
                progress: None,
                errors,
                warnings,
            }
            .label()
        };
        assert_eq!(
            summary(None, 0, 0),
            None,
            "nothing is said of a server that was not asked for"
        );
        assert_eq!(
            summary(Some(Status::Starting), 0, 0).as_deref(),
            Some("Language server starting\u{2026}")
        );
        assert_eq!(
            summary(Some(Status::Ready), 0, 0).as_deref(),
            Some("Language server: no problems")
        );
        assert_eq!(
            summary(Some(Status::Ready), 1, 0).as_deref(),
            Some("Language server: 1 error")
        );
        assert_eq!(
            summary(Some(Status::Ready), 2, 0).as_deref(),
            Some("Language server: 2 errors")
        );
        assert_eq!(
            summary(Some(Status::Ready), 0, 1).as_deref(),
            Some("Language server: 1 warning")
        );
        assert_eq!(
            summary(Some(Status::Ready), 3, 2).as_deref(),
            Some("Language server: 3 errors, 2 warnings")
        );
        assert_eq!(
            summary(Some(Status::Failed("it exited".into())), 0, 0).as_deref(),
            Some("Language server failed: it exited")
        );
        assert_eq!(
            summary(Some(Status::Stopped), 0, 0).as_deref(),
            Some("Language server stopped")
        );

        // What it is busy with is said, and the problems the file has so far with it.
        let busy = |errors, warnings| {
            Summary {
                status: Some(Status::Ready),
                progress: Some("Indexing 42%".into()),
                errors,
                warnings,
            }
            .label()
        };
        assert_eq!(
            busy(0, 0).as_deref(),
            Some("Language server: Indexing 42%\u{2026}")
        );
        assert_eq!(
            busy(2, 1).as_deref(),
            Some("Language server: Indexing 42%\u{2026} \u{b7} 2 errors, 1 warning")
        );
        // A server that is over is not busy, whatever it was last doing.
        let gone = Summary {
            status: Some(Status::Stopped),
            progress: Some("Indexing".into()),
            errors: 0,
            warnings: 0,
        }
        .label();
        assert_eq!(gone.as_deref(), Some("Language server stopped"));
    }

    #[test]
    fn what_the_server_is_busy_with_is_in_the_summary_and_the_interface_is_told_when_it_changes() {
        let fake = Fake::new();
        let mut core = core_with(&fake);
        core.track(PathBuf::from(FILE), TextEditingController::with_text("x\n"));
        let _ = core.sync();
        answer_initialize(&fake, &mut core);
        let _ = fake.take();
        assert_eq!(core.summary(Path::new(FILE)).progress, None);
        fake.says(Incoming::Notification { method: protocol::PROGRESS.into(), params: json!({"token": "a", "value": {"kind": "begin", "title": "Loading workspace", "percentage": 5}}) });
        assert!(core.receive(), "something to show");
        assert_eq!(
            core.summary(Path::new(FILE))
                .progress
                .as_deref(),
            Some("Loading workspace 5%")
        );
        fake.says(Incoming::Notification {
            method: protocol::PROGRESS.into(),
            params: json!({"token": "a", "value": {"kind": "end"}}),
        });
        assert!(core.receive());
        assert_eq!(core.summary(Path::new(FILE)).progress, None);
    }

    #[test]
    fn when_the_app_quits_the_server_is_asked_to_shut_down_exit_and_then_let_go() {
        let fake = Fake::new();
        let mut core = core_with(&fake);
        core.track(PathBuf::from(FILE), TextEditingController::with_text("x\n"));
        let _ = core.sync();
        answer_initialize(&fake, &mut core);
        let _ = fake.take();
        // The server answers the shutdown (the next request is number 2) before it is waited for.
        fake.says(Incoming::Response {
            id: 2,
            result: Ok(Value::Null),
        });
        core.finish(Duration::from_secs(5));
        assert_eq!(methods(&fake.take()), ["shutdown", "exit"]);
        assert_eq!(core.summary(Path::new(FILE)).status, Some(Status::Stopped));
        assert!(
            fake.dropped.get() >= 1,
            "the connection is let go, which ends the process"
        );
    }

    #[test]
    fn a_server_that_does_not_answer_is_not_waited_for_longer_than_it_is_told_to() {
        let fake = Fake::new();
        let mut core = core_with(&fake);
        core.track(PathBuf::from(FILE), TextEditingController::with_text("x\n"));
        let _ = core.sync();
        answer_initialize(&fake, &mut core);
        let _ = fake.take();
        let started = std::time::Instant::now();
        core.finish(Duration::from_millis(60));
        let waited = started.elapsed();
        assert!(
            waited >= Duration::from_millis(60) && waited < Duration::from_secs(3),
            "{waited:?}"
        );
        assert_eq!(methods(&fake.take()), ["shutdown"]);
        assert!(fake.dropped.get() >= 1, "let go all the same");
    }

    #[test]
    fn finishing_what_was_never_run_does_nothing() {
        let fake = Fake::new();
        let mut core = core_with(&fake);
        core.finish(Duration::from_secs(5));
        core.track(PathBuf::from(FILE), TextEditingController::with_text("x"));
        core.finish(Duration::from_secs(5));
        assert_eq!(fake.starts.get(), 0, "tracked but not synced is not run");
    }

    /// With the real rust-analyzer: what could be put at the caret is the items it says, with the places of the
    /// words they replace as bytes, and an item that is not in scope brings the line that imports it. Not run with
    /// the others: `cargo nextest run --run-ignored only real_rust_analyzer_completes`.
    #[test]
    #[ignore = "runs rust-analyzer"]
    fn real_rust_analyzer_completes_a_word_after_a_dot_a_path_and_an_item_that_is_not_in_scope() {
        let folder =
            std::env::temp_dir().join(format!("refractor-lsp-complete-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(folder.join("src")).unwrap();
        std::fs::write(
            folder.join("Cargo.toml"),
            "[package]\nname = \"lsptest\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        let file = folder.join("src/main.rs");
        let base = "mod shapes {\n    pub struct Circle;\n}\n\nfn main() {\n    let mut s = String::new();\n    ";
        std::fs::write(&file, format!("{base}\n}}\n")).unwrap();

        let starter: Starter = Box::new(|root| {
            Server::spawn("rust-analyzer", &[], root)
                .map(|server| Box::new(server) as Box<dyn Link>)
        });
        let mut core = Core::new(folder.clone(), starter);
        let controller = TextEditingController::with_text(&format!("{base}\n}}\n"));
        core.track(file.clone(), controller.clone());
        assert!(core.sync().started.is_some());
        let deadline = std::time::Instant::now() + Duration::from_secs(90);
        while core.summary(&file).status != Some(Status::Ready) {
            core.receive();
            assert!(
                std::time::Instant::now() < deadline,
                "not ready: {:?}",
                core.summary(&file)
            );
            std::thread::sleep(Duration::from_millis(10));
        }

        // Types `text` at the caret, which is at the end of the line, and asks what could follow it, until the server
        // says something: it answers that it has not finished with what it was told, while it has not.
        let ask = |core: &mut Core, text: &str| -> crate::lsp::Answer {
            let at = controller
                .value()
                .text()
                .rfind("\n}\n")
                .unwrap();
            let line_start = base.len();
            controller.replace_range(line_start..at, text);
            controller.select_bytes(line_start + text.len(), line_start + text.len());
            core.mark_dirty(&file);
            loop {
                let id = core
                    .complete(&file)
                    .expect("a ready server is asked");
                let waited = std::time::Instant::now() + Duration::from_secs(30);
                let answer = loop {
                    core.receive();
                    if let Some(found) = core
                        .take_answers()
                        .into_iter()
                        .find(|(wanted, _)| *wanted == id)
                    {
                        break found.1;
                    }
                    assert!(std::time::Instant::now() < waited, "no answer for {text:?}");
                    std::thread::sleep(Duration::from_millis(5));
                };
                match answer {
                    Some(answer) if !answer.items.is_empty() => return answer,
                    _ => {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "rust-analyzer had nothing to say of {text:?}"
                        );
                        std::thread::sleep(Duration::from_millis(300));
                    }
                }
            }
        };

        let word = ask(&mut core, "s.push_");
        let start = base.len() + 2;
        let push_str = word
            .items
            .iter()
            .find(|item| item.label.starts_with("push_str"))
            .unwrap_or_else(|| {
                panic!(
                    "push_str is not among {:?}",
                    word.items
                        .iter()
                        .map(|i| &i.label)
                        .take(20)
                        .collect::<Vec<_>>()
                )
            });
        assert_eq!(
            push_str
                .edit
                .range
                .as_ref()
                .map(|range| range.start),
            Some(start),
            "the word it replaces begins at push_"
        );
        assert!(
            push_str.edit.text.starts_with("push_str"),
            "{:?}",
            push_str.edit.text
        );

        let members = ask(&mut core, "s.");
        assert!(
            members.items.len() > 20,
            "a String has more members than {}",
            members.items.len()
        );
        assert!(
            members
                .items
                .iter()
                .any(|item| item.label == "len")
        );
        assert!(
            members.items.iter().all(|item| item
                .edit
                .range
                .as_ref()
                .is_none_or(|range| range.start == base.len() + 2)),
            "all begin where the caret is"
        );

        // What is not in scope is found in an index that the server is still making when it first answers.
        let circle = loop {
            let imported = ask(&mut core, "Circ");
            if let Some(circle) = imported
                .items
                .into_iter()
                .find(|item| item.label.starts_with("Circle"))
            {
                break circle;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Circle is not offered, though it could be imported"
            );
            std::thread::sleep(Duration::from_millis(500));
        };
        // It is not given with the item: it is asked for, as the editor does when the item is chosen.
        let payload = circle
            .resolve
            .as_ref()
            .expect("an item that is not in scope has something to resolve");
        let id = core
            .resolve(&file, payload)
            .expect("a ready server is asked");
        let waited = std::time::Instant::now() + Duration::from_secs(30);
        let edits = loop {
            core.receive();
            if let Some((_, edits)) = core
                .take_resolved()
                .into_iter()
                .find(|(wanted, _)| *wanted == id)
            {
                break edits.expect("the server answers");
            }
            assert!(
                std::time::Instant::now() < waited,
                "no answer to the request for the rest of Circle"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        assert!(
            edits
                .iter()
                .any(|(_, text)| text.contains("use crate::shapes::Circle;")),
            "it brings the line that imports it: {edits:?}"
        );
        // The line goes in front of the code, as bytes of the text: before the word it is for.
        assert!(
            edits
                .iter()
                .all(|(range, _)| range.end <= circle.edit.range.as_ref().unwrap().start),
            "{edits:?}"
        );
        eprintln!(
            "completion: {} items after `s.`, Circle edits {:?}",
            members.items.len(),
            edits
        );
        core.finish(Duration::from_secs(10));
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// With the real rust-analyzer: the definition of a function is found from a call of it, as a place in the file. Not run
    /// with the others: `cargo nextest run --run-ignored only real_rust_analyzer_finds`.
    #[test]
    #[ignore = "runs rust-analyzer"]
    fn real_rust_analyzer_finds_where_a_function_is_defined_from_a_call_of_it() {
        let folder =
            std::env::temp_dir().join(format!("refractor-lsp-definition-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(folder.join("src")).unwrap();
        std::fs::write(
            folder.join("Cargo.toml"),
            "[package]\nname = \"lsptest\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        let file = folder.join("src/main.rs");
        let text = "fn caf\u{e9}_target() {}\n\nfn main() {\n    caf\u{e9}_target();\n}\n";
        std::fs::write(&file, text).unwrap();

        let starter: Starter = Box::new(|root| {
            Server::spawn("rust-analyzer", &[], root)
                .map(|server| Box::new(server) as Box<dyn Link>)
        });
        let mut core = Core::new(folder.clone(), starter);
        let controller = TextEditingController::with_text(text);
        core.track(file.clone(), controller.clone());
        assert!(core.sync().started.is_some());
        let deadline = std::time::Instant::now() + Duration::from_secs(90);
        while core.summary(&file).status != Some(Status::Ready) {
            core.receive();
            assert!(
                std::time::Instant::now() < deadline,
                "not ready: {:?}",
                core.summary(&file)
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        // In the middle of the name of the call, which has an accent in it: the byte is not the character.
        let call = text.rfind("caf\u{e9}_target").unwrap() + 5;
        let places = loop {
            let id = core
                .definition(&file, call)
                .expect("a ready server is asked");
            let waited = std::time::Instant::now() + Duration::from_secs(30);
            let answer = loop {
                core.receive();
                if let Some(found) = core
                    .take_places()
                    .into_iter()
                    .find(|(wanted, _)| *wanted == id)
                {
                    break found.1;
                }
                assert!(std::time::Instant::now() < waited, "no answer");
                std::thread::sleep(Duration::from_millis(5));
            };
            match answer {
                Some(places) if !places.is_empty() => break places,
                _ => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "rust-analyzer found no definition"
                    );
                    std::thread::sleep(Duration::from_millis(300));
                }
            }
        };
        assert_eq!(places[0].path, file, "in the same file");
        assert_eq!(
            places[0].at.line, 0,
            "on the first line, where the function is"
        );
        // The server counts as it was agreed: the name begins after `fn `, and the position is turned into the byte of the text.
        let start = core.byte_of(&controller.snapshot(), places[0].at);
        assert_eq!(
            &text[start..start + 3],
            "caf",
            "the byte is where the name begins: {start}"
        );
        assert_eq!(start, 3);
        core.finish(Duration::from_secs(10));
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// With the real rust-analyzer, which is what this is all for: a file with a mistake in it gets an
    /// error, and the error goes when the mistake is mended. Not run with the others, for it needs the
    /// program and takes a few seconds: `cargo nextest run --run-ignored only real_rust_analyzer`.
    #[test]
    #[ignore = "runs rust-analyzer"]
    fn real_rust_analyzer_reports_a_mistake_and_stops_reporting_it_when_it_is_mended() {
        let folder =
            std::env::temp_dir().join(format!("refractor-lsp-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(folder.join("src")).unwrap();
        std::fs::write(
            folder.join("Cargo.toml"),
            "[package]\nname = \"lsptest\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        let broken = "fn main() {\n    let x: u32 = \"not a number\";\n    println!(\"{x}\");\n}\n";
        let mended = "fn main() {\n    let x: u32 = 1;\n    println!(\"{x}\");\n}\n";
        let file = folder.join("src/main.rs");
        std::fs::write(&file, broken).unwrap();

        let starter: Starter = Box::new(|root| {
            Server::spawn("rust-analyzer", &[], root)
                .map(|server| Box::new(server) as Box<dyn Link>)
        });
        let mut core = Core::new(folder.clone(), starter);
        let controller = TextEditingController::with_text(broken);
        core.track(file.clone(), controller.clone());
        let synced = core.sync();
        assert!(synced.started.is_some(), "{:?}", core.summary(&file));

        let jobs_seen: RefCell<Vec<String>> = RefCell::default();
        let wait_until = |core: &mut Core, what: &str, done: &dyn Fn(&Summary) -> bool| {
            let deadline = std::time::Instant::now() + Duration::from_secs(90);
            loop {
                core.receive();
                core.flush();
                let summary = core.summary(&file);
                if let Some(job) = &summary.progress {
                    let mut seen = jobs_seen.borrow_mut();
                    if !seen.contains(job) {
                        seen.push(job.clone());
                    }
                }
                if done(&summary) {
                    return summary;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "rust-analyzer did not {what} in ninety seconds: {summary:?}"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        let ready = wait_until(&mut core, "become ready", &|summary| {
            summary.status == Some(Status::Ready)
        });
        assert_eq!(ready.status, Some(Status::Ready));
        let broken_summary = wait_until(&mut core, "report the mistake", &|summary| {
            summary.errors > 0
        });
        assert!(broken_summary.errors >= 1, "{broken_summary:?}");

        controller.set_text(mended);
        assert!(core.mark_dirty(&file));
        let mended_summary = wait_until(&mut core, "stop reporting the mistake", &|summary| {
            summary.errors == 0 && summary.status == Some(Status::Ready)
        });
        assert_eq!(mended_summary.errors, 0);

        // What it was busy with while it loaded was reported, which is what the status bar shows.
        eprintln!("jobs seen: {:?}", jobs_seen.borrow());
        assert!(
            !jobs_seen.borrow().is_empty(),
            "rust-analyzer reported no job while it loaded"
        );
        core.finish(Duration::from_secs(10));
        assert_eq!(
            core.summary(&file).status,
            Some(Status::Stopped),
            "it shut down when asked"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }
}
