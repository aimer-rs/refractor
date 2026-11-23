//! A language server for the code in the editor: talks the language server protocol to a server
//! program (rust-analyzer) and keeps what it says, such as the diagnostics of a file, in the terms
//! of the editor, which are byte ranges of the text.
//!
//! The protocol's own types are used in `protocol` and nowhere else; everything that this module
//! shows the rest of the app is made here.

mod client;
mod core;
mod diagnostics;
#[cfg(test)]
pub(crate) mod fake;
mod fix;
mod format;
mod inlay;
mod items;
mod lens;
pub(crate) mod locate;
mod position;
mod problems;
mod process;
mod protocol;
mod rename;
mod semantic;
mod session;
mod signature;
mod symbols;
mod transport;

pub use self::client::Lsp;
pub use self::core::Summary;
pub use self::fix::CodeFix;
pub use self::format::format_external;
pub use self::inlay::Hint;
pub use self::items::{Answer, Edit, Hover, Item};
pub use self::lens::{Lens, LensLoad};
pub use self::position::Position;
pub use self::problems::{Problem, Weight};
pub use self::protocol::WorkspaceEdit;
pub use self::protocol::{
    HintKind, ItemKind, Legend, Place, Severity, SignatureHelp, Symbol, SymbolKind,
};
pub use self::rename::Rename;
pub use self::semantic::{SemanticToken, SemanticTokens};
pub use self::session::{Diagnostic, Status};
