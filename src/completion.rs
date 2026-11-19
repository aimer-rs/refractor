//! Completion: what the word before the caret could be. The items come from the language server, which is asked
//! when a word is begun, and are filtered here as the word grows; where there is no server, the keywords of the
//! language are what is offered.

mod completer;
mod keywords;
mod lsp_source;
mod menu;
mod rank;
mod word;

pub use completer::{Completer, Source};
pub use lsp_source::LspSource;
pub use menu::{Menu, MenuCell};
