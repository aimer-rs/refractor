//! What the menu of a right click in the file tree offers: making a file or a folder, putting a row on the clipboard or copying its path,
//! showing it in the system's file manager, and renaming or deleting it.
//!
//! All of the rows are listed wherever the click was, so that the menu does not change shape under the one who reads it; when the click was
//! on the blank space below the rows, what acts on a row (cutting, copying, renaming and deleting it) cannot be chosen, while making
//! something new, copying the folder's path and showing the folder can.

/// What a row of the file tree's menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    NewFile,
    NewFolder,
    Cut,
    Copy,
    Paste,
    CopyPath,
    ShowInFileManager,
    Rename,
    Delete,
}

impl Action {
    /// Every action, in the order the menu draws them.
    pub const ALL: [Self; 9] = [
        Self::NewFile,
        Self::NewFolder,
        Self::Cut,
        Self::Copy,
        Self::Paste,
        Self::CopyPath,
        Self::ShowInFileManager,
        Self::Rename,
        Self::Delete,
    ];

    /// What the row says, with the system's file manager named as `mac` names it.
    pub const fn label(self, mac: bool) -> &'static str {
        match (self, mac) {
            (Self::NewFile, _) => "New File",
            (Self::NewFolder, _) => "New Folder",
            (Self::Cut, _) => "Cut",
            (Self::Copy, _) => "Copy",
            (Self::Paste, _) => "Paste",
            (Self::CopyPath, _) => "Copy Path",
            (Self::ShowInFileManager, true) => "Show in Finder",
            (Self::ShowInFileManager, false) => "Show in Explorer",
            (Self::Rename, _) => "Rename\u{2026}",
            (Self::Delete, _) => "Delete",
        }
    }

    /// The keys that do the same as the row, as [`aimer::shortcut::KeyChord`] reads them, if the menu bar binds them.
    ///
    /// `Cut`, `Copy`, and `Paste` are the system's editing keys, which the `Edit` menu already owns; they are hinted on the row and
    /// not bound a second time in `File`.
    pub const fn chord(self) -> Option<&'static str> {
        match self {
            Self::NewFile => Some("CmdOrCtrl+N"),
            Self::NewFolder => Some("CmdOrCtrl+Shift+N"),
            Self::Cut | Self::Copy | Self::Paste => None,
            Self::CopyPath => Some("CmdOrCtrl+Alt+C"),
            Self::ShowInFileManager => Some("CmdOrCtrl+Alt+R"),
            Self::Rename => Some("F2"),
            Self::Delete => Some("CmdOrCtrl+Backspace"),
        }
    }

    /// The keys that do the same as the row, written as `mac` writes them.
    pub const fn hint(self, mac: bool) -> Option<&'static str> {
        Some(match (self, mac) {
            (Self::NewFile, true) => "\u{2318}N",
            (Self::NewFile, false) => "Ctrl+N",
            (Self::NewFolder, true) => "\u{21e7}\u{2318}N",
            (Self::NewFolder, false) => "Ctrl+Shift+N",
            (Self::Cut, true) => "\u{2318}X",
            (Self::Cut, false) => "Ctrl+X",
            (Self::Copy, true) => "\u{2318}C",
            (Self::Copy, false) => "Ctrl+C",
            (Self::Paste, true) => "\u{2318}V",
            (Self::Paste, false) => "Ctrl+V",
            (Self::CopyPath, true) => "\u{2325}\u{2318}C",
            (Self::CopyPath, false) => "Ctrl+Alt+C",
            (Self::ShowInFileManager, true) => "\u{2325}\u{2318}R",
            (Self::ShowInFileManager, false) => "Ctrl+Alt+R",
            (Self::Rename, _) => "F2",
            (Self::Delete, true) => "\u{2318}\u{232b}",
            (Self::Delete, false) => "Ctrl+Backspace",
        })
    }

    /// The picture drawn before the row, if it has one. The first two groups (creation and clipboard) have none and line up with the rest.
    pub fn icon(self) -> Option<&'static aimer::StaticSvg> {
        match self {
            Self::NewFile
            | Self::NewFolder
            | Self::Cut
            | Self::Copy
            | Self::Paste
            | Self::CopyPath => None,
            Self::ShowInFileManager => Some(aimer::Icons::FOLDER_OPEN),
            Self::Rename => Some(aimer::Icons::PENCIL),
            Self::Delete => Some(aimer::Icons::TRASH),
        }
    }

    /// Whether a line goes above the row to set its group apart from the one before it.
    pub const fn separated(self) -> bool {
        matches!(self, Self::Cut | Self::ShowInFileManager | Self::Rename)
    }

    /// Whether the row can be chosen when `selection_count` entries are targeted (0 for blank space, 1 for a single row, >1 for a
    /// multi-selection) and whether the file clipboard holds an entry to paste (`can_paste`).
    pub const fn enabled(self, selection_count: usize, can_paste: bool) -> bool {
        match self {
            Self::NewFile | Self::NewFolder | Self::CopyPath | Self::ShowInFileManager => true,
            Self::Paste => can_paste,
            Self::Cut | Self::Copy | Self::Delete => selection_count > 0,
            Self::Rename => selection_count == 1,
        }
    }
}

/// A row of the file tree's menu: what it does, whether it can be chosen, and whether a line sets it apart from the row above it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub action: Action,
    pub enabled: bool,
    pub separated: bool,
}

/// The rows of the menu for `selection_count` targeted entries (0 on blank space, 1 on a single row, >1 on a multi-selection), with
/// `can_paste` saying whether the file clipboard holds an entry.
pub fn layout(selection_count: usize, can_paste: bool) -> Vec<Row> {
    Action::ALL
        .into_iter()
        .map(|action| Row {
            action,
            enabled: action.enabled(selection_count, can_paste),
            separated: action.separated(),
        })
        .collect()
}

#[cfg(test)]
#[path = "../tests/ui/tree_menu.rs"]
mod tests;
