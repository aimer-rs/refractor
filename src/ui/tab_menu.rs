//! What the menu of a right click on an open tab offers: closing the tab, closing all unpinned tabs,
//! closing the other unpinned tabs, and pinning or unpinning the tab.

/// What a row of a tab's context menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Close,
    CloseAll,
    CloseOther,
    Pin,
}

impl Action {
    /// Every action, in the order the menu draws them.
    pub const ALL: [Self; 4] = [Self::Close, Self::CloseAll, Self::CloseOther, Self::Pin];

    /// What the row says, with `Pin` reading `Unpin` when the tab is already pinned.
    pub const fn label(self, pinned: bool) -> &'static str {
        match self {
            Self::Close => "Close",
            Self::CloseAll => "Close All",
            Self::CloseOther => "Close Other",
            Self::Pin if pinned => "Unpin",
            Self::Pin => "Pin",
        }
    }

    /// The keys that do the same as the row, written as `mac` writes them.
    pub const fn hint(self, mac: bool) -> Option<&'static str> {
        match (self, mac) {
            (Self::Close, true) => Some("\u{2318}W"),
            (Self::Close, false) => Some("Ctrl+W"),
            _ => None,
        }
    }

    /// The picture drawn before the row, if it has one. `Close All` and `Close Other` have none and line up with the rest.
    pub fn icon(self, pinned: bool) -> Option<&'static aimer::StaticSvg> {
        match self {
            Self::Close => Some(aimer::Icons::X),
            Self::CloseAll | Self::CloseOther => None,
            Self::Pin if pinned => Some(aimer::Icons::PIN_OFF),
            Self::Pin => Some(aimer::Icons::PIN),
        }
    }

    /// Whether a line goes above the row to set its group apart from the one before it.
    pub const fn separated(self) -> bool {
        matches!(self, Self::Pin)
    }

    /// Whether the row can be chosen given the state of the clicked tab and the other open tabs.
    pub const fn enabled(self, facts: Facts) -> bool {
        match self {
            Self::Close => true,
            Self::CloseAll => facts.unpinned_tabs > 0,
            Self::CloseOther => facts.other_unpinned_tabs > 0,
            Self::Pin => facts.can_pin,
        }
    }
}

/// What the menu needs to know about the clicked tab and the rest of the tab bar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    /// Whether the clicked tab is pinned.
    pub pinned: bool,
    /// Whether the clicked tab can be pinned (file tabs can; Settings cannot).
    pub can_pin: bool,
    /// How many unpinned tabs are open across the workspace.
    pub unpinned_tabs: usize,
    /// How many unpinned tabs other than the clicked tab are open.
    pub other_unpinned_tabs: usize,
}

/// One row of the menu, as it is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub action: Action,
    pub enabled: bool,
    pub separated: bool,
}

/// The rows of the tab context menu, in order, with whether each can be chosen and whether a line separates it from the row before.
pub fn layout(facts: Facts) -> Vec<Row> {
    Action::ALL
        .into_iter()
        .map(|action| Row {
            action,
            enabled: action.enabled(facts),
            separated: action.separated(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpinned_tab_with_other_tabs_enables_every_action() {
        let rows = layout(Facts {
            pinned: false,
            can_pin: true,
            unpinned_tabs: 3,
            other_unpinned_tabs: 2,
        });
        assert_eq!(rows.len(), 4);
        assert!(rows.iter().all(|row| row.enabled));
        assert_eq!(
            rows.iter()
                .map(|row| row.action)
                .collect::<Vec<_>>(),
            vec![
                Action::Close,
                Action::CloseAll,
                Action::CloseOther,
                Action::Pin
            ]
        );
        assert_eq!(
            rows.iter()
                .map(|row| row.separated)
                .collect::<Vec<_>>(),
            vec![false, false, false, true]
        );
    }

    #[test]
    fn sole_unpinned_tab_disables_close_other_only() {
        let rows = layout(Facts {
            pinned: false,
            can_pin: true,
            unpinned_tabs: 1,
            other_unpinned_tabs: 0,
        });
        let enabled: Vec<(Action, bool)> = rows
            .into_iter()
            .map(|r| (r.action, r.enabled))
            .collect();
        assert_eq!(
            enabled,
            vec![
                (Action::Close, true),
                (Action::CloseAll, true),
                (Action::CloseOther, false),
                (Action::Pin, true),
            ]
        );
    }

    #[test]
    fn all_tabs_pinned_disables_close_all_and_close_other_and_labels_unpin() {
        let rows = layout(Facts {
            pinned: true,
            can_pin: true,
            unpinned_tabs: 0,
            other_unpinned_tabs: 0,
        });
        let enabled: Vec<(Action, bool)> = rows
            .into_iter()
            .map(|r| (r.action, r.enabled))
            .collect();
        assert_eq!(
            enabled,
            vec![
                (Action::Close, true),
                (Action::CloseAll, false),
                (Action::CloseOther, false),
                (Action::Pin, true),
            ]
        );
        assert_eq!(Action::Pin.label(false), "Pin");
        assert_eq!(Action::Pin.label(true), "Unpin");
    }

    #[test]
    fn settings_tab_disables_pin() {
        let rows = layout(Facts {
            pinned: false,
            can_pin: false,
            unpinned_tabs: 2,
            other_unpinned_tabs: 1,
        });
        let pin = rows
            .iter()
            .find(|r| r.action == Action::Pin)
            .unwrap();
        assert!(!pin.enabled);
    }

    #[test]
    fn close_has_platform_shortcut_hint_and_only_close_and_pin_have_icons() {
        assert_eq!(Action::Close.hint(true), Some("\u{2318}W"));
        assert_eq!(Action::Close.hint(false), Some("Ctrl+W"));
        assert_eq!(Action::CloseAll.hint(true), None);
        assert_eq!(Action::CloseOther.hint(false), None);
        assert_eq!(Action::Pin.hint(true), None);
        assert!(Action::Close.icon(false).is_some());
        assert!(Action::CloseAll.icon(false).is_none());
        assert!(Action::CloseOther.icon(false).is_none());
        assert!(!std::ptr::eq(
            Action::Pin.icon(false).expect("pin icon"),
            Action::Pin.icon(true).expect("unpin icon")
        ));
    }
}
