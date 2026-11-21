//! A language server for the code in the editor: talks the language server protocol to a server
//! program (rust-analyzer) and keeps what it says, such as the diagnostics of a file, in the terms
//! of the editor, which are byte ranges of the text.
//!
//! The protocol's own types are used in `protocol` and nowhere else; everything that this module
//! shows the rest of the app is made here.

mod client;
mod core;
mod diagnostics;
mod fix;
#[cfg(test)]
pub(crate) mod fake;
mod inlay;
mod items;
mod lens;
mod locate;
mod position;
mod problems;
mod process;
mod protocol;
mod semantic;
mod session;
mod symbols;
mod transport;

pub use self::client::Lsp;
pub use self::core::Summary;
pub use self::fix::CodeFix;
pub use self::inlay::Hint;
pub use self::semantic::{SemanticToken, SemanticTokens};
pub use self::items::{Answer, Edit, Hover, Item};
pub use self::lens::{Lens, LensLoad};
pub use self::position::Position;
pub use self::problems::{Problem, Weight};
pub use self::protocol::{HintKind, ItemKind, Legend, Severity, Place, Symbol, SymbolKind};
pub use self::session::{Diagnostic, Status};
