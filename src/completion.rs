//! Completion: what the word before the caret could be. The items come from the language server, which is asked
//! when a word is begun, and are filtered here as the word grows; where there is no server, the keywords of the
//! language are what is offered.

mod call;
mod completer;
mod keywords;
mod label;
mod lsp_source;
mod menu;
mod rank;
mod word;

pub use call::Call;
pub use label::Label;
pub use completer::{Completer, Source};
pub use lsp_source::LspSource;
pub use menu::{Menu, MenuCell};
