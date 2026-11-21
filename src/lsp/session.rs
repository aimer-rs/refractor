//! The conversation with one server, without the server: what is to be sent, kept as bodies of messages,
//! and what is made of what comes back. It does no reading or writing of its own, so that all of it can
//! be tried with a server that is made of a script.
//!
//! The documents it knows are told to the server when it is ready to be told, and after that their changes
//! are, a version at a time. The version of a document is counted here and the server says which version
//! its diagnostics are about, so that diagnostics of a text that has been changed since are not drawn on
//! the text as it is now.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use aimer::input::input::TextSnapshot;
use serde_json::{Value, json};

use super::items::{self, Answer, Hover};
use super::position::{self, Encoding};
use super::protocol::{
    self, Capabilities, Change, Health, Legend, Place, Progress, ProgressUpdate, Published, Severity,
    SyncKind,
};
use super::transport::{self, Incoming};

/// How the conversation stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// `initialize` is sent and not answered.
    Starting,
    /// The server has answered, and the documents are told to it.
    Ready,
    /// It did not answer well, or it has gone: what was said, or why it stopped.
    Failed(String),
    /// It was asked to shut down, and has.
    Stopped,
}

/// A diagnostic of a text, in the terms of the editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// The bytes it is about. It can be empty: a problem that is at a point and not on text.
    pub range: Range<usize>,
    pub severity: Severity,
    pub message: String,
    pub code: Option<String>,
}

/// A job the server is doing, as far as it has said.
struct Job {
    token: String,
    title: String,
    message: Option<String>,
    percentage: Option<u32>,
}

impl Job {
    /// What is shown of it: its title and how far it is, or what it says it is doing.
    fn label(&self) -> String {
        match (self.percentage, self.message.as_deref()) {
            (Some(percentage), _) => format!("{} {percentage}%", self.title),
            (None, Some(message)) if !message.is_empty() => format!("{}: {message}", self.title),
            _ => self.title.clone(),
        }
    }
}

/// A file the server is, or is to be, told about.
struct Document {
    /// How many times the server was told of a text of it, from one when it was opened.
    version: i32,
    /// The text as it is now, as far as the session was told.
    latest: TextSnapshot,
    /// The text the server has, which the next change is made from. `None` until it is told it.
    sent: Option<TextSnapshot>,
}

/// The conversation with one server.
pub struct Session {
    status: Status,
    capabilities: Option<Capabilities>,
    next_id: i64,
    initialize_id: Option<i64>,
    shutdown_id: Option<i64>,
    documents: HashMap<PathBuf, Document>,
    /// What the server last published for each file.
    published: HashMap<PathBuf, Published>,
    /// The jobs the server has begun and not ended, the latest last.
    jobs: Vec<Job>,
    /// Whether the server says it has nothing left to do, and what it says it is doing if it has.
    quiescent: bool,
    busy_with: Option<String>,
    /// The requests for completion that are not answered, with the text each was made for.
    completions: HashMap<i64, TextSnapshot>,
    /// The answers to them that are not taken yet, in the order they came: the items, or none where the server
    /// could not say.
    answers: Vec<(i64, Option<Answer>)>,
    /// The requests for a definition that are not answered.
    definitions: HashSet<i64>,
    /// The answers to them that are not taken yet: where the name is defined, or none where the server could not say.
    places: Vec<(i64, Option<Vec<Place>>)>,
    /// The requests for what to say of a name that are not answered, with the text each was made for.
    hovers: HashMap<i64, TextSnapshot>,
    /// The answers to them that are not taken yet: the words, or none where the server had none or could not say.
    hovered: Vec<(i64, Option<Hover>)>,
    /// The requests for the rest of an item that are not answered, with the text each was made for.
    resolves: HashMap<i64, TextSnapshot>,
    /// The answers to them that are not taken yet: the edits the item asks for, or none where the server could
    /// not say.
    resolved: Vec<(i64, Option<Vec<(Range<usize>, String)>>)>,
    /// The requests that were made by the name of the method, and not for something of their own, that are not answered.
    asked: HashSet<i64>,
    /// The answers to them that are not taken yet: what the server said, or none where it said an error.
    replies: Vec<(i64, Option<Value>)>,
    /// What is to be sent, in order.
    outgoing: Vec<Vec<u8>>,
    /// Moves when what there is to show changes: the status, and the diagnostics a file has.
    revision: u64,
    /// What the server calls the kinds and the modifiers of its tokens, if it has the tokens of a file to give.
    legend: Option<Rc<Legend>>,
    /// How many times the server said that its tokens are not as they were, since that was last asked.
    semantic_refreshes: u64,
}

/// The code of an answer that says the server asked for something that is not done here.
const METHOD_NOT_FOUND: i64 = -32601;

impl Session {
    /// A conversation that has asked the server to initialize for the folder `root`.
    pub fn new(root: &Path, process_id: u32) -> Self {
        let mut session = Self {
            status: Status::Starting,
            capabilities: None,
            next_id: 1,
            initialize_id: None,
            shutdown_id: None,
            documents: HashMap::new(),
            published: HashMap::new(),
            jobs: Vec::new(),
            quiescent: true,
            busy_with: None,
            completions: HashMap::new(),
            answers: Vec::new(),
            definitions: HashSet::new(),
            places: Vec::new(),
            hovers: HashMap::new(),
            hovered: Vec::new(),
            resolves: HashMap::new(),
            resolved: Vec::new(),
            asked: HashSet::new(),
            replies: Vec::new(),
            outgoing: Vec::new(),
            revision: 0,
            legend: None,
            semantic_refreshes: 0,
        };
        let id = session.take_id();
        session.initialize_id = Some(id);
        session.outgoing.push(transport::request(
            id,
            protocol::INITIALIZE,
            protocol::initialize_params(root, process_id),
        ));
        session
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    /// A number that is another when the status or the diagnostics of a file are, so that what shows them
    /// is drawn again when it is and not when it is not.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// What is to be sent to the server now, in order, which is not asked for again.
    pub fn take_outgoing(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.outgoing)
    }

    /// Tells the session that the file `path`, whose text is `text`, is open.
    pub fn open(&mut self, path: &Path, text: &TextSnapshot) {
        if self.is_over() || self.documents.contains_key(path) {
            return;
        }
        self.documents.insert(
            path.to_path_buf(),
            Document {
                version: 0,
                latest: text.clone(),
                sent: None,
            },
        );
        if self.status == Status::Ready {
            self.tell_open(path);
        }
    }

    /// Tells the session that the text of the file `path` is now `text`. The server is told of it as a
    /// change from the text it has.
    pub fn changed(&mut self, path: &Path, text: &TextSnapshot) {
        let (sync, encoding) = (self.sync(), self.encoding());
        let ready = self.status == Status::Ready;
        let Some(document) = self.documents.get_mut(path) else {
            return;
        };
        document.latest = text.clone();
        let Some(sent) = document
            .sent
            .clone()
            .filter(|_| ready && sync != SyncKind::None)
        else {
            return;
        };
        let Some((old, new)) = sent.diff(text) else {
            return;
        };
        let change = match sync {
            SyncKind::Incremental => Change::Incremental {
                range: (
                    position::position_of(&sent, old.start, encoding),
                    position::position_of(&sent, old.end, encoding),
                ),
                text: text.slice(new).into_owned(),
            },
            _ => Change::Full(text.slice(0..text.len()).into_owned()),
        };
        document.version += 1;
        document.sent = Some(text.clone());
        let version = document.version;
        if self.published.contains_key(path) {
            // What was published is about the text before.
            self.revision += 1;
        }
        self.outgoing.push(transport::notification(
            protocol::DID_CHANGE,
            protocol::did_change_params(path, version, &change),
        ));
    }

    /// Tells the session that the file `path` is closed.
    pub fn close(&mut self, path: &Path) {
        let Some(document) = self.documents.remove(path) else {
            return;
        };
        if self.published.remove(path).is_some() {
            self.revision += 1;
        }
        if self.status == Status::Ready && document.version > 0 {
            self.outgoing.push(transport::notification(
                protocol::DID_CLOSE,
                protocol::did_close_params(path),
            ));
        }
    }

    /// Tells the session that the file `path` was saved, which a server such as rust-analyzer answers by checking the project. Nothing
    /// is said of a file that the server was not told of, or to a server that is not ready.
    pub fn saved(&mut self, path: &Path) {
        if self.status == Status::Ready && self.documents.get(path).is_some_and(|document| document.version > 0) {
            self.outgoing.push(transport::notification(protocol::DID_SAVE, protocol::did_save_params(path)));
        }
    }

