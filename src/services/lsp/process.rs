//! A server program that is running: the pipes of its process, read and written by threads of their own,
//! so that nothing waits for the server on the thread of the interface. What it says is left in a
//! mailbox, which wakes the task that waits on it.

use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::thread;
use std::time::{Duration, Instant};

use super::transport::{self, Incoming};

/// Things left by one thread for another: any thread puts, and one that waits is woken.
pub struct Mailbox<T> {
    queue: Mutex<VecDeque<T>>,
    waker: Mutex<Option<Waker>>,
}

impl<T> Mailbox<T> {
    pub fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            waker: Mutex::new(None),
        }
    }

    /// Leaves `item`, and wakes whoever waits.
    pub fn push(&self, item: T) {
        self.queue
            .lock()
            .expect("no thread panics holding the queue")
            .push_back(item);
        let waker = self
            .waker
            .lock()
            .expect("no thread panics holding the waker")
            .take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// Takes everything that is left, in the order it was left.
    pub fn drain(&self) -> Vec<T> {
        self.queue
            .lock()
            .expect("no thread panics holding the queue")
            .drain(..)
            .collect()
    }

    fn is_empty(&self) -> bool {
        self.queue
            .lock()
            .expect("no thread panics holding the queue")
            .is_empty()
    }

    /// Ready when there is something to take, and wakes the task that polls it when there is.
    pub fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<()> {
        if !self.is_empty() {
            return Poll::Ready(());
        }
        *self
            .waker
            .lock()
            .expect("no thread panics holding the waker") = Some(cx.waker().clone());
        // Looked at again: something left between the first look and the waker being kept would
        // have found no waker to wake.
        if self.is_empty() {
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
}

/// What a server did.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// It said something.
    Message(Incoming),
    /// It said something that is not a message, and why it is not.
    Malformed(String),
    /// It is gone, or it cannot be heard from, and why.
    Gone(String),
}

/// How many of the last lines a server wrote on its error output are kept, to say with why it went.
const KEPT_ERROR_LINES: usize = 5;

/// A server that is running. The process is stopped when this is dropped.
pub struct Server {
    id: u32,
    child: Arc<Mutex<Child>>,
    outgoing: Sender<Vec<u8>>,
    mailbox: Arc<Mailbox<Event>>,
}

impl Server {
    /// Runs `program` with `args` in the folder `cwd`, with its input and output the pipes of the
    /// protocol, or says why it cannot be run.
    pub fn spawn(program: &str, args: &[String], cwd: &Path) -> io::Result<Self> {
        let mut child = Command::new(program)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let id = child.id();
        let stdin = child
            .stdin
            .take()
            .expect("the input was asked to be a pipe");
        let stdout = child
            .stdout
            .take()
            .expect("the output was asked to be a pipe");
        let stderr = child
            .stderr
            .take()
            .expect("the error output was asked to be a pipe");
        let child = Arc::new(Mutex::new(child));
        let mailbox = Arc::new(Mailbox::new());

        // What it writes on its error output is kept, a few lines of it, for the reason it goes.
        let tail: Arc<Mutex<VecDeque<String>>> = Arc::default();
        let tail_done = Arc::new(AtomicBool::new(false));
        {
            let (tail, done) = (Arc::clone(&tail), Arc::clone(&tail_done));
            thread::Builder::new()
                .name("refractor-lsp-stderr".into())
                .spawn(move || {
                    for line in BufReader::new(stderr)
                        .lines()
                        .map_while(Result::ok)
                    {
                        let mut tail = tail
                            .lock()
                            .expect("no thread panics holding the lines");
                        if tail.len() == KEPT_ERROR_LINES {
                            tail.pop_front();
                        }
                        tail.push_back(line);
                    }
                    done.store(true, Ordering::Release);
                })?;
        }

        let (outgoing, to_write) = mpsc::channel::<Vec<u8>>();
        thread::Builder::new()
            .name("refractor-lsp-writer".into())
            .spawn(move || {
                let mut stdin = stdin;
                // Ends when the server is dropped, which closes the pipe, or when the pipe is broken, which
                // the reader says when it finds the server gone.
                while let Ok(body) = to_write.recv() {
                    if transport::write_message(&mut stdin, &body).is_err() {
                        break;
                    }
                }
            })?;

        {
            let (mailbox, child) = (Arc::clone(&mailbox), Arc::clone(&child));
            thread::Builder::new()
                .name("refractor-lsp-reader".into())
                .spawn(move || {
                    let mut input = BufReader::new(stdout);
                    let reason = loop {
                        match transport::read_message(&mut input) {
                            Ok(Some(body)) => match transport::parse(&body) {
                                Ok(message) => mailbox.push(Event::Message(message)),
                                Err(why) => mailbox.push(Event::Malformed(why)),
                            },
                            Ok(None) => break "the server closed its output".to_owned(),
                            Err(error) => {
                                break format!("the server's output could not be read: {error}");
                            }
                        }
                    };
                    let status = exit_status(&child, Duration::from_secs(2));
                    // Its last words are the part of its error output that has been read: a little is waited
                    // for, but not for a program it started that still holds the pipe.
                    let waited = Instant::now();
                    while !tail_done.load(Ordering::Acquire)
                        && waited.elapsed() < Duration::from_millis(300)
                    {
                        thread::sleep(Duration::from_millis(5));
                    }
                    let last = tail
                        .lock()
                        .expect("no thread panics holding the lines")
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(" / ");
                    let mut why = reason;
                    if let Some(status) = status {
                        why = format!("{why} ({status})");
                    }
                    if !last.is_empty() {
                        why = format!("{why}: {last}");
                    }
                    mailbox.push(Event::Gone(why));
                })?;
        }
        Ok(Self {
            id,
            child,
            outgoing,
            mailbox,
        })
    }

    /// The process's number.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Leaves the body of a message to be written to the server.
    pub fn send(&self, body: Vec<u8>) {
        // A server that is gone has nobody to write to, which is said by the reader and not here.
        let _ = self.outgoing.send(body);
    }

    /// What was left by the server, which can be waited for.
    pub fn mailbox(&self) -> Arc<Mailbox<Event>> {
        Arc::clone(&self.mailbox)
    }

    /// Stops the process.
    pub fn kill(&self) {
        let _ = self
            .child
            .lock()
            .expect("no thread panics holding the process")
            .kill();
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.kill();
    }
}

/// How the process exited, if it does in `time`.
fn exit_status(child: &Mutex<Child>, time: Duration) -> Option<String> {
    let deadline = Instant::now() + time;
    loop {
        match child
            .lock()
            .expect("no thread panics holding the process")
            .try_wait()
        {
            Ok(Some(status)) => return Some(status.to_string()),
            Ok(None) if Instant::now() < deadline => {}
            _ => return None,
        }
        thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::lsp::transport;
    use serde_json::json;
    use std::future::Future;
    use std::pin::pin;
    use std::task::Wake;
    use std::thread;
    use std::time::{Duration, Instant};

    struct Unpark(thread::Thread);

    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }

    /// Runs `future` on this thread until it is done, parking while it waits, and fails if it waits too long.
    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(Unpark(thread::current())));
        let mut cx = Context::from_waker(&waker);
        let mut future = pin!(future);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
                return value;
            }
            assert!(
                Instant::now() < deadline,
                "the future was not woken in ten seconds"
            );
            thread::park_timeout(Duration::from_millis(200));
        }
    }

    fn wait_for<T>(mailbox: &Arc<Mailbox<T>>) {
        block_on(std::future::poll_fn(|cx| mailbox.poll_ready(cx)));
    }

    /// Takes events from `server` until `done` says there are enough.
    fn events_until(server: &Server, done: impl Fn(&[Event]) -> bool) -> Vec<Event> {
        let mailbox = server.mailbox();
        let mut events = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done(&events) {
            assert!(
                Instant::now() < deadline,
                "the server did not do enough in ten seconds: {events:?}"
            );
            wait_for(&mailbox);
            events.extend(mailbox.drain());
        }
        events
    }

    #[test]
    fn a_mailbox_gives_what_was_left_in_order_and_once() {
        let mailbox = Mailbox::new();
        assert!(mailbox.drain().is_empty());
        mailbox.push(1);
        mailbox.push(2);
        mailbox.push(3);
        assert_eq!(mailbox.drain(), vec![1, 2, 3]);
        assert!(mailbox.drain().is_empty());
    }

    #[test]
    fn a_task_that_waits_on_a_mailbox_is_woken_by_a_thread_that_leaves_something() {
        let mailbox = Arc::new(Mailbox::new());
        // Something already there is ready at once.
        mailbox.push("early");
        wait_for(&mailbox);
        assert_eq!(mailbox.drain(), vec!["early"]);
        // Something left after the task has begun to wait wakes it.
        let from = Arc::clone(&mailbox);
        let thread = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            from.push("late");
        });
        wait_for(&mailbox);
        assert_eq!(mailbox.drain(), vec!["late"]);
        thread.join().unwrap();
    }

    #[test]
    fn a_mailbox_that_is_filled_by_many_threads_while_one_waits_loses_nothing() {
        let mailbox = Arc::new(Mailbox::new());
        let threads: Vec<_> = (0..4)
            .map(|thread_number| {
                let mailbox = Arc::clone(&mailbox);
                thread::spawn(move || {
                    for n in 0..250 {
                        mailbox.push(thread_number * 1000 + n);
                        if n % 50 == 0 {
                            thread::yield_now();
                        }
                    }
                })
            })
            .collect();
        let mut got = Vec::new();
        while got.len() < 1000 {
            wait_for(&mailbox);
            got.extend(mailbox.drain());
        }
        for thread in threads {
            thread.join().unwrap();
        }
        got.sort_unstable();
        let mut expected: Vec<i32> = (0..4)
            .flat_map(|t| (0..250).map(move |n| t * 1000 + n))
            .collect();
        expected.sort_unstable();
        assert_eq!(got, expected);
    }

    #[cfg(unix)]
    #[test]
    fn what_a_server_is_sent_goes_through_its_pipes_and_what_it_says_comes_back_as_messages() {
        // `cat` says what it is told, so a request that is sent comes back as a request.
        let server = Server::spawn("cat", &[], Path::new("/")).expect("cat can be run");
        server.send(transport::request(
            5,
            "echo",
            json!({"text": "caf\u{e9} \u{1f600}"}),
        ));
        server.send(transport::notification("later", json!([1, 2])));
        server.send(b"{\"not\": \"a message\"}".to_vec());
        let events = events_until(&server, |events| events.len() >= 3);
        assert_eq!(
            events[0],
            Event::Message(Incoming::Request {
                id: json!(5),
                method: "echo".into(),
                params: json!({"text": "caf\u{e9} \u{1f600}"})
            })
        );
        assert_eq!(
            events[1],
            Event::Message(Incoming::Notification {
                method: "later".into(),
                params: json!([1, 2])
            })
        );
        assert!(matches!(&events[2], Event::Malformed(_)), "{:?}", events[2]);
        server.kill();
    }

    #[cfg(unix)]
    #[test]
    fn a_server_that_exits_is_reported_gone_with_how_it_exited_and_what_it_said_last() {
        let server = Server::spawn(
            "sh",
            &["-c".into(), "echo 'the last thing said' >&2; exit 3".into()],
            Path::new("/"),
        )
        .expect("sh can be run");
        let events = events_until(&server, |events| {
            events
                .iter()
                .any(|event| matches!(event, Event::Gone(_)))
        });
        let Some(Event::Gone(why)) = events.last() else {
            panic!("{events:?}")
        };
        assert!(why.contains('3'), "the status is said: {why}");
        assert!(
            why.contains("the last thing said"),
            "and what it last said on its error output: {why}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_server_that_is_killed_is_reported_gone_at_once() {
        let server =
            Server::spawn("sleep", &["30".into()], Path::new("/")).expect("sleep can be run");
        assert!(server.id() > 0);
        let started = Instant::now();
        server.kill();
        let events = events_until(&server, |events| {
            events
                .iter()
                .any(|event| matches!(event, Event::Gone(_)))
        });
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        assert!(matches!(events.last(), Some(Event::Gone(_))));
    }

    #[test]
    fn a_program_that_cannot_be_run_is_an_error_and_not_a_server_that_is_gone() {
        let error = Server::spawn("a-program-that-is-not-installed-here", &[], Path::new("/"))
            .err()
            .expect("it is not found");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        let error = Server::spawn("cat", &[], Path::new("/a/folder/that/is/not/there"))
            .err()
            .expect("the folder is not there");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }
}
