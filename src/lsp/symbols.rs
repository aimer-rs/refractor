//! The definitions of the project that fit a name, asked of the server: what a search for a type or a function lists.

use super::client::Lsp;
use super::protocol::{self, Symbol};

impl Lsp {
    /// Asks the server for the definitions of the project whose names fit `query`, and calls `done` with them, in the order
    /// the server gave them, or with none when it could not be asked or could not say. The number that is returned is the
    /// request's, for [`cancel`](Self::cancel): none where it could not be asked, which is when `done` has been called.
    /// `done` is not called from inside this unless it is with none, and not at all if the request is cancelled.
    pub fn workspace_symbols(
        &self,
        query: &str,
        done: impl FnOnce(Option<Vec<Symbol>>) + 'static,
    ) -> Option<i64> {
        self.ask(protocol::WORKSPACE_SYMBOL, protocol::symbol_params(query), move |answer| {
            done(answer.map(|answer| protocol::parse_symbols(&answer)));
        })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::Path;
    use std::rc::Rc;

    use aimer::input::TextEditingController;
    use serde_json::json;

    use super::*;
    use crate::lsp::SymbolKind;
    use crate::lsp::fake::{Fake, installed, lsp_with, settle};

    /// A server that is ready, with a file open, and the fake that is its script.
    fn ready() -> (Rc<aimer::Venus>, Fake, Lsp) {
        let venus = installed();
        let fake = Fake::new();
        let lsp = lsp_with(&fake);
        lsp.open(Path::new("/project/src/main.rs"), &TextEditingController::with_text("fn a() {}\n"), Rc::new(|| {}));
        settle(&venus);
        let _ = fake.requests();
        fake.answer(1, json!({"capabilities": {"textDocumentSync": 2, "positionEncoding": "utf-8"}}));
        settle(&venus);
        let _ = fake.requests();
        (venus, fake, lsp)
    }

    type Given = Rc<RefCell<Vec<Option<Vec<Symbol>>>>>;

    fn ask(lsp: &Lsp, query: &str) -> (Option<i64>, Given) {
        let given: Given = Rc::default();
        let sink = Rc::clone(&given);
        let id = lsp.workspace_symbols(query, move |symbols| sink.borrow_mut().push(symbols));
        (id, given)
    }

    #[test]
    fn the_symbols_that_fit_a_query_are_asked_of_the_server_and_given_when_it_answers() {
        let (venus, fake, lsp) = ready();
        let (id, given) = ask(&lsp, "req");
        let id = id.expect("the server can be asked");
        let sent = fake.requests();
        let (method, params, number) = sent.last().expect("a request was sent");
        assert_eq!((method.as_str(), params, *number), ("workspace/symbol", &json!({"query": "req"}), Some(id)));
        assert!(given.borrow().is_empty(), "it is not called from inside");

        fake.answer(id, json!([{"name": "request", "kind": 12, "containerName": "ureq",
            "location": {"uri": "file:///project/src/main.rs",
                         "range": {"start": {"line": 0, "character": 3}, "end": {"line": 0, "character": 10}}}}]));
        settle(&venus);
        let given = given.borrow();
        let symbols = given[0].as_ref().expect("an answer");
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "request");
        assert_eq!(symbols[0].kind, SymbolKind::Function);
    }

    #[test]
    fn a_request_that_is_cancelled_is_never_answered() {
        let (venus, fake, lsp) = ready();
        let (id, given) = ask(&lsp, "req");
        let id = id.expect("asked");
        lsp.cancel(id);
        fake.answer(id, json!([]));
        settle(&venus);
        assert!(given.borrow().is_empty());
    }

    #[test]
    fn a_server_that_is_not_there_gives_none_at_once() {
        let lsp = Lsp::with_starter(
            std::path::PathBuf::from("/project"),
            Box::new(|_| Err(std::io::Error::other("no server"))),
            true,
        );
        let (id, given) = ask(&lsp, "req");
        assert_eq!(id, None);
        assert_eq!(&*given.borrow(), &[None]);
    }
}
