//! Signature help: asking the server for the parameters of the call around the caret.

use std::path::Path;

use super::client::Lsp;
use super::protocol::{self, SignatureHelp};

impl Lsp {
    /// Asks the server for the signature help at byte `at` of the open file `path`, and calls `done` with the active
    /// signature's parameters when it answers, or with `None` when it could not be asked, has nothing to say, or could
    /// not say. `done` is not called from inside this unless it is with `None`, and not at all if cancelled via
    /// [`cancel`](Self::cancel).
    pub fn signature_help(
        &self,
        path: &Path,
        at: usize,
        done: impl FnOnce(Option<SignatureHelp>) + 'static,
    ) -> Option<i64> {
        let text = self.snapshot(path).filter(|_| self.can_signature_help());
        let Some(text) = text else {
            done(None);
            return None;
        };
        let position = self.position_of(&text, at);
        let encoding = self.encoding();
        let params = protocol::signature_params(path, position);
        self.ask(protocol::SIGNATURE_HELP, params, move |reply| {
            done(reply.and_then(|value| protocol::signature_help(&value, encoding)));
        })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::Path;
    use std::rc::Rc;

    use aimer::Venus;
    use aimer::input::TextEditingController;
    use serde_json::json;

    use crate::services::lsp::fake::{Fake, installed, lsp_with, settle};
    use crate::services::lsp::process::Event;
    use crate::services::lsp::protocol::SignatureHelp;
    use crate::services::lsp::transport::Incoming;

    const FILE: &str = "/project/src/main.rs";

    #[test]
    fn signature_help_asks_at_caret_position_and_parses_parameters() {
        let venus = installed();
        let fake = Fake::new();
        let lsp = lsp_with(&fake);
        let controller = TextEditingController::with_text("fn main() { foo(1, ); }\n");
        lsp.open(Path::new(FILE), &controller, Rc::new(|| {}));
        settle(&venus);
        let _ = fake.methods();
        fake.mailbox.push(Event::Message(Incoming::Response {
            id: 1,
            result: Ok(json!({
                "capabilities": {
                    "textDocumentSync": 2,
                    "positionEncoding": "utf-8",
                    "signatureHelpProvider": {"triggerCharacters": ["(", ","]},
                }
            })),
        }));
        settle(&venus);
        let _ = fake.methods();

        let caret = controller.value().text().find(");").unwrap();
        let got: Rc<RefCell<Vec<Option<SignatureHelp>>>> = Rc::default();
        let into = Rc::clone(&got);
        let id = lsp
            .signature_help(Path::new(FILE), caret, move |help| {
                into.borrow_mut().push(help);
            })
            .expect("ready server with signatureHelpProvider is asked");
        let told = fake.methods();
        assert_eq!(told.len(), 1);
        assert_eq!(told[0].0, "textDocument/signatureHelp");
        assert_eq!(told[0].1["position"], json!({"line": 0, "character": caret}));

        fake.mailbox.push(Event::Message(Incoming::Response {
            id,
            result: Ok(json!({
                "signatures": [{
                    "label": "fn foo(a: i32, b: &str)",
                    "parameters": [
                        {"label": "a: i32"},
                        {"label": "b: &str"},
                    ],
                }],
                "activeParameter": 1,
            })),
        }));
        settle(&venus);
        assert_eq!(
            *got.borrow(),
            vec![Some(SignatureHelp {
                parameters: vec!["a: i32".to_owned(), "b: &str".to_owned()],
                active_parameter: Some(1),
            })]
        );
        let _ = Venus::uninstall();
    }

    #[test]
    fn signature_help_returns_none_immediately_when_server_does_not_support_it() {
        let venus = installed();
        let fake = Fake::new();
        let lsp = lsp_with(&fake);
        let controller = TextEditingController::with_text("fn main() { foo(); }\n");
        lsp.open(Path::new(FILE), &controller, Rc::new(|| {}));
        settle(&venus);
        let _ = fake.methods();
        fake.mailbox.push(Event::Message(Incoming::Response {
            id: 1,
            result: Ok(json!({"capabilities": {"textDocumentSync": 2, "positionEncoding": "utf-8"}})),
        }));
        settle(&venus);
        let _ = fake.methods();

        let got: Rc<RefCell<Vec<Option<SignatureHelp>>>> = Rc::default();
        let into = Rc::clone(&got);
        let id = lsp.signature_help(Path::new(FILE), 16, move |help| {
            into.borrow_mut().push(help);
        });
        assert_eq!(id, None);
        assert_eq!(*got.borrow(), vec![None]);
        assert!(fake.methods().is_empty());
        let _ = Venus::uninstall();
    }
}