    /// What the server said, which may call for an answer.
    pub fn receive(&mut self, message: Incoming) {
        match message {
            Incoming::Response { id, result } if Some(id) == self.initialize_id => {
                self.initialize_id = None;
                self.initialized(result);
            }
            Incoming::Response { id, .. } if Some(id) == self.shutdown_id => {
                self.shutdown_id = None;
                self.outgoing
                    .push(transport::notification(protocol::EXIT, Value::Null));
                self.status = Status::Stopped;
                self.revision += 1;
            }
            Incoming::Response { id, result } if self.completions.contains_key(&id) => {
                let text = self
                    .completions
                    .remove(&id)
                    .expect("it was found");
                // What is not made as it should be is no answer, as an error is.
                let answer = result
                    .ok()
                    .and_then(|value| protocol::completion_result(value).ok())
                    .map(|found| items::answer_of(found, &text, self.encoding()));
                self.answers.push((id, answer));
            }
            Incoming::Response { id, result } if self.definitions.contains(&id) => {
                self.definitions.remove(&id);
                // What is not made as it should be is no answer, as an error is.
                let places = result
                    .ok()
                    .and_then(|value| protocol::definition_result(value).ok());
                self.places.push((id, places));
            }
            Incoming::Response { id, result } if self.hovers.contains_key(&id) => {
                let text = self.hovers.remove(&id).expect("it was found");
                let encoding = self.encoding();
                // What is not made as it should be is no answer, as an error is, and so is a name the server has nothing to say of.
                let hover = result
                    .ok()
                    .and_then(|value| protocol::hover_result(value).ok())
                    .flatten()
                    .map(|said| Hover {
                        markdown: said.markdown,
                        range: said.range.map(|(from, to)| {
                            let start = position::byte_of(&text, from, encoding);
                            start..position::byte_of(&text, to, encoding).max(start)
                        }),
                    });
                self.hovered.push((id, hover));
            }
            Incoming::Response { id, result } if self.resolves.contains_key(&id) => {
                let text = self.resolves.remove(&id).expect("it was found");
                let encoding = self.encoding();
                let edits = result
                    .ok()
                    .and_then(|value| protocol::resolve_result(value).ok())
                    .map(|edits| {
                        edits
                            .into_iter()
                            .map(|((from, to), new_text)| {
                                let start = position::byte_of(&text, from, encoding);
                                (
                                    start..position::byte_of(&text, to, encoding).max(start),
                                    new_text,
                                )
                            })
                            .collect()
                    });
                self.resolved.push((id, edits));
            }
            Incoming::Response { id, result } if self.asked.remove(&id) => {
                self.replies.push((id, result.ok()));
            }
            Incoming::Response { .. } => {}
            Incoming::Notification { method, params }
                if method == protocol::PUBLISH_DIAGNOSTICS =>
            {
                // What is not about a file that is open, or is not made as it should be, is passed over.
                if let Ok(published) = protocol::publish_diagnostics(&params)
                    && self.documents.contains_key(&published.path)
                {
                    self.published
                        .insert(published.path.clone(), published);
                    self.revision += 1;
                }
            }
            Incoming::Notification { method, params } if method == protocol::PROGRESS => {
                if let Ok(step) = protocol::progress(&params) {
                    let before = self.progress_label();
                    self.step(step);
                    if self.progress_label() != before {
                        self.revision += 1;
                    }
                }
            }
            Incoming::Notification { method, params } if method == protocol::SERVER_STATUS => {
                if let Ok(status) = protocol::server_status(&params) {
                    let before = self.progress_label();
                    self.quiescent = status.quiescent;
                    // What a server that is well says is what it is doing, on its first line. What one that
                    // is not says is why, which is not that.
                    self.busy_with = status
                        .message
                        .filter(|_| status.health == Health::Ok)
                        .and_then(|message| {
                            message
                                .lines()
                                .next()
                                .map(str::trim)
                                .filter(|line| !line.is_empty())
                                .map(str::to_owned)
                        });
                    if self.progress_label() != before {
                        self.revision += 1;
                    }
                }
            }
            Incoming::Notification { .. } => {}
            Incoming::Request { id, method, params } => self.respond(&id, &method, &params),
        }
    }

    /// Asks the server what could be put at the byte `caret` of the file `path`, as the server has the text. None
    /// where it cannot be asked: the server is not ready or does not complete, or the file is not one it has been
    /// told of. The answer comes to [`take_answers`](Self::take_answers) with the number that is returned.
    pub fn complete(&mut self, path: &Path, caret: usize) -> Option<i64> {
        if self.status != Status::Ready
            || !self
                .capabilities
                .is_some_and(|capabilities| capabilities.completion)
        {
            return None;
        }
        let text = self.documents.get(path)?.sent.clone()?;
        let at = position::position_of(&text, caret, self.encoding());
        let id = self.take_id();
        self.completions.insert(id, text);
        self.outgoing.push(transport::request(
            id,
            protocol::COMPLETION,
            protocol::completion_params(path, at),
        ));
        Some(id)
    }

    /// Asks the server where the name at the byte `at` of the file `path` is defined, as the server has the text. None where it
    /// cannot be asked: the server is not ready or does not find definitions, or the file is not one it has been told of.
    pub fn definition(&mut self, path: &Path, at: usize) -> Option<i64> {
        if self.status != Status::Ready
            || !self
                .capabilities
                .is_some_and(|capabilities| capabilities.definition)
        {
            return None;
        }
        let text = self.documents.get(path)?.sent.clone()?;
        let position = position::position_of(&text, at, self.encoding());
        let id = self.take_id();
        self.definitions.insert(id);
        self.outgoing.push(transport::request(
            id,
            protocol::DEFINITION,
            protocol::definition_params(path, position),
        ));
        Some(id)
    }

    /// Asks the server what the name at the byte `at` of the file `path` is, and what is written of it, as the server has the
    /// text. None where it cannot be asked: the server is not ready or does not say, or the file is not one it has been told of.
    /// The answer comes to [`take_hovers`](Self::take_hovers) with the number that is returned.
    pub fn hover(&mut self, path: &Path, at: usize) -> Option<i64> {
        if self.status != Status::Ready
            || !self
                .capabilities
                .is_some_and(|capabilities| capabilities.hover)
        {
            return None;
        }
        let text = self.documents.get(path)?.sent.clone()?;
        let position = position::position_of(&text, at, self.encoding());
        let id = self.take_id();
        self.hovers.insert(id, text);
        self.outgoing.push(transport::request(
            id,
            protocol::HOVER,
            protocol::hover_params(path, position),
        ));
        Some(id)
    }

    /// Where the byte `byte` of `text` is, counted as the server counts, which is in UTF-16 until it has said.
    pub fn position_of(&self, text: &TextSnapshot, byte: usize) -> position::Position {
        position::position_of(text, byte, self.encoding())
    }

    /// What the server calls the kinds and the modifiers of its tokens, if it has the tokens of a file to give.
    pub fn legend(&self) -> Option<Rc<Legend>> {
        self.legend.clone()
    }

    /// Whether the server is ready and has the tokens of a file to give.
    pub fn can_semantic(&self) -> bool {
        self.status == Status::Ready && self.legend.is_some()
    }

    /// How many times the server said that its tokens are not as they were, since this was last asked.
    pub fn take_semantic_refreshes(&mut self) -> u64 {
        std::mem::take(&mut self.semantic_refreshes)
    }

    /// Whether the server is ready and says it has hints to give.
    pub fn can_hint(&self) -> bool {
        self.status == Status::Ready
            && self
                .capabilities
                .is_some_and(|capabilities| capabilities.inlay_hints)
    }

    /// Makes the request `method` with `params`, whatever it is, for a caller that knows what it asks and reads the answer
    /// itself. None where the server is not ready. The answer comes to [`take_replies`](Self::take_replies) with the number
    /// that is returned.
    pub fn ask(&mut self, method: &str, params: Value) -> Option<i64> {
        if self.status != Status::Ready {
            return None;
        }
        let id = self.take_id();
        self.asked.insert(id);
        self.outgoing.push(transport::request(id, method, params));
        Some(id)
    }

    /// The answers to the requests made by [`ask`](Self::ask) that came since this was last asked.
    pub fn take_replies(&mut self) -> Vec<(i64, Option<Value>)> {
        std::mem::take(&mut self.replies)
    }

    /// The answers to the requests for a hover that came since this was last asked.
    pub fn take_hovers(&mut self) -> Vec<(i64, Option<Hover>)> {
        std::mem::take(&mut self.hovered)
    }

    /// The answers to the requests for a definition that came since this was last asked.
    pub fn take_places(&mut self) -> Vec<(i64, Option<Vec<Place>>)> {
        std::mem::take(&mut self.places)
    }

    /// The byte of `text` that `at`, a position of the server's, is: it counts as it was agreed that it would, which is in
    /// UTF-16 until the server has said.
    pub fn byte_of(&self, text: &TextSnapshot, at: position::Position) -> usize {
        position::byte_of(text, at, self.encoding())
    }

    /// Asks the server for the rest of an item of the file `path`, as `item` says to ask for it: the edits it asks
    /// for, which are in the answer as bytes of the text the server has. None where it cannot be asked.
    pub fn resolve(&mut self, path: &Path, item: &Value) -> Option<i64> {
        if self.status != Status::Ready
            || !self
                .capabilities
                .is_some_and(|capabilities| capabilities.completion)
        {
            return None;
        }
        let text = self.documents.get(path)?.sent.clone()?;
        let id = self.take_id();
        self.resolves.insert(id, text);
        self.outgoing.push(transport::request(
            id,
            protocol::COMPLETION_RESOLVE,
            item.clone(),
        ));
        Some(id)
    }

    /// The answers to the requests for the rest of an item that came since this was last asked.
    pub fn take_resolved(&mut self) -> Vec<(i64, Option<Vec<(Range<usize>, String)>>)> {
        std::mem::take(&mut self.resolved)
    }

    /// Says that the answer to the request `id` for completion, or for the rest of an item, is not wanted any more,
    /// which lets the server stop working it out, and no answer to it is kept.
    pub fn cancel(&mut self, id: i64) {
        if self.completions.remove(&id).is_some()
            | self.resolves.remove(&id).is_some()
            | self.definitions.remove(&id)
            | self.hovers.remove(&id).is_some()
            | self.asked.remove(&id)
        {
            self.outgoing.push(transport::notification(
                protocol::CANCEL_REQUEST,
                json!({"id": id}),
            ));
        }
    }

    /// The answers to the requests for completion that came since this was last asked.
    pub fn take_answers(&mut self) -> Vec<(i64, Option<Answer>)> {
        std::mem::take(&mut self.answers)
    }

    /// The server has stopped, or cannot be spoken to.
    pub fn server_gone(&mut self, why: &str) {
        if !self.is_over() {
            self.status = Status::Failed(why.to_owned());
            self.revision += 1;
        }
    }

    /// Asks the server to shut down, and to exit when it has.
    pub fn shut_down(&mut self) {
        match self.status {
            Status::Ready => {
                let id = self.take_id();
                self.shutdown_id = Some(id);
                self.outgoing
                    .push(transport::request(id, protocol::SHUTDOWN, Value::Null));
            }
            // A server that has not answered cannot be asked anything, and one that is over needs nothing.
            Status::Starting => {
                self.status = Status::Stopped;
                self.revision += 1;
            }
            Status::Failed(_) | Status::Stopped => {}
        }
    }

    /// The diagnostics of `path` as bytes of `text`, which must be the text as it is. None when the
    /// server has published none, and none that were published for a text that has been changed since.
    pub fn diagnostics(&self, path: &Path, text: &TextSnapshot) -> Vec<Diagnostic> {
        let (Some(published), Some(document)) =
            (self.published.get(path), self.documents.get(path))
        else {
            return Vec::new();
        };
        if published
            .version
            .is_some_and(|version| version != document.version)
        {
            return Vec::new();
        }
        let encoding = self.encoding();
        published
            .diagnostics
            .iter()
            .map(|reported| {
                let start = position::byte_of(text, reported.range.0, encoding);
                let end = position::byte_of(text, reported.range.1, encoding).max(start);
                Diagnostic {
                    range: start..end,
                    severity: reported.severity,
                    message: reported.message.clone(),
                    code: reported.code.clone(),
                }
            })
            .collect()
    }

