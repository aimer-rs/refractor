//! A server that is a script, for the tests of what talks to one: what the client sends is kept, and what the server says is
//! put in the mailbox by the test.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use aimer::Venus;
use serde_json::Value;

use super::client::Lsp;
use super::core::{Link, Starter};
use super::process::{Event, Mailbox};
use super::transport::{self, Incoming};

#[derive(Clone)]
pub(crate) struct Fake {
    pub(crate) sent: Rc<RefCell<Vec<Vec<u8>>>>,
    pub(crate) mailbox: Arc<Mailbox<Event>>,
}

impl Link for Fake {
    fn send(&self, body: Vec<u8>) {
        self.sent.borrow_mut().push(body);
    }

    fn mailbox(&self) -> Arc<Mailbox<Event>> {
        Arc::clone(&self.mailbox)
    }

    fn id(&self) -> u32 {
        1
    }
}

impl Fake {
    pub(crate) fn new() -> Self {
        Self {
            sent: Rc::default(),
            mailbox: Arc::new(Mailbox::new()),
        }
    }

    pub(crate) fn methods(&self) -> Vec<(String, Value)> {
        self.sent
            .borrow_mut()
            .drain(..)
            .map(|body| match transport::parse(&body).unwrap() {
                Incoming::Request { method, params, .. }
                | Incoming::Notification { method, params } => (method, params),
                other => panic!("{other:?}"),
            })
            .collect()
    }
}

impl Fake {
    /// What was sent, as `(method, parameters, number)`, the number being there for a request and not for a notification.
    pub(crate) fn requests(&self) -> Vec<(String, Value, Option<i64>)> {
        self.sent
            .borrow_mut()
            .drain(..)
            .filter_map(|body| match transport::parse(&body).unwrap() {
                Incoming::Request { id, method, params } => Some((method, params, id.as_i64())),
                Incoming::Notification { method, params } => Some((method, params, None)),
                // What the client answers to a request of the server's is not what it asks.
                Incoming::Response { .. } => None,
            })
            .collect()
    }

    /// Says `result` to the request `id`, as the server does.
    pub(crate) fn answer(&self, id: i64, result: Value) {
        self.mailbox
            .push(Event::Message(Incoming::Response { id, result: Ok(result) }));
    }

    /// Says a notification of the server's, as it does to a client.
    pub(crate) fn notify(&self, method: &str, params: Value) {
        self.mailbox
            .push(Event::Message(Incoming::Notification { method: method.to_owned(), params }));
    }

    /// Says a request of the server's, as it does to a client: `method` with `params`, to be answered.
    pub(crate) fn request(&self, id: Value, method: &str, params: Value) {
        self.mailbox
            .push(Event::Message(Incoming::Request { id, method: method.to_owned(), params }));
    }

    /// Says that the request `id` failed.
    pub(crate) fn fail(&self, id: i64) {
        self.mailbox.push(Event::Message(Incoming::Response {
            id,
            result: Err(transport::ResponseError { code: -32801, message: "content modified".into() }),
        }));
    }
}

/// Runs the tasks that are ready until there are none.
pub(crate) fn settle(venus: &Rc<Venus>) {
    for _ in 0..50 {
        if !venus.has_ready_work() {
            return;
        }
        venus.run_microtasks();
    }
}

pub(crate) fn lsp_with(fake: &Fake) -> Lsp {
    let fake = fake.clone();
    let starter: Starter = Box::new(move |_| Ok(Box::new(fake.clone()) as Box<dyn Link>));
    Lsp::with_starter(PathBuf::from("/project"), starter, false)
}

pub(crate) fn installed() -> Rc<Venus> {
    let venus = Venus::new();
    venus.install();
    venus
}
