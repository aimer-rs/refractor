//! What the menu of a right click in the editor offers beyond the clipboard: which rows there are, and which of them can be chosen where the
//! click was. What each row does is the editor's, which has the language server, the usages and the rest to do it with.

use std::ops::Range;

/// What the click was on, as far as the rows are concerned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Facts {
    /// The bytes of the name that the click was on, or that is selected, if there is one.
    pub name: Option<Range<usize>>,
    /// Whether the server says something is wrong at the place, which it may have a fix for.
    pub problem: bool,
}

/// What a row does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Shows the problem at the place and what can be done about it.
    ShowContextActions,
    /// Lists who uses the name.
    FindUsages,
    /// Goes to where the name is defined, or lists who uses it where it is the definition.
    GoToDefinition,
    /// Asks what the name is to be called, and calls it that everywhere.
    Rename,
}

impl Action {
    /// What the row says.
    pub const fn label(self) -> &'static str {
        match self {
            Self::ShowContextActions => "Show Context Actions",
            Self::FindUsages => "Find Usages",
            Self::GoToDefinition => "Go to Definition",
            Self::Rename => "Rename\u{2026}",
        }
    }
}

impl Action {
    /// The keys or the gesture that do the same as the row, written as `mac` writes them, if there are any. Only what is really so is said:
    /// a hint that names a key that does something else is worse than none.
    pub const fn hint(self, mac: bool) -> Option<&'static str> {
        match (self, mac) {
            (Self::GoToDefinition, true) => Some("\u{2318}Click"),
            (Self::GoToDefinition, false) => Some("Ctrl+Click"),
            _ => None,
        }
    }

    /// The picture drawn before the row.
    pub fn icon(self) -> &'static aimer::StaticSvg {
        match self {
            Self::ShowContextActions => aimer::Icons::LIGHTBULB,
            Self::FindUsages => aimer::Icons::SEARCH,
            Self::GoToDefinition => aimer::Icons::ARROW_UP_RIGHT,
            Self::Rename => aimer::Icons::PENCIL,
        }
    }
}

/// A row, and whether it can be chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub action: Action,
    pub enabled: bool,
}

/// The rows that go before the clipboard verbs and those that go after them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub before: Vec<Row>,
    pub after: Vec<Row>,
}

/// The rows for `facts`. They are all there wherever the click was, and those that have nothing to act on cannot be chosen: a menu whose rows
/// came and went with where the pointer was would be read again every time.
pub fn layout(facts: &Facts) -> Layout {
    let named = facts.name.is_some();
    Layout {
        before: vec![Row {
            action: Action::ShowContextActions,
            enabled: facts.problem,
        }],
        after: [Action::FindUsages, Action::GoToDefinition, Action::Rename]
            .into_iter()
            .map(|action| Row {
                action,
                enabled: named,
            })
            .collect(),
    }
}

/// The name that a menu is about: the one under the pointer, and where the pointer was on none, or there was none, the one that the selection
/// starts in. `name_at` says which bytes are the name that a byte is in.
pub fn name_of(
    name_at: &dyn Fn(usize) -> Option<Range<usize>>,
    pointer: Option<usize>,
    selection: &Range<usize>,
) -> Option<Range<usize>> {
    pointer
        .and_then(name_at)
        .or_else(|| name_at(selection.start))
}

#[cfg(test)]
mod tests;
