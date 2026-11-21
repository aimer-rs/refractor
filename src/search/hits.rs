//! What a search lists: the tabs, a hit of any kind, and how what the sources found is put in one list.

use std::ops::Range;
use std::path::PathBuf;

use super::actions::{Action, ActionId};
use super::{FileHit, Pattern, TextHit};
use crate::lsp::{Place, Symbol, SymbolKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tab {
    All,
    Types,
    Files,
    Symbols,
    Actions,
    Text,
}

impl Tab {
    pub const ALL: [Tab; 6] = [Tab::All, Tab::Types, Tab::Files, Tab::Symbols, Tab::Actions, Tab::Text];
    /// How many names, and how many lines of text, the All tab lists at most: it is a glance over everything, and each of the
    /// other tabs is where to look at one kind further.
    pub const NAMES_IN_ALL: usize = 30;
    pub const TEXT_IN_ALL: usize = 10;

    pub fn title(self) -> &'static str {
        match self {
            Tab::All => "All",
            Tab::Types => "Types",
            Tab::Files => "Files",
            Tab::Symbols => "Symbols",
            Tab::Actions => "Actions",
            Tab::Text => "Text",
        }
    }

    /// The tab `delta` tabs after this one, going round the ends: before is negative.
    pub fn step(self, delta: isize) -> Tab {
        let at = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0);
        Self::ALL[(at as isize + delta).rem_euclid(Self::ALL.len() as isize) as usize]
    }

    /// Whether the tab lists lines of text, which is what the files are read for.
    pub fn wants_text(self) -> bool {
        matches!(self, Tab::All | Tab::Text)
    }

    /// Whether the tab lists definitions, which is what the language server is asked for.
    pub fn wants_symbols(self) -> bool {
        matches!(self, Tab::All | Tab::Types | Tab::Symbols)
    }

    /// Whether the tab lists files, which is what the index of the folder is looked through for.
    pub fn wants_files(self) -> bool {
        matches!(self, Tab::All | Tab::Files)
    }

    /// Whether the tab lists actions.
    pub fn wants_actions(self) -> bool {
        matches!(self, Tab::All | Tab::Actions)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    File,
    Symbol(SymbolKind),
    Action,
    Text,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    File { path: PathBuf, at: Option<(usize, usize)> },
    Place(Place),
    Action(ActionId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub kind: Kind,
    pub title: String,
    pub marked: Vec<Range<usize>>,
    pub detail: String,
    pub target: Target,
    pub score: i32,
}

#[derive(Clone, Debug, Default)]
pub struct Parts {
    pub root: PathBuf,
    pub symbols: Vec<Symbol>,
    pub files: Vec<FileHit>,
    pub actions: Vec<(Action, super::Match)>,
    pub text: Vec<TextHit>,
    pub pattern: Pattern,
}

impl Hit {
    /// What a symbol is listed as. The letters of `pattern` are marked in its name, where they are; a symbol the server found
    /// that has not those letters is listed after the rest, unmarked.
    fn of_symbol(symbol: &Symbol, pattern: &Pattern, root: &std::path::Path) -> Self {
        let (score, marked) = match pattern.matches(&symbol.name) {
            Some(found) => (found.score, found.ranges),
            None => (Self::UNMATCHED, Vec::new()),
        };
        Self {
            kind: Kind::Symbol(symbol.kind),
            title: symbol.name.clone(),
            marked,
            // What it is in, as the server says it; and when it says nothing, the file, which is as good a place to look.
            detail: symbol.container.as_ref().map_or_else(
                || symbol.place.path.strip_prefix(root).unwrap_or(&symbol.place.path).display().to_string(),
                |container| format!("in {container}"),
            ),
            target: Target::Place(symbol.place.clone()),
            score,
        }
    }

    fn of_file(file: &FileHit) -> Self {
        Self {
            kind: Kind::File,
            title: file.name.clone(),
            marked: file.marked.clone(),
            detail: file.directory.clone(),
            target: Target::File { path: file.path.clone(), at: None },
            score: file.score,
        }
    }

    fn of_action((action, found): &(Action, super::Match)) -> Self {
        Self {
            kind: Kind::Action,
            title: action.title.to_owned(),
            marked: found.ranges.clone(),
            detail: action.group.to_owned(),
            target: Target::Action(action.id),
            score: found.score,
        }
    }

    fn of_text(hit: &TextHit, root: &std::path::Path) -> Self {
        let shown = hit.path.strip_prefix(root).unwrap_or(&hit.path);
        Self {
            kind: Kind::Text,
            title: hit.preview.clone(),
            marked: vec![hit.marked.clone()],
            detail: format!("{}:{}", shown.display(), hit.line),
            target: Target::File { path: hit.path.clone(), at: Some((hit.line, hit.column)) },
            score: 0,
        }
    }

    /// The score of a symbol that has not the letters of the pattern: below any that has.
    const UNMATCHED: i32 = -1000;
}

/// Puts the best first, and keeps what fits equally well in the order it came in.
fn by_score(hits: &mut [Hit]) {
    hits.sort_by_key(|hit| std::cmp::Reverse(hit.score));
}

impl Tab {
    /// The list this tab shows of what the sources found.
    pub fn combine(self, parts: &Parts) -> Vec<Hit> {
        let symbols = |types: bool| -> Vec<Hit> {
            let mut hits: Vec<Hit> = parts
                .symbols
                .iter()
                .filter(|symbol| symbol.kind.is_type() == types)
                .map(|symbol| Hit::of_symbol(symbol, &parts.pattern, &parts.root))
                .collect();
            by_score(&mut hits);
            hits
        };
        let text = |limit: usize| parts.text.iter().take(limit).map(|hit| Hit::of_text(hit, &parts.root));
        match self {
            Tab::Types => symbols(true),
            Tab::Symbols => symbols(false),
            Tab::Files => parts.files.iter().map(Hit::of_file).collect(),
            Tab::Actions => parts.actions.iter().map(Hit::of_action).collect(),
            Tab::Text => text(usize::MAX).collect(),
            Tab::All => {
                let mut names: Vec<Hit> = parts
                    .symbols
                    .iter()
                    .map(|symbol| Hit::of_symbol(symbol, &parts.pattern, &parts.root))
                    .chain(parts.files.iter().map(Hit::of_file))
                    .chain(parts.actions.iter().map(Hit::of_action))
                    .collect();
                by_score(&mut names);
                names.truncate(Self::NAMES_IN_ALL);
                names.extend(text(Self::TEXT_IN_ALL));
                names
            }
        }
    }
}

#[cfg(test)]
#[path = "hits_tests.rs"]
mod hits_tests;
