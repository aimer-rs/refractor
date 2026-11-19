//! A language server for the code in the editor: talks the language server protocol to a server
//! program (rust-analyzer) and keeps what it says, such as the diagnostics of a file, in the terms
//! of the editor, which are byte ranges of the text.
//!
//! The protocol's own types are used in `protocol` and nowhere else; everything that this module
//! shows the rest of the app is made here.

mod client;
mod core;
mod items;
mod position;
mod problems;
mod process;
mod protocol;
mod session;
mod transport;

pub use self::client::Lsp;
pub use self::core::Summary;
pub use self::session::Status;
pub use self::problems::{Problem, Weight};
pub use self::items::{Answer, Edit, Item};
pub use self::position::Position;
pub use self::protocol::{ItemKind, Place};