    /// The diagnostics of `path` that are about the text the server has, and that text, which their bytes are of; or none where the
    /// file is not one the server has been told of, or the server has since been told of a text that it has not yet said anything
    /// of. Nothing published is none that are wrong, and so is empty and not none.
    pub fn current_diagnostics(&self, path: &Path) -> Option<(Vec<Diagnostic>, TextSnapshot)> {
        let document = self.documents.get(path)?;
        let text = document.sent.clone()?;
        let Some(published) = self.published.get(path) else {
            return Some((Vec::new(), text));
        };
        if published.version.is_some_and(|version| version != document.version) {
            return None;
        }
        Some((self.diagnostics(path, &text), text))
    }

    /// What the server is busy with, if it is: the latest job it has begun and not ended, or what it says
    /// when it says that it is not idle and names no job. Nothing for a server that is over, whatever it
    /// was doing.
    pub fn progress_label(&self) -> Option<String> {
        if self.is_over() {
            return None;
        }
        self.jobs.last().map(Job::label).or_else(|| {
            (!self.quiescent).then(|| {
                self.busy_with
                    .clone()
                    .unwrap_or_else(|| "Working".to_owned())
            })
        })
    }

    /// How many errors and how many warnings the server has published for `path`, as it is now.
    pub fn counts_of(&self, path: &Path) -> (usize, usize) {
        let (Some(published), Some(document)) =
            (self.published.get(path), self.documents.get(path))
        else {
            return (0, 0);
        };
        if published
            .version
            .is_some_and(|version| version != document.version)
        {
            return (0, 0);
        }
        let count = |severity| {
            published
                .diagnostics
                .iter()
                .filter(|reported| reported.severity == severity)
                .count()
        };
        (count(Severity::Error), count(Severity::Warning))
    }

    /// The version the server has of `path`, if it was told of it.
    #[cfg(test)]
    pub fn version_of(&self, path: &Path) -> Option<i32> {
        self.documents
            .get(path)
            .map(|document| document.version)
            .filter(|version| *version > 0)
    }

    /// Applies a step of a job. A report or an end for a job that was not begun is passed over.
    fn step(&mut self, step: Progress) {
        let position = self
            .jobs
            .iter()
            .position(|job| job.token == step.token);
        match (step.update, position) {
            (
                ProgressUpdate::Begin {
                    title,
                    message,
                    percentage,
                },
                existing,
            ) => {
                // A job that begins again is the same job, and is the latest.
                if let Some(existing) = existing {
                    self.jobs.remove(existing);
                }
                self.jobs.push(Job {
                    token: step.token,
                    title,
                    message,
                    percentage,
                });
            }
            (
                ProgressUpdate::Report {
                    message,
                    percentage,
                },
                Some(at),
            ) => {
                let job = &mut self.jobs[at];
                job.message = message.or(job.message.take());
                job.percentage = percentage.or(job.percentage);
            }
            (ProgressUpdate::End, Some(at)) => {
                self.jobs.remove(at);
            }
            (ProgressUpdate::Report { .. } | ProgressUpdate::End, None) => {}
        }
        // A job that is at a hundred per cent is done, which not every server says: rust-analyzer reports
        // the scan of the files at a hundred, over and over, and never ends it.
        self.jobs
            .retain(|job| job.percentage != Some(100));
    }

    /// Whether the conversation is over, and nothing more is told.
    fn is_over(&self) -> bool {
        matches!(self.status, Status::Failed(_) | Status::Stopped)
    }

