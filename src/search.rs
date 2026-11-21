//! Search over everything of the folder that the editor can go to.

mod actions;
mod files;
mod fuzzy;
mod hits;
mod model;
mod preview;
#[cfg(test)]
mod scratch;
mod text;

pub use self::actions::{Action, ActionId};
pub use self::files::{FileHit, FileIndex};
pub use self::fuzzy::{Match, Pattern};
pub use self::hits::{Hit, Kind, Parts, Tab, Target};
pub use self::model::Search;
pub use self::preview::Preview;
pub use self::text::{Found, TextHit, TextSearch};
