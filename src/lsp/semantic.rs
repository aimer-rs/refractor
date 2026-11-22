//! What the server knows of each name of a file, as bytes of the text: which kind of name it is, and what is said of it.
//!
//! The tokens of the whole file are asked for, and they are given with the legend that says what their numbers stand for and the text
//! that they are of, since the text goes on changing while the server works them out and what shows them has to move them over what
//! was typed.

use std::ops::Range;
use std::path::Path;
use std::rc::Rc;

use aimer::input::input::TextSnapshot;

use super::client::Lsp;
use super::position::Position;
use super::protocol::{self, Legend};

/// A name, where it is and what the server calls it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticToken {
    /// The bytes of the name, in the text that the tokens were asked for.
    pub range: Range<usize>,
    /// Which kind of the legend it is.
    pub kind: u32,
    /// Which modifiers of the legend it has, as bits.
    pub modifiers: u32,
}

/// The tokens of a file.
#[derive(Clone)]
pub struct SemanticTokens {
    /// In the order of the text, and none of them is empty.
    pub tokens: Vec<SemanticToken>,
    /// What the numbers of the tokens are called.
    pub legend: Rc<Legend>,
    /// The text they are of.
    pub text: TextSnapshot,
}

impl Lsp {
    /// Asks the server for the tokens of the open file `path` and calls `done` with them when it has said, or with none when it could
    /// not be asked, has no tokens to give, or could not say. `done` is not called from inside this unless it is with none, and not at
    /// all if the request is [cancelled](Self::cancel). The number that is returned is the request's.
    pub fn semantic_tokens(
        &self,
        path: &Path,
        done: impl FnOnce(Option<SemanticTokens>) + 'static,
    ) -> Option<i64> {
        let wanted = self.snapshot(path).zip(self.legend());
        let Some((text, legend)) = wanted else {
            done(None);
            return None;
        };
        let lsp = self.clone();
        self.ask(
            protocol::SEMANTIC_TOKENS_FULL,
            protocol::semantic_params(path),
            move |reply| {
                done(reply.map(|reply| {
                    let tokens = protocol::parse_tokens(&reply)
                        .into_iter()
                        .filter_map(|token| {
                            let at = |character| Position {
                                line: token.line,
                                character,
                            };
                            let start = lsp.byte_of(&text, at(token.start));
                            let end =
                                lsp.byte_of(&text, at(token.start.saturating_add(token.length)));
                            (end > start).then_some(SemanticToken {
                                range: start..end,
                                kind: token.kind,
                                modifiers: token.modifiers,
                            })
                        })
                        .collect();
                    SemanticTokens {
                        tokens,
                        legend,
                        text,
                    }
                }));
            },
        )
    }

    /// Adds what is called when the server says that its tokens are not as they were, and are to be asked for again: which is when it
    /// has found out what the names it did not know are. It says whether what it is for is still there, and is forgotten when it is not.
    pub fn on_semantic_refresh(&self, hook: Rc<dyn Fn() -> bool>) {
        self.add_semantic_refresh(hook);
    }
}

#[cfg(test)]
mod tests;