    fn take_id(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn encoding(&self) -> Encoding {
        self.capabilities
            .map_or(Encoding::Utf16, |capabilities| capabilities.encoding)
    }

    fn sync(&self) -> SyncKind {
        self.capabilities
            .map_or(SyncKind::None, |capabilities| capabilities.sync)
    }

    /// What the server answered to `initialize`: the session is ready and the server is told of the files.
    fn initialized(&mut self, result: Result<Value, transport::ResponseError>) {
        self.legend = result.as_ref().ok().and_then(protocol::semantic_legend).map(Rc::new);
        let capabilities = match result {
            Ok(result) => protocol::initialize_result(&result)
                .map_err(|why| format!("the server's answer to initialize was not one: {why}")),
            Err(error) => Err(format!(
                "the server could not initialize: {}",
                error.message
            )),
        };
        match capabilities {
            Ok(capabilities) => {
                self.capabilities = Some(capabilities);
                self.status = Status::Ready;
                self.outgoing
                    .push(transport::notification(protocol::INITIALIZED, json!({})));
                let mut paths: Vec<PathBuf> = self.documents.keys().cloned().collect();
                paths.sort();
                for path in paths {
                    self.tell_open(&path);
                }
            }
            Err(why) => self.status = Status::Failed(why),
        }
        self.revision += 1;
    }

    /// Tells the server of a file as its text is now.
    fn tell_open(&mut self, path: &Path) {
        let Some(document) = self.documents.get_mut(path) else {
            return;
        };
        document.version = 1;
        document.sent = Some(document.latest.clone());
        let text = document
            .latest
            .slice(0..document.latest.len())
            .into_owned();
        self.outgoing.push(transport::notification(
            protocol::DID_OPEN,
            protocol::did_open_params(path, 1, &text),
        ));
    }

    /// Answers a request of the server's: it asks things that a client with nothing to say answers with
    /// nothing, and what is not known is said not to be.
    fn respond(&mut self, id: &Value, method: &str, params: &Value) {
        let body = match method {
            "window/workDoneProgress/create"
            | "client/registerCapability"
            | "client/unregisterCapability" => transport::respond(id, Value::Null),
            // The names that were not known are known: the tokens are to be asked for again, which is for whoever shows them to do.
            protocol::SEMANTIC_TOKENS_REFRESH => {
                self.semantic_refreshes += 1;
                transport::respond(id, Value::Null)
            }
            "workspace/configuration" => {
                let items = params
                    .get("items")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                transport::respond(id, Value::Array(vec![Value::Null; items]))
            }
            _ => transport::respond_error(
                id,
                METHOD_NOT_FOUND,
                &format!("{method} is not supported by this client"),
            ),
        };
        self.outgoing.push(body);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "/project/src/main.rs";

    fn path() -> &'static Path {
        Path::new(FILE)
    }

    fn snapshot(text: &str) -> TextSnapshot {
        TextSnapshot::from_text(text)
    }

    /// What the session sent, as `(method, params)` for the notifications and requests and the id of requests.
    fn sent(session: &mut Session) -> Vec<(String, Value, Option<i64>)> {
        session
            .take_outgoing()
            .iter()
            .map(
                |body| match transport::parse(body).expect("the session sends messages") {
                    Incoming::Request { id, method, params } => (method, params, id.as_i64()),
                    Incoming::Notification { method, params } => (method, params, None),
                    other => panic!("a client sends requests and notifications: {other:?}"),
                },
            )
            .collect()
    }

    fn methods(sent: &[(String, Value, Option<i64>)]) -> Vec<&str> {
        sent.iter()
            .map(|(method, _, _)| method.as_str())
            .collect()
    }

    /// A session whose server answered `initialize` with what `capabilities` says.
    fn ready(capabilities: Value) -> Session {
        let mut session = Session::new(Path::new("/project"), 1);
        let first = sent(&mut session);
        let id = first[0].2.expect("initialize is a request");
        session.receive(Incoming::Response {
            id,
            result: Ok(json!({"capabilities": capabilities})),
        });
        let _ = sent(&mut session);
        session
    }

    fn incremental_utf8() -> Value {
        json!({"textDocumentSync": 2, "positionEncoding": "utf-8"})
    }

    fn publish(version: Option<i32>, diagnostics: Value) -> Incoming {
        let mut params = json!({"uri": protocol::uri_of(path()), "diagnostics": diagnostics});
        if let Some(version) = version {
            params["version"] = json!(version);
        }
        Incoming::Notification {
            method: protocol::PUBLISH_DIAGNOSTICS.into(),
            params,
        }
    }

    fn completing_utf8() -> Value {
        json!({"textDocumentSync": 2, "positionEncoding": "utf-8", "completionProvider": {"triggerCharacters": ["."]}})
    }

    fn defining(encoding: &str) -> Value {
        json!({"textDocumentSync": 2, "positionEncoding": encoding, "definitionProvider": true})
    }

    #[test]
    fn a_definition_is_asked_for_at_the_position_of_the_byte_in_the_text_the_server_has() {
        let mut session = ready(defining("utf-8"));
        // The accent is two bytes, and the server counts bytes.
        let text = snapshot("let \u{e9} = 1;\nuse_it(\u{e9});\n");
        session.open(path(), &text);
        let _ = sent(&mut session);
        let at = text
            .slice(0..text.len())
            .find("use_it")
            .unwrap()
            + 3;
        let id = session
            .definition(path(), at)
            .expect("a ready server that finds definitions is asked");
        let told = sent(&mut session);
        assert_eq!(methods(&told), [protocol::DEFINITION]);
        assert_eq!(
            (told[0].2, &told[0].1["position"]),
            (Some(id), &json!({"line": 1, "character": 3}))
        );
        assert_eq!(told[0].1["textDocument"]["uri"], protocol::uri_of(path()));
    }

    #[test]
    fn a_definition_is_not_asked_where_the_server_cannot_say() {
        let mut starting = Session::new(Path::new("/project"), 1);
        starting.open(path(), &snapshot("x"));
        assert_eq!(starting.definition(path(), 0), None, "not ready");
        let mut silent = ready(incremental_utf8());
        silent.open(path(), &snapshot("x"));
        assert_eq!(
            silent.definition(path(), 0),
            None,
            "it does not find definitions"
        );
        let mut other = ready(defining("utf-8"));
        assert_eq!(
            other.definition(path(), 0),
            None,
            "a file it was not told of"
        );
        let _ = sent(&mut other);
        assert!(other.take_outgoing().is_empty());
    }

    #[test]
    fn the_answer_to_a_request_for_a_definition_is_the_places_and_an_error_is_none() {
        let mut session = ready(defining("utf-8"));
        session.open(path(), &snapshot("x"));
        let (first, second) = (
            session.definition(path(), 0).unwrap(),
            session.definition(path(), 0).unwrap(),
        );
        let _ = sent(&mut session);
        let place = json!({"uri": "file:///project/src/lib.rs", "range": {"start": {"line": 4, "character": 7}, "end": {"line": 4, "character": 9}}});
        session.receive(Incoming::Response {
            id: first,
            result: Ok(place),
        });
        session.receive(Incoming::Response {
            id: second,
            result: Err(transport::ResponseError {
                code: -32803,
                message: "busy".into(),
            }),
        });
        let answers = session.take_places();
        assert_eq!(answers.len(), 2);
        assert_eq!(answers[0].0, first);
        let found = answers[0].1.as_ref().expect("a place");
        assert_eq!(
            (found[0].path.as_path(), found[0].at),
            (
                Path::new("/project/src/lib.rs"),
                position::Position {
                    line: 4,
                    character: 7
                }
            )
        );
        assert_eq!(answers[1], (second, None));
        assert!(session.take_places().is_empty(), "taken once");
        // What is not a place is no answer.
        let third = session.definition(path(), 0).unwrap();
        session.receive(Incoming::Response {
            id: third,
            result: Ok(json!("nope")),
        });
        assert_eq!(session.take_places(), vec![(third, None)]);
    }

    fn hovering(encoding: &str) -> Value {
        json!({"textDocumentSync": 2, "positionEncoding": encoding, "hoverProvider": true})
    }

    #[test]
    fn a_hover_is_asked_for_at_the_position_of_the_byte_in_the_text_the_server_has() {
        let mut session = ready(hovering("utf-8"));
        let text = snapshot("let \u{e9} = 1;\nuse_it(\u{e9});\n");
        session.open(path(), &text);
        let _ = sent(&mut session);
        let at = text
            .slice(0..text.len())
            .find("use_it")
            .unwrap()
            + 3;
        let id = session
            .hover(path(), at)
            .expect("a ready server that says what names are is asked");
        let told = sent(&mut session);
        assert_eq!(methods(&told), [protocol::HOVER]);
        assert_eq!(
            (told[0].2, &told[0].1["position"]),
            (Some(id), &json!({"line": 1, "character": 3}))
        );
    }

    #[test]
    fn a_hover_is_not_asked_where_the_server_cannot_say() {
        let mut starting = Session::new(Path::new("/project"), 1);
        starting.open(path(), &snapshot("x"));
        assert_eq!(starting.hover(path(), 0), None, "not ready");
        let mut silent = ready(incremental_utf8());
        silent.open(path(), &snapshot("x"));
        assert_eq!(
            silent.hover(path(), 0),
            None,
            "it does not say what names are"
        );
        let mut other = ready(hovering("utf-8"));
        assert_eq!(other.hover(path(), 0), None, "a file it was not told of");
    }

    #[test]
    fn the_answer_to_a_request_for_a_hover_is_the_words_in_bytes_and_none_where_there_are_none_or_it_was_an_error()
     {
        let mut session = ready(hovering("utf-8"));
        // The accent is two bytes, and the server counts bytes: the range of the name after it is in bytes of the text.
        session.open(path(), &snapshot("\u{e9} name\n"));
        let (first, second, third) = (
            session.hover(path(), 3).unwrap(),
            session.hover(path(), 3).unwrap(),
            session.hover(path(), 3).unwrap(),
        );
        let _ = sent(&mut session);
        let said = json!({"contents": {"kind": "markdown", "value": "`name`"}, "range": {"start": {"line": 0, "character": 3}, "end": {"line": 0, "character": 7}}});
        session.receive(Incoming::Response {
            id: first,
            result: Ok(said),
        });
        session.receive(Incoming::Response {
            id: second,
            result: Ok(Value::Null),
        });
        session.receive(Incoming::Response {
            id: third,
            result: Err(transport::ResponseError {
                code: -32803,
                message: "busy".into(),
            }),
        });
        let answers = session.take_hovers();
        assert_eq!(answers.len(), 3);
        assert_eq!(
            answers[0],
            (
                first,
                Some(Hover {
                    markdown: "`name`".into(),
                    range: Some(3..7)
                })
            )
        );
        assert_eq!(answers[1], (second, None), "nothing to say of it");
        assert_eq!(answers[2], (third, None), "an error is no answer");
        assert!(session.take_hovers().is_empty(), "taken once");
    }

    #[test]
    fn a_hover_that_is_cancelled_is_not_kept() {
        let mut session = ready(hovering("utf-8"));
        session.open(path(), &snapshot("x"));
        let id = session.hover(path(), 0).unwrap();
        let _ = sent(&mut session);
        session.cancel(id);
        assert_eq!(methods(&sent(&mut session)), [protocol::CANCEL_REQUEST]);
        session.receive(Incoming::Response {
            id,
            result: Ok(json!({"contents": "late"})),
        });
        assert!(session.take_hovers().is_empty());
    }

    #[test]
    fn a_request_by_method_is_sent_as_it_is_and_answered_with_what_the_server_said_and_none_for_an_error() {
        let mut session = ready(incremental_utf8());
        let (first, second) = (
            session.ask("textDocument/codeLens", json!({"textDocument": {"uri": "file:///a.rs"}})).expect("a ready server is asked"),
            session.ask("codeLens/resolve", json!({"data": 1})).expect("and again, with the first not answered"),
        );
        let told = sent(&mut session);
        assert_eq!(methods(&told), ["textDocument/codeLens", "codeLens/resolve"]);
        assert_eq!((told[0].2, told[1].2), (Some(first), Some(second)));
        assert_eq!(told[1].1, json!({"data": 1}), "the parameters are the caller's");
        // They are answered out of order, and by number.
        session.receive(Incoming::Response { id: second, result: Ok(json!({"command": null})) });
        session.receive(Incoming::Response {
            id: first,
            result: Err(transport::ResponseError { code: -32801, message: "content modified".into() }),
        });
        assert_eq!(session.take_replies(), vec![(second, Some(json!({"command": null}))), (first, None)]);
        assert!(session.take_replies().is_empty(), "taken once");
    }

    #[test]
    fn a_request_by_method_is_not_made_while_the_server_is_not_ready() {
        let mut starting = Session::new(Path::new("/project"), 1);
        assert_eq!(starting.ask("textDocument/codeLens", json!({})), None);
    }

    #[test]
    fn a_request_by_method_that_is_cancelled_is_not_kept() {
        let mut session = ready(incremental_utf8());
        let id = session.ask("codeLens/resolve", json!({})).unwrap();
        let _ = sent(&mut session);
        session.cancel(id);
        assert_eq!(methods(&sent(&mut session)), [protocol::CANCEL_REQUEST]);
        session.receive(Incoming::Response { id, result: Ok(json!(1)) });
        assert!(session.take_replies().is_empty());
    }

    #[test]
    fn a_definition_that_is_cancelled_is_not_kept() {
        let mut session = ready(defining("utf-8"));
        session.open(path(), &snapshot("x"));
        let id = session.definition(path(), 0).unwrap();
        let _ = sent(&mut session);
        session.cancel(id);
        assert_eq!(methods(&sent(&mut session)), [protocol::CANCEL_REQUEST]);
        session.receive(Incoming::Response {
            id,
            result: Ok(json!([])),
        });
        assert!(session.take_places().is_empty());
    }

    #[test]
    fn a_position_of_the_server_is_a_byte_of_a_text_as_the_server_counts() {
        // The accent is two bytes, and one unit of UTF-16 and of UTF-8 positions alike where it is the server's choice.
        let text = snapshot("\u{e9}b\nsecond\n");
        let utf8 = ready(defining("utf-8"));
        assert_eq!(
            utf8.byte_of(
                &text,
                position::Position {
                    line: 0,
                    character: 2
                }
            ),
            2,
            "after the accent, which is two bytes"
        );
        let utf16 = ready(defining("utf-16"));
        assert_eq!(
            utf16.byte_of(
                &text,
                position::Position {
                    line: 0,
                    character: 1
                }
            ),
            2,
            "after the accent, which is one unit"
        );
        assert_eq!(
            utf16.byte_of(
                &text,
                position::Position {
                    line: 1,
                    character: 3
                }
            ),
            4 + 3,
            "the second line begins at byte 4, after the accent, b and the line break"
        );
        assert_eq!(
            utf16.byte_of(
                &text,
                position::Position {
                    line: 99,
                    character: 0
                }
            ),
            text.len(),
            "past the end is the end"
        );
        // A server that has not answered yet is taken to count in UTF-16, which every server does.
        let starting = Session::new(Path::new("/project"), 1);
        assert_eq!(
            starting.byte_of(
                &text,
                position::Position {
                    line: 0,
                    character: 1
                }
            ),
            2
        );
    }

    #[test]
    fn a_request_for_completion_is_for_the_position_of_the_byte_in_the_text_the_server_has() {
        let mut session = ready(completing_utf8());
        // The accent is two bytes, and the server counts bytes.
        let text = snapshot("fn main() {\n    let \u{e9} = St\n}\n");
        session.open(path(), &text);
        let _ = sent(&mut session);
        let caret = text.slice(0..text.len()).find("St").unwrap() + 2;
        let id = session
            .complete(path(), caret)
            .expect("a ready server that completes is asked");
        let told = sent(&mut session);
        assert_eq!(methods(&told), [protocol::COMPLETION]);
        assert_eq!(told[0].2, Some(id));
        assert_eq!(
            told[0].1["position"],
            json!({"line": 1, "character": 15}),
            "four spaces, let, a space, two bytes, a space, =, a space, St"
        );
        assert_eq!(told[0].1["textDocument"]["uri"], protocol::uri_of(path()));
    }

    #[test]
    fn a_request_for_completion_is_after_the_change_it_is_made_for() {
        let mut session = ready(completing_utf8());
        session.open(path(), &snapshot("ab\n"));
        let _ = sent(&mut session);
        session.changed(path(), &snapshot("abc\n"));
        session.complete(path(), 3).expect("asked");
        let told = sent(&mut session);
        assert_eq!(
            methods(&told),
            [protocol::DID_CHANGE, protocol::COMPLETION],
            "the server has the text before it is asked about it"
        );
        assert_eq!(told[1].1["position"], json!({"line": 0, "character": 3}));
    }

    #[test]
    fn nothing_is_asked_where_the_server_cannot_say() {
        // Not ready: it has not answered initialize.
        let mut starting = Session::new(Path::new("/project"), 1);
        starting.open(path(), &snapshot("x"));
        assert_eq!(starting.complete(path(), 1), None);
        // Ready, and it does not complete.
        let mut silent = ready(incremental_utf8());
        silent.open(path(), &snapshot("x"));
        assert_eq!(silent.complete(path(), 1), None);
        // A file it was not told of.
        let mut other = ready(completing_utf8());
        assert_eq!(other.complete(path(), 0), None);
        let _ = sent(&mut other);
        assert!(other.take_outgoing().is_empty(), "and nothing was sent");
    }

    #[test]
    fn the_answer_to_a_request_for_completion_is_the_items_with_their_places_as_bytes() {
        let mut session = ready(completing_utf8());
        let text = snapshot("let x = Ve\n");
        session.open(path(), &text);
        let id = session.complete(path(), 10).expect("asked");
        let _ = sent(&mut session);
        let item = json!({"label": "Vec", "kind": 22, "textEdit": {"range": {"start": {"line": 0, "character": 8}, "end": {"line": 0, "character": 10}}, "newText": "Vec"}});
        session.receive(Incoming::Response {
            id,
            result: Ok(json!({"isIncomplete": false, "items": [item]})),
        });
        let answers = session.take_answers();
        assert_eq!(answers.len(), 1);
        assert_eq!(answers[0].0, id);
        let answer = answers[0].1.as_ref().expect("an answer");
        assert_eq!(
            (
                answer.items[0].label.as_str(),
                answer.items[0].edit.range.clone()
            ),
            ("Vec", Some(8..10))
        );
        assert!(session.take_answers().is_empty(), "an answer is taken once");
    }

    #[test]
    fn an_answer_that_is_an_error_or_not_made_as_it_should_be_is_no_answer() {
        let mut session = ready(completing_utf8());
        session.open(path(), &snapshot("x"));
        let first = session.complete(path(), 1).unwrap();
        let second = session.complete(path(), 1).unwrap();
        session.receive(Incoming::Response {
            id: first,
            result: Err(transport::ResponseError {
                code: -32803,
                message: "busy".into(),
            }),
        });
        session.receive(Incoming::Response {
            id: second,
            result: Ok(json!("not items")),
        });
        assert_eq!(
            session
                .take_answers()
                .iter()
                .map(|(id, answer)| (*id, answer.is_none()))
                .collect::<Vec<_>>(),
            [(first, true), (second, true)]
        );
    }

    #[test]
    fn the_rest_of_an_item_is_asked_for_with_what_the_item_says_and_the_answer_is_its_edits_as_bytes()
     {
        let mut session = ready(completing_utf8());
        let text = snapshot("mod a;\n\u{e9}Circ\n");
        session.open(path(), &text);
        let _ = sent(&mut session);
        let item = json!({"label": "Circle", "data": {"imports": [1]}});
        let id = session.resolve(path(), &item).expect("asked");
        let told = sent(&mut session);
        assert_eq!(methods(&told), [protocol::COMPLETION_RESOLVE]);
        assert_eq!((&told[0].1, told[0].2), (&item, Some(id)));
        let edit = |line: u32, from: u32, to: u32, text: &str| json!({"range": {"start": {"line": line, "character": from}, "end": {"line": line, "character": to}}, "newText": text});
        session.receive(Incoming::Response { id, result: Ok(json!({"label": "Circle", "additionalTextEdits": [edit(0, 0, 0, "use a::Circle;\n"), edit(1, 2, 4, "x")]})) });
        assert!(
            session.take_answers().is_empty(),
            "it is not an answer to a request for completion"
        );
        // The accent is two bytes and the server counts bytes: line 1 begins at byte 7.
        assert_eq!(
            session.take_resolved(),
            vec![(
                id,
                Some(vec![
                    (0..0, "use a::Circle;\n".to_owned()),
                    (9..11, "x".to_owned())
                ])
            )]
        );
        assert!(session.take_resolved().is_empty(), "taken once");
    }

    #[test]
    fn the_rest_of_an_item_is_not_asked_where_the_server_cannot_say_and_an_error_is_no_answer() {
        let item = json!({"label": "x"});
        let mut silent = ready(incremental_utf8());
        silent.open(path(), &snapshot("x"));
        assert_eq!(
            silent.resolve(path(), &item),
            None,
            "a server that does not complete"
        );
        let mut other = ready(completing_utf8());
        assert_eq!(
            other.resolve(path(), &item),
            None,
            "a file it was not told of"
        );
        let mut session = ready(completing_utf8());
        session.open(path(), &snapshot("x"));
        let id = session.resolve(path(), &item).unwrap();
        session.receive(Incoming::Response {
            id,
            result: Err(transport::ResponseError {
                code: -32800,
                message: "cancelled".into(),
            }),
        });
        assert_eq!(session.take_resolved(), vec![(id, None)]);
    }

    #[test]
    fn a_request_for_the_rest_of_an_item_can_be_cancelled_like_a_request_for_completion() {
        let mut session = ready(completing_utf8());
        session.open(path(), &snapshot("x"));
        let id = session
            .resolve(path(), &json!({"label": "x"}))
            .unwrap();
        let _ = sent(&mut session);
        session.cancel(id);
        assert_eq!(methods(&sent(&mut session)), [protocol::CANCEL_REQUEST]);
        session.receive(Incoming::Response {
            id,
            result: Ok(json!({"label": "x"})),
        });
        assert!(session.take_resolved().is_empty());
    }

    #[test]
    fn a_request_that_is_cancelled_tells_the_server_so_and_its_answer_is_not_kept() {
        let mut session = ready(completing_utf8());
        session.open(path(), &snapshot("x"));
        let id = session.complete(path(), 1).unwrap();
        let _ = sent(&mut session);
        session.cancel(id);
        let told = sent(&mut session);
        assert_eq!(methods(&told), [protocol::CANCEL_REQUEST]);
        assert_eq!(told[0].1, json!({"id": id}));
        session.receive(Incoming::Response {
            id,
            result: Ok(json!([{"label": "late"}])),
        });
        assert!(
            session.take_answers().is_empty(),
            "a late answer to what was cancelled"
        );
        session.cancel(id);
        assert!(
            session.take_outgoing().is_empty(),
            "and cancelling it again says nothing"
        );
    }

    fn diagnostic(start: (u32, u32), end: (u32, u32), severity: u32, message: &str) -> Value {
        json!({"range": {"start": {"line": start.0, "character": start.1}, "end": {"line": end.0, "character": end.1}}, "severity": severity, "message": message})
    }

    #[test]
    fn a_session_begins_by_asking_the_server_to_initialize_and_waits_for_the_answer() {
        let mut session = Session::new(Path::new("/project"), 77);
        assert_eq!(session.status(), &Status::Starting);
        let first = sent(&mut session);
        assert_eq!(methods(&first), ["initialize"]);
        assert_eq!(first[0].1["processId"], 77);
        assert_eq!(first[0].1["rootUri"], "file:///project");
        assert!(
            sent(&mut session).is_empty(),
            "what was sent is not sent again"
        );

        // Files opened before the server answers are not told to it yet.
        session.open(path(), &snapshot("fn main() {}\n"));
        assert!(sent(&mut session).is_empty());
        assert_eq!(session.version_of(path()), None);
    }

    #[test]
    fn once_the_server_has_answered_it_is_told_it_is_initialized_and_of_the_files_as_they_are_now()
    {
        let mut session = Session::new(Path::new("/project"), 1);
        let id = sent(&mut session)[0].2.unwrap();
        session.open(path(), &snapshot("fn a() {}\n"));
        // The text changes before the server answers: it is told the text as it is, in one open.
        session.changed(path(), &snapshot("fn a() {}\nfn b() {}\n"));
        session.changed(path(), &snapshot("fn a() {}\nfn b() {}\nfn c() {}\n"));
        assert!(sent(&mut session).is_empty());

        session.receive(Incoming::Response {
            id,
            result: Ok(json!({"capabilities": incremental_utf8()})),
        });

        assert_eq!(session.status(), &Status::Ready);
        let after = sent(&mut session);
        assert_eq!(methods(&after), ["initialized", "textDocument/didOpen"]);
        assert_eq!(
            after[1].1["textDocument"]["text"],
            "fn a() {}\nfn b() {}\nfn c() {}\n"
        );
        assert_eq!(after[1].1["textDocument"]["version"], 1);
        assert_eq!(
            after[1].1["textDocument"]["uri"],
            "file:///project/src/main.rs"
        );
        assert_eq!(session.version_of(path()), Some(1));
    }

    #[test]
    fn a_file_opened_when_the_server_is_ready_is_told_at_once() {
        let mut session = ready(incremental_utf8());
        session.open(path(), &snapshot("let x = 1;\n"));
        let told = sent(&mut session);
        assert_eq!(methods(&told), ["textDocument/didOpen"]);
        assert_eq!(told[0].1["textDocument"]["text"], "let x = 1;\n");
        // Opened twice, it is told once.
        session.open(path(), &snapshot("let x = 1;\n"));
        assert!(sent(&mut session).is_empty());
    }

    #[test]
    fn a_server_that_cannot_initialize_fails_the_session_and_nothing_more_is_sent() {
        let mut session = Session::new(Path::new("/project"), 1);
        let id = sent(&mut session)[0].2.unwrap();
        session.open(path(), &snapshot("x"));
        session.receive(Incoming::Response {
            id,
            result: Err(transport::ResponseError {
                code: -32603,
                message: "no cargo".into(),
            }),
        });
        assert_eq!(
            session.status(),
            &Status::Failed("the server could not initialize: no cargo".into())
        );
        session.open(Path::new("/project/other.rs"), &snapshot("y"));
        session.changed(path(), &snapshot("xx"));
        assert!(sent(&mut session).is_empty());

        // An answer that is not what initialize answers is a failure too.
        let mut session = Session::new(Path::new("/project"), 1);
        let id = sent(&mut session)[0].2.unwrap();
        session.receive(Incoming::Response {
            id,
            result: Ok(json!("what")),
        });
        assert!(matches!(session.status(), Status::Failed(_)));

        // And one that chooses positions the client did not offer.
        let mut session = Session::new(Path::new("/project"), 1);
        let id = sent(&mut session)[0].2.unwrap();
        session.receive(Incoming::Response {
            id,
            result: Ok(json!({"capabilities": {"positionEncoding": "utf-32"}})),
        });
        assert!(matches!(session.status(), Status::Failed(_)));
    }

    #[test]
    fn a_change_is_sent_as_the_part_that_changed_with_the_next_version() {
        let mut session = ready(incremental_utf8());
        let text = snapshot("fn main() {\n    let x = 1;\n}\n");
        session.open(path(), &text);
        let _ = sent(&mut session);

        // "1" becomes "42".
        let at = "fn main() {\n    let x = ".len();
        let edited = text.with_replaced(at..at + 1, "42");
        session.changed(path(), &edited);
        let told = sent(&mut session);
        assert_eq!(methods(&told), ["textDocument/didChange"]);
        assert_eq!(told[0].1["textDocument"]["version"], 2);
        assert_eq!(
            told[0].1["contentChanges"],
            json!([{"range": {"start": {"line": 1, "character": 12}, "end": {"line": 1, "character": 13}}, "text": "42"}])
        );
        assert_eq!(session.version_of(path()), Some(2));

        // The next change is made from the text the server has now.
        let again = edited.with_replaced(0..2, "pub fn");
        session.changed(path(), &again);
        let told = sent(&mut session);
        assert_eq!(told[0].1["textDocument"]["version"], 3);
        // The smallest change: "pub " put in before the "fn" that was there.
        assert_eq!(
            told[0].1["contentChanges"][0]["range"],
            json!({"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}})
        );
        assert_eq!(told[0].1["contentChanges"][0]["text"], "pub ");

        // Text that did not change is not sent.
        session.changed(path(), &again);
        assert!(sent(&mut session).is_empty());
        assert_eq!(session.version_of(path()), Some(3));
    }

    #[test]
    fn a_change_that_shares_the_first_byte_of_a_character_is_still_a_change_of_whole_characters() {
        // "é" and "è" are two bytes that start the same, and so are "\u{1f600}" and "\u{1f601}" in four.
        for (before, after) in [
            ("caf\u{e9}!", "caf\u{e8}!"),
            ("a\u{1f600}b", "a\u{1f601}b"),
            ("\u{4f60}", "\u{4f61}"),
        ] {
            let mut session = ready(incremental_utf8());
            let text = snapshot(before);
            session.open(path(), &text);
            let _ = sent(&mut session);
            session.changed(path(), &snapshot(after));
            let told = sent(&mut session);
            let change = &told[0].1["contentChanges"][0];
            let (start, end) = (
                change["range"]["start"]["character"]
                    .as_u64()
                    .unwrap() as usize,
                change["range"]["end"]["character"]
                    .as_u64()
                    .unwrap() as usize,
            );
            assert!(
                before.is_char_boundary(start) && before.is_char_boundary(end),
                "{before:?} changed at bytes {start}..{end}"
            );
            // What the server makes of it is the text after.
            let mut made = before.to_owned();
            made.replace_range(start..end, change["text"].as_str().unwrap());
            assert_eq!(made, after);
        }
    }

    #[test]
    fn many_edits_before_the_flush_are_one_change_from_the_text_the_server_has() {
        let mut session = ready(incremental_utf8());
        let text = snapshot("one\ntwo\nthree\n");
        session.open(path(), &text);
        let _ = sent(&mut session);
        // Three edits, and only the text now is given to the session: it is one change and one version.
        let edited = text
            .with_replaced(0..3, "ONE")
            .with_replaced(4..7, "TWO")
            .with_replaced(8..13, "THREE");
        session.changed(path(), &edited);
        let told = sent(&mut session);
        assert_eq!(told.len(), 1);
        assert_eq!(told[0].1["textDocument"]["version"], 2);
        assert_eq!(
            told[0].1["contentChanges"][0]["range"],
            json!({"start": {"line": 0, "character": 0}, "end": {"line": 2, "character": 5}})
        );
        assert_eq!(told[0].1["contentChanges"][0]["text"], "ONE\nTWO\nTHREE");
    }

    #[test]
    fn positions_of_a_change_are_counted_as_the_server_chose() {
        // A byte of text and a unit of UTF-16 are different after an emoji and an accent.
        let text = snapshot("a\u{1f600}\u{e9}z\n");
        let at = "a\u{1f600}\u{e9}".len();
        let range = |capabilities: Value| {
            let mut session = ready(capabilities);
            session.open(path(), &text);
            let _ = sent(&mut session);
            session.changed(path(), &text.with_replaced(at..at + 1, "Z"));
            sent(&mut session)[0].1["contentChanges"][0]["range"].clone()
        };
        assert_eq!(
            range(json!({"textDocumentSync": 2, "positionEncoding": "utf-8"}))["start"]["character"],
            1 + 4 + 2
        );
        assert_eq!(
            range(json!({"textDocumentSync": 2, "positionEncoding": "utf-16"}))["start"]["character"],
            1 + 2 + 1
        );
        assert_eq!(
            range(json!({"textDocumentSync": 2}))["start"]["character"],
            1 + 2 + 1,
            "UTF-16 when the server says nothing"
        );
    }

    #[test]
    fn a_server_that_wants_the_whole_text_gets_it_and_one_that_wants_none_gets_none() {
        let mut session = ready(json!({"textDocumentSync": 1}));
        let text = snapshot("a\nb\n");
        session.open(path(), &text);
        let _ = sent(&mut session);
        session.changed(path(), &text.with_replaced(0..1, "A"));
        let told = sent(&mut session);
        assert_eq!(told[0].1["contentChanges"], json!([{"text": "A\nb\n"}]));
        assert_eq!(told[0].1["textDocument"]["version"], 2);

        let mut session = ready(json!({"textDocumentSync": 0}));
        session.open(path(), &text);
        let _ = sent(&mut session);
        session.changed(path(), &text.with_replaced(0..1, "A"));
        assert!(sent(&mut session).is_empty());
    }

    #[test]
    fn closing_a_file_tells_the_server_and_forgets_what_it_said_of_it() {
        let mut session = ready(incremental_utf8());
        let text = snapshot("let x = 1;\n");
        session.open(path(), &text);
        let _ = sent(&mut session);
        session.receive(publish(
            Some(1),
            json!([diagnostic((0, 4), (0, 5), 1, "bad")]),
        ));
        assert_eq!(session.diagnostics(path(), &text).len(), 1);

        session.close(path());
        let told = sent(&mut session);
        assert_eq!(methods(&told), ["textDocument/didClose"]);
        assert!(session.diagnostics(path(), &text).is_empty());
        assert_eq!(session.version_of(path()), None);
        session.close(path());
        assert!(
            sent(&mut session).is_empty(),
            "a file that is not open is not closed"
        );
        // Closed before the server was ready, it is never told.
        let mut early = Session::new(Path::new("/project"), 1);
        let id = sent(&mut early)[0].2.unwrap();
        early.open(path(), &text);
        early.close(path());
        early.receive(Incoming::Response {
            id,
            result: Ok(json!({"capabilities": incremental_utf8()})),
        });
        assert_eq!(methods(&sent(&mut early)), ["initialized"]);
    }

    #[test]
    fn diagnostics_are_the_bytes_the_server_pointed_at_whatever_it_counted_in() {
        let text = snapshot("let \u{1f600} = oops;\nsecond line\n");
        let oops = "let \u{1f600} = ".len();
        for (encoding, start, end) in [
            ("utf-8", oops as u32, oops as u32 + 4),
            ("utf-16", 4 + 2 + 3, 4 + 2 + 3 + 4),
        ] {
            let mut session = ready(json!({"textDocumentSync": 2, "positionEncoding": encoding}));
            session.open(path(), &text);
            let _ = sent(&mut session);
            session.receive(publish(
                Some(1),
                json!([diagnostic((0, start), (0, end), 1, "unresolved")]),
            ));
            let found = session.diagnostics(path(), &text);
            assert_eq!(
                found,
                vec![Diagnostic {
                    range: oops..oops + 4,
                    severity: Severity::Error,
                    message: "unresolved".into(),
                    code: None
                }],
                "{encoding}"
            );
        }
    }

    #[test]
    fn diagnostics_that_are_out_of_range_or_empty_or_backwards_are_made_to_make_sense() {
        let text = snapshot("short\n");
        let mut session = ready(incremental_utf8());
        session.open(path(), &text);
        let _ = sent(&mut session);
        session.receive(publish(
            Some(1),
            json!([
                diagnostic((0, 2), (0, 2), 2, "a point"),
                diagnostic((0, 3), (9, 9), 1, "past the end"),
                diagnostic((0, 4), (0, 1), 3, "backwards"),
            ]),
        ));
        let found = session.diagnostics(path(), &text);
        assert_eq!(found[0].range, 2..2);
        assert_eq!(found[1].range, 3..text.len());
        assert_eq!(found[2].range, 4..4, "the end is not before the start");
        assert!(
            found
                .iter()
                .all(|d| d.range.start <= d.range.end && d.range.end <= text.len())
        );
    }

    #[test]
    fn the_diagnostics_that_are_current_are_given_with_the_text_the_server_has_and_are_not_given_while_the_server_is_behind() {
        let mut session = ready(incremental_utf8());
        let text = snapshot("let x = 1;\n");
        assert!(session.current_diagnostics(path()).is_none(), "a file that is not open");
        session.open(path(), &text);
        let _ = sent(&mut session);
        let (none, _) = session.current_diagnostics(path()).expect("open and told: nothing is published, and that is nothing wrong");
        assert!(none.is_empty());
        session.receive(publish(Some(1), json!([diagnostic((0, 4), (0, 5), 1, "old")])));
        let (found, of) = session.current_diagnostics(path()).expect("current");
        assert_eq!(found.iter().map(|found| found.range.clone()).collect::<Vec<_>>(), [4..5]);
        assert_eq!(of.len(), text.len(), "and the text that its bytes are of");
        // The text changes and the server is told: what was published is not about this text, and is not given; the next is.
        let edited = text.with_replaced(0..0, "// c\n");
        session.changed(path(), &edited);
        let _ = sent(&mut session);
        assert!(session.current_diagnostics(path()).is_none(), "a version behind");
        session.receive(publish(Some(2), json!([diagnostic((1, 4), (1, 5), 1, "new")])));
        let (found, of) = session.current_diagnostics(path()).expect("current again");
        assert_eq!((found[0].range.clone(), of.len()), (9..10, edited.len()));
        // A publication that says no version is taken for the text as it is, and an empty one says that there is nothing wrong.
        session.receive(publish(None, json!([])));
        assert!(session.current_diagnostics(path()).expect("current").0.is_empty());
    }

    #[test]
    fn diagnostics_of_a_text_that_has_changed_since_are_not_given_and_the_next_ones_are() {
        let mut session = ready(incremental_utf8());
        let text = snapshot("let x = 1;\n");
        session.open(path(), &text);
        let _ = sent(&mut session);
        session.receive(publish(
            Some(1),
            json!([diagnostic((0, 4), (0, 5), 1, "old")]),
        ));
        assert_eq!(session.diagnostics(path(), &text).len(), 1);

        // The text changes: what was published for the version before is not about this text.
        let edited = text.with_replaced(0..0, "// c\n");
        session.changed(path(), &edited);
        let _ = sent(&mut session);
        assert!(
            session.diagnostics(path(), &edited).is_empty(),
            "a version behind"
        );
        session.receive(publish(
            Some(1),
            json!([diagnostic((0, 4), (0, 5), 1, "still old")]),
        ));
        assert!(
            session.diagnostics(path(), &edited).is_empty(),
            "a late publication for the old version"
        );
        session.receive(publish(
            Some(2),
            json!([diagnostic((1, 4), (1, 5), 1, "new")]),
        ));
        let found = session.diagnostics(path(), &edited);
        assert_eq!(
            found
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>(),
            ["new"]
        );
        // A publication that says no version is taken for the text as it is.
        session.receive(publish(
            None,
            json!([diagnostic((1, 0), (1, 3), 2, "no version")]),
        ));
        assert_eq!(
            session.diagnostics(path(), &edited)[0].message,
            "no version"
        );
        // An empty one clears them.
        session.receive(publish(Some(2), json!([])));
        assert!(session.diagnostics(path(), &edited).is_empty());
    }

    #[test]
    fn diagnostics_of_a_file_that_is_not_open_are_not_kept() {
        let mut session = ready(incremental_utf8());
        session.receive(publish(
            Some(1),
            json!([diagnostic((0, 0), (0, 1), 1, "stray")]),
        ));
        session.open(path(), &snapshot("x\n"));
        assert!(
            session
                .diagnostics(path(), &snapshot("x\n"))
                .is_empty()
        );
    }

    fn server_status(quiescent: bool, message: Option<&str>) -> Incoming {
        let mut params = json!({"health": "ok", "quiescent": quiescent});
        if let Some(message) = message {
            params["message"] = json!(message);
        }
        Incoming::Notification {
            method: protocol::SERVER_STATUS.into(),
            params,
        }
    }

    #[test]
    fn a_job_that_has_reached_a_hundred_per_cent_is_done_whether_or_not_the_server_says_so() {
        let mut session = ready(incremental_utf8());
        session.receive(progress(
            json!("roots"),
            json!({"kind": "begin", "title": "Roots Scanned", "percentage": 0}),
        ));
        session.receive(progress(
            json!("roots"),
            json!({"kind": "report", "percentage": 99}),
        ));
        assert_eq!(
            session.progress_label().as_deref(),
            Some("Roots Scanned 99%")
        );
        // The server reports it at a hundred and never ends it.
        session.receive(progress(
            json!("roots"),
            json!({"kind": "report", "percentage": 100, "message": "365/365"}),
        ));
        assert_eq!(session.progress_label(), None);
        // A job of another kind that began after it still shows while it goes, and the one that is done does not come back.
        session.receive(progress(
            json!("index"),
            json!({"kind": "begin", "title": "Indexing", "percentage": 3}),
        ));
        assert_eq!(session.progress_label().as_deref(), Some("Indexing 3%"));
        session.receive(progress(
            json!("index"),
            json!({"kind": "report", "percentage": 100}),
        ));
        assert_eq!(session.progress_label(), None);
        // A job that begins at a hundred is not one that shows.
        session.receive(progress(
            json!("instant"),
            json!({"kind": "begin", "title": "Done already", "percentage": 100}),
        ));
        assert_eq!(session.progress_label(), None);
    }

    #[test]
    fn a_server_that_says_it_is_not_idle_is_busy_with_what_it_says_when_it_names_no_job() {
        let mut session = ready(incremental_utf8());
        assert_eq!(
            session.progress_label(),
            None,
            "idle until it says otherwise"
        );
        let idle = session.revision();
        session.receive(server_status(true, None));
        assert_eq!(session.revision(), idle, "idle again is not a change");

        session.receive(server_status(false, Some("Loading")));
        assert_eq!(session.progress_label().as_deref(), Some("Loading"));
        let busy = session.revision();
        session.receive(server_status(false, Some("Loading")));
        assert_eq!(session.revision(), busy, "the same, said again");
        session.receive(server_status(false, None));
        assert_eq!(
            session.progress_label().as_deref(),
            Some("Working"),
            "busy and saying nothing of what"
        );
        // A job it names is what is shown, and when it ends the server is still not idle.
        session.receive(progress(
            json!("index"),
            json!({"kind": "begin", "title": "Indexing", "percentage": 10}),
        ));
        assert_eq!(session.progress_label().as_deref(), Some("Indexing 10%"));
        session.receive(progress(json!("index"), json!({"kind": "end"})));
        assert_eq!(session.progress_label().as_deref(), Some("Working"));
        session.receive(server_status(true, None));
        assert_eq!(session.progress_label(), None);
    }

    #[test]
    fn only_what_a_healthy_server_says_it_is_doing_is_shown_as_what_it_is_doing_and_only_its_first_line()
     {
        let mut session = ready(incremental_utf8());
        // A server that is not well says why, which is not what it is doing, and is not shown here.
        session.receive(Incoming::Notification {
            method: protocol::SERVER_STATUS.into(),
            params: json!({"health": "warning", "quiescent": false, "message": "invalid config values:\ncargo/targetDir: unexpected field\n;"}),
        });
        assert_eq!(session.progress_label().as_deref(), Some("Working"));
        session.receive(Incoming::Notification {
            method: protocol::SERVER_STATUS.into(),
            params: json!({"health": "error", "quiescent": false, "message": "it broke"}),
        });
        assert_eq!(session.progress_label().as_deref(), Some("Working"));
        // A healthy one that says several lines is shown by the first.
        session.receive(server_status(
            false,
            Some("Loading the workspace\nthis part of the project\nand this one"),
        ));
        assert_eq!(
            session.progress_label().as_deref(),
            Some("Loading the workspace")
        );
        session.receive(server_status(
            false,
            Some("  \n second line only after a blank first  "),
        ));
        assert_eq!(
            session.progress_label().as_deref(),
            Some("Working"),
            "nothing on the first line"
        );
    }

    #[test]
    fn a_server_that_is_not_idle_and_is_gone_is_not_busy_and_what_is_not_a_status_is_passed_over() {
        let mut session = ready(incremental_utf8());
        session.receive(server_status(false, Some("Loading")));
        let before = session.revision();
        session.receive(Incoming::Notification {
            method: protocol::SERVER_STATUS.into(),
            params: json!({"nonsense": 1}),
        });
        assert_eq!(session.revision(), before);
        assert!(session.progress_label().is_some());
        session.server_gone("it exited");
        assert_eq!(session.progress_label(), None);
    }

    fn progress(token: Value, value: Value) -> Incoming {
        Incoming::Notification {
            method: protocol::PROGRESS.into(),
            params: json!({"token": token, "value": value}),
        }
    }

    #[test]
    fn what_the_server_is_busy_with_is_known_while_it_is_and_the_latest_job_is_the_one_shown() {
        let mut session = ready(incremental_utf8());
        assert_eq!(session.progress_label(), None);
        session.receive(progress(
            json!("index"),
            json!({"kind": "begin", "title": "Indexing", "percentage": 0}),
        ));
        assert_eq!(session.progress_label().as_deref(), Some("Indexing 0%"));
        session.receive(progress(
            json!("index"),
            json!({"kind": "report", "percentage": 42, "message": "5/12 (a_crate)"}),
        ));
        assert_eq!(session.progress_label().as_deref(), Some("Indexing 42%"));
        // A second job is the one that is shown, and the first is shown again when it ends.
        session.receive(progress(
            json!(7),
            json!({"kind": "begin", "title": "Fetching", "message": "metadata"}),
        ));
        assert_eq!(
            session.progress_label().as_deref(),
            Some("Fetching: metadata"),
            "no percentage, so what it says"
        );
        session.receive(progress(
            json!(7),
            json!({"kind": "report", "message": "rustc sources"}),
        ));
        assert_eq!(
            session.progress_label().as_deref(),
            Some("Fetching: rustc sources")
        );
        session.receive(progress(json!(7), json!({"kind": "end"})));
        assert_eq!(session.progress_label().as_deref(), Some("Indexing 42%"));
        session.receive(progress(
            json!("index"),
            json!({"kind": "end", "message": "done"}),
        ));
        assert_eq!(session.progress_label(), None);
    }

    #[test]
    fn a_report_or_an_end_for_a_job_that_was_not_begun_is_passed_over() {
        let mut session = ready(incremental_utf8());
        let before = session.revision();
        session.receive(progress(
            json!("ghost"),
            json!({"kind": "report", "percentage": 50}),
        ));
        session.receive(progress(json!("ghost"), json!({"kind": "end"})));
        session.receive(Incoming::Notification {
            method: protocol::PROGRESS.into(),
            params: json!({"nonsense": true}),
        });
        assert_eq!(session.progress_label(), None);
        assert_eq!(session.revision(), before, "nothing to show");
    }

    #[test]
    fn the_revision_moves_with_what_is_shown_of_a_job_and_not_when_that_is_the_same() {
        let mut session = ready(incremental_utf8());
        let start = session.revision();
        session.receive(progress(
            json!("a"),
            json!({"kind": "begin", "title": "Indexing", "percentage": 10}),
        ));
        let begun = session.revision();
        assert_ne!(begun, start);
        session.receive(progress(json!("a"), json!({"kind": "report", "percentage": 10, "message": "another message that is not shown"})));
        assert_eq!(session.revision(), begun, "the same label");
        session.receive(progress(
            json!("a"),
            json!({"kind": "report", "percentage": 11}),
        ));
        assert_ne!(session.revision(), begun);
        let reported = session.revision();
        session.receive(progress(json!("a"), json!({"kind": "end"})));
        assert_ne!(session.revision(), reported);
    }

    #[test]
    fn a_server_that_is_gone_or_stopped_is_not_busy() {
        let mut session = ready(incremental_utf8());
        session.receive(progress(
            json!("a"),
            json!({"kind": "begin", "title": "Indexing"}),
        ));
        assert!(session.progress_label().is_some());
        session.server_gone("it exited");
        assert_eq!(session.progress_label(), None);

        let mut session = ready(incremental_utf8());
        session.receive(progress(
            json!("a"),
            json!({"kind": "begin", "title": "Indexing"}),
        ));
        session.shut_down();
        let id = sent(&mut session)[0].2.unwrap();
        session.receive(Incoming::Response {
            id,
            result: Ok(Value::Null),
        });
        assert_eq!(session.progress_label(), None);
    }

    #[test]
    fn the_revision_moves_when_what_there_is_to_show_changes_and_not_when_it_does_not() {
        let mut session = Session::new(Path::new("/project"), 1);
        let id = sent(&mut session)[0].2.unwrap();
        let starting = session.revision();
        session.receive(Incoming::Notification {
            method: "window/logMessage".into(),
            params: json!({}),
        });
        assert_eq!(session.revision(), starting, "a message that shows nothing");
        session.receive(Incoming::Response {
            id,
            result: Ok(json!({"capabilities": incremental_utf8()})),
        });
        let ready = session.revision();
        assert_ne!(ready, starting, "ready is a change");
        session.open(path(), &snapshot("x\n"));
        let _ = sent(&mut session);
        session.receive(publish(
            Some(1),
            json!([diagnostic((0, 0), (0, 1), 1, "bad")]),
        ));
        let published = session.revision();
        assert_ne!(published, ready, "diagnostics are a change");
        session.receive(Incoming::Notification {
            method: "textDocument/publishDiagnostics".into(),
            params: json!({"uri": "file:///elsewhere.rs", "diagnostics": []}),
        });
        assert_eq!(
            session.revision(),
            published,
            "diagnostics of a file that is not open"
        );
        session.changed(path(), &snapshot("xy\n"));
        assert_ne!(
            session.revision(),
            published,
            "what was published is no longer about this text"
        );
        let before_gone = session.revision();
        session.server_gone("gone");
        assert_ne!(session.revision(), before_gone);
        let after_gone = session.revision();
        session.server_gone("gone again");
        assert_eq!(session.revision(), after_gone, "already failed");
    }

    #[test]
    fn the_errors_and_warnings_of_a_file_are_counted_for_the_text_that_it_is() {
        let mut session = ready(incremental_utf8());
        session.open(path(), &snapshot("a\nb\n"));
        let _ = sent(&mut session);
        assert_eq!(session.counts_of(path()), (0, 0));
        session.receive(publish(
            Some(1),
            json!([
                diagnostic((0, 0), (0, 1), 1, "e1"),
                diagnostic((1, 0), (1, 1), 1, "e2"),
                diagnostic((1, 0), (1, 1), 2, "w"),
                diagnostic((1, 0), (1, 1), 4, "h")
            ]),
        ));
        assert_eq!(session.counts_of(path()), (2, 1));
        session.changed(path(), &snapshot("a\nb\nc\n"));
        assert_eq!(session.counts_of(path()), (0, 0), "of an older text");
        assert_eq!(session.counts_of(Path::new("/not/open.rs")), (0, 0));
    }

    #[test]
    fn a_saved_file_is_told_to_the_server_that_has_it_open_and_one_that_is_not_ready_or_not_open_is_not_told() {
        let mut session = ready(incremental_utf8());
        session.open(Path::new("/project/a.rs"), &TextSnapshot::from_text("fn a() {}\n"));
        let _ = sent(&mut session);
        session.saved(Path::new("/project/a.rs"));
        let told = sent(&mut session);
        assert_eq!(methods(&told), ["textDocument/didSave"]);
        assert_eq!(told[0].1, json!({"textDocument": {"uri": "file:///project/a.rs"}}));
        session.saved(Path::new("/project/not_open.rs"));
        assert!(sent(&mut session).is_empty(), "a file the server was not told of");
        let mut starting = Session::new(Path::new("/project"), 1);
        let _ = sent(&mut starting);
        starting.open(Path::new("/project/a.rs"), &TextSnapshot::from_text("x"));
        starting.saved(Path::new("/project/a.rs"));
        assert!(sent(&mut starting).is_empty(), "a server that is not ready");
    }

    #[test]
    fn the_legend_of_a_server_that_has_tokens_is_kept_and_one_that_has_none_has_no_use_of_them() {
        let with = ready(json!({
            "textDocumentSync": 2,
            "semanticTokensProvider": {"legend": {"tokenTypes": ["variable"], "tokenModifiers": ["mutable"]}, "full": true},
        }));
        assert!(with.can_semantic());
        let legend = with.legend().expect("a legend");
        assert_eq!(legend.type_name(0), Some("variable"));
        let without = ready(incremental_utf8());
        assert!(!without.can_semantic());
        assert!(without.legend().is_none());
    }

    #[test]
    fn a_server_that_says_its_tokens_are_not_as_they_were_is_answered_and_it_is_counted_once_for_each() {
        let mut session = ready(incremental_utf8());
        assert_eq!(session.take_semantic_refreshes(), 0);
        for id in [json!(1), json!("b")] {
            session.receive(Incoming::Request {
                id: id.clone(),
                method: "workspace/semanticTokens/refresh".into(),
                params: Value::Null,
            });
            let bodies = session.take_outgoing();
            assert_eq!(bodies.len(), 1, "it is answered");
            assert_eq!(serde_json::from_slice::<Value>(&bodies[0]).unwrap(), json!({"jsonrpc": "2.0", "id": id, "result": null}));
        }
        assert_eq!(session.take_semantic_refreshes(), 2);
        assert_eq!(session.take_semantic_refreshes(), 0, "and not again");
    }

    #[test]
    fn what_the_server_asks_of_the_client_is_answered() {
        let mut session = ready(incremental_utf8());
        let ask = |session: &mut Session, id: Value, method: &str, params: Value| {
            session.receive(Incoming::Request {
                id,
                method: method.into(),
                params,
            });
            let bodies = session.take_outgoing();
            assert_eq!(bodies.len(), 1, "{method} is answered");
            serde_json::from_slice::<Value>(&bodies[0]).unwrap()
        };
        assert_eq!(
            ask(
                &mut session,
                json!(1),
                "window/workDoneProgress/create",
                json!({"token": "x"})
            ),
            json!({"jsonrpc": "2.0", "id": 1, "result": null})
        );
        assert_eq!(
            ask(
                &mut session,
                json!("a"),
                "client/registerCapability",
                json!({"registrations": []})
            ),
            json!({"jsonrpc": "2.0", "id": "a", "result": null})
        );
        assert_eq!(
            ask(
                &mut session,
                json!(3),
                "client/unregisterCapability",
                json!({})
            ),
            json!({"jsonrpc": "2.0", "id": 3, "result": null})
        );
        assert_eq!(
            ask(
                &mut session,
                json!(4),
                "workspace/configuration",
                json!({"items": [{"section": "a"}, {"section": "b"}]})
            ),
            json!({"jsonrpc": "2.0", "id": 4, "result": [null, null]}),
            "one answer for each thing asked"
        );
        assert_eq!(
            ask(&mut session, json!(5), "workspace/configuration", json!({})),
            json!({"jsonrpc": "2.0", "id": 5, "result": []})
        );
        let unknown = ask(
            &mut session,
            json!(6),
            "textDocument/somethingNew",
            json!({}),
        );
        assert_eq!(unknown["error"]["code"], -32601);
        assert_eq!(unknown["id"], 6);
        // Notifications that are not understood, and answers to requests that were not made, are passed over.
        session.receive(Incoming::Notification {
            method: "window/logMessage".into(),
            params: json!({}),
        });
        session.receive(Incoming::Response {
            id: 999,
            result: Ok(Value::Null),
        });
        assert!(session.take_outgoing().is_empty());
        assert_eq!(session.status(), &Status::Ready);
    }

    #[test]
    fn shutting_down_asks_the_server_and_exits_when_it_has_answered() {
        let mut session = ready(incremental_utf8());
        session.shut_down();
        let told = sent(&mut session);
        assert_eq!(methods(&told), ["shutdown"]);
        let id = told[0].2.unwrap();
        assert_ne!(
            session.status(),
            &Status::Stopped,
            "not until it has answered"
        );
        session.receive(Incoming::Response {
            id,
            result: Ok(Value::Null),
        });
        assert_eq!(methods(&sent(&mut session)), ["exit"]);
        assert_eq!(session.status(), &Status::Stopped);
        // Files opened after that are not told to anyone.
        session.open(path(), &snapshot("x"));
        assert!(sent(&mut session).is_empty());
    }

    #[test]
    fn a_server_that_goes_is_a_failure_with_its_reason_and_a_stopped_one_is_not() {
        let mut session = ready(incremental_utf8());
        session.server_gone("the server exited with status 101");
        assert_eq!(
            session.status(),
            &Status::Failed("the server exited with status 101".into())
        );
        session.open(path(), &snapshot("x"));
        assert!(sent(&mut session).is_empty());

        let mut session = ready(incremental_utf8());
        session.shut_down();
        let id = sent(&mut session)[0].2.unwrap();
        session.receive(Incoming::Response {
            id,
            result: Ok(Value::Null),
        });
        session.server_gone("the server exited with status 0");
        assert_eq!(session.status(), &Status::Stopped, "it was asked to go");
    }
}
