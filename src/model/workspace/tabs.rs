//! Tab pinning and bulk-closing operations on [`Workspace`].

use crate::ui::tab_menu::{Action, Facts};

use super::Workspace;

impl Workspace {
    /// Whether the tab `id` is pinned.
    pub fn is_pinned(&self, id: u64) -> bool {
        self.pinned.contains(&id)
    }

    /// Pins the document tab `id` and moves it to the end of the pinned group on the left of the tab bar.
    /// Does nothing for the settings tab, an unknown id, or a tab that is already pinned.
    pub fn pin_tab(&mut self, id: u64) {
        if id == Self::SETTINGS_TAB || self.pinned.contains(&id) {
            return;
        }
        let Some(index) = self
            .documents
            .iter()
            .position(|doc| doc.id == id)
        else {
            return;
        };
        let target = self
            .documents
            .iter()
            .filter(|doc| self.pinned.contains(&doc.id))
            .count();
        let document = self.documents.remove(index);
        let insert_at = target.min(self.documents.len());
        self.documents.insert(insert_at, document);
        self.pinned.insert(id);
    }

    /// Unpins the document tab `id` and moves it to the start of the unpinned group (immediately after the remaining pinned tabs).
    pub fn unpin_tab(&mut self, id: u64) {
        if !self.pinned.remove(&id) {
            return;
        }
        let Some(index) = self
            .documents
            .iter()
            .position(|doc| doc.id == id)
        else {
            return;
        };
        let document = self.documents.remove(index);
        let insert_at = self
            .documents
            .iter()
            .filter(|doc| self.pinned.contains(&doc.id))
            .count();
        self.documents.insert(insert_at, document);
    }

    /// Toggles the pinned state of the document tab `id`.
    pub fn toggle_pin_tab(&mut self, id: u64) {
        if self.pinned.contains(&id) {
            self.unpin_tab(id);
        } else {
            self.pin_tab(id);
        }
    }

    /// Facts about the tab `id` and the rest of the tab bar, used to enable/disable rows of the tab context menu.
    pub fn tab_menu_facts(&self, id: u64) -> Facts {
        let pinned = self.is_pinned(id);
        let is_doc = self.documents.iter().any(|doc| doc.id == id);
        let can_pin = id != Self::SETTINGS_TAB && is_doc;
        let unpinned_docs = self
            .documents
            .iter()
            .filter(|doc| !self.pinned.contains(&doc.id))
            .count();
        let unpinned_tabs = unpinned_docs + usize::from(self.settings_open);
        let target_is_unpinned =
            (id == Self::SETTINGS_TAB && self.settings_open) || (is_doc && !pinned);
        Facts {
            pinned,
            can_pin,
            unpinned_tabs,
            other_unpinned_tabs: unpinned_tabs.saturating_sub(usize::from(target_is_unpinned)),
        }
    }

    /// Closes all unpinned tabs that have no unsaved changes (including Settings). Pinned tabs and dirty documents are kept open so no
    /// edits are lost.
    pub fn close_all_tabs(&mut self) {
        self.close_unpinned(None);
    }

    /// Closes every unpinned tab other than `keep_id` that has no unsaved changes. `keep_id`, pinned tabs, and dirty documents are kept
    /// open so no edits are lost.
    pub fn close_other_tabs(&mut self, keep_id: u64) {
        self.close_unpinned(Some(keep_id));
    }

    /// Executes a tab context menu `action` targeting the tab `id`.
    pub fn run_tab_action(&mut self, action: Action, id: u64) {
        match action {
            Action::Close => self.request_close(id),
            Action::CloseAll => self.close_all_tabs(),
            Action::CloseOther => self.close_other_tabs(id),
            Action::Pin => self.toggle_pin_tab(id),
        }
    }

    /// Executes a tab menu `action` on the currently active tab (used by the `Tabs` menu bar section).
    pub fn run_tab_action_on_active(&mut self, action: Action) {
        match action {
            Action::CloseAll => self.close_all_tabs(),
            _ => {
                if let Some(id) = self.active {
                    self.run_tab_action(action, id);
                }
            }
        }
    }

    /// Moves the document tab `from_id` to the position of `to_id` within the same group (pinned with pinned, unpinned with unpinned).
    /// Returns `true` if the tab order changed.
    pub fn reorder_tab(&mut self, from_id: u64, to_id: u64) -> bool {
        if from_id == to_id || from_id == Self::SETTINGS_TAB || to_id == Self::SETTINGS_TAB {
            return false;
        }
        if self.is_pinned(from_id) != self.is_pinned(to_id) {
            return false;
        }
        let Some(from_idx) = self
            .documents
            .iter()
            .position(|doc| doc.id == from_id)
        else {
            return false;
        };
        let Some(to_idx) = self
            .documents
            .iter()
            .position(|doc| doc.id == to_id)
        else {
            return false;
        };
        let document = self.documents.remove(from_idx);
        self.documents.insert(to_idx, document);
        true
    }

    fn close_unpinned(&mut self, keep_id: Option<u64>) {
        self.closing = None;
        self.message = None;

        let close_settings = self.settings_open && keep_id != Some(Self::SETTINGS_TAB);
        let mut to_close = Vec::new();
        let mut skipped_dirty: Vec<(u64, String)> = Vec::new();

        for doc in &self.documents {
            if keep_id == Some(doc.id) || self.pinned.contains(&doc.id) {
                continue;
            }
            if doc.is_dirty() {
                skipped_dirty.push((doc.id, doc.name.clone()));
            } else {
                to_close.push(doc.id);
            }
        }

        for id in to_close {
            self.close(id);
        }
        if close_settings {
            self.close(Self::SETTINGS_TAB);
        }

        if let Some(keep) = keep_id {
            let keep_exists = (keep == Self::SETTINGS_TAB && self.settings_open)
                || self.documents.iter().any(|doc| doc.id == keep);
            let active_exists = self.active.is_some_and(|active| {
                (active == Self::SETTINGS_TAB && self.settings_open)
                    || self
                        .documents
                        .iter()
                        .any(|doc| doc.id == active)
            });
            if keep_exists && !active_exists {
                self.active = Some(keep);
            }
        }

        match skipped_dirty.as_slice() {
            [] => {}
            [(dirty_id, name)] => {
                if self.active.is_none() {
                    self.active = Some(*dirty_id);
                }
                self.closing = Some(*dirty_id);
                self.message = Some(format!(
                    "{name} has unsaved changes: save it, or close it again to discard them"
                ));
            }
            [(first_id, _), ..] => {
                if self.active.is_none() {
                    self.active = Some(*first_id);
                }
                self.message = Some(format!(
                    "{} tabs with unsaved changes were kept open",
                    skipped_dirty.len()
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tree::Scratch;
    use std::rc::Rc;

    fn noop() -> Rc<dyn Fn()> {
        Rc::new(|| {})
    }

    fn open_id(space: &mut Workspace, path: &std::path::Path) -> u64 {
        space.open(path, noop()).unwrap();
        space.active.unwrap()
    }

    #[test]
    fn pinning_moves_tabs_to_the_left_group_in_order_and_unpinning_moves_them_after_pinned() {
        let scratch = Scratch::new("ws-tab-pin");
        let a = scratch.file("a.rs", "fn a() {}\n");
        let b = scratch.file("b.rs", "fn b() {}\n");
        let c = scratch.file("c.rs", "fn c() {}\n");
        let mut space = Workspace::new(scratch.0.clone());
        let id_a = open_id(&mut space, &a);
        let id_b = open_id(&mut space, &b);
        let id_c = open_id(&mut space, &c);

        let ids = |s: &Workspace| {
            s.documents
                .iter()
                .map(|d| d.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&space), vec![id_a, id_b, id_c]);

        // Pin C -> moves to front.
        space.toggle_pin_tab(id_c);
        assert!(space.is_pinned(id_c));
        assert_eq!(ids(&space), vec![id_c, id_a, id_b]);

        // Pin B -> moves right after C.
        space.toggle_pin_tab(id_b);
        assert!(space.is_pinned(id_b));
        assert_eq!(ids(&space), vec![id_c, id_b, id_a]);

        // Unpin C -> moves right after remaining pinned tab B.
        space.toggle_pin_tab(id_c);
        assert!(!space.is_pinned(id_c));
        assert_eq!(ids(&space), vec![id_b, id_c, id_a]);

        // Settings tab cannot be pinned.
        space.open_settings();
        space.toggle_pin_tab(Workspace::SETTINGS_TAB);
        assert!(!space.is_pinned(Workspace::SETTINGS_TAB));
    }

    #[test]
    fn close_all_and_close_other_keep_pinned_tabs_and_close_unpinned_including_settings() {
        let scratch = Scratch::new("ws-tab-bulk-close");
        let a = scratch.file("a.rs", "fn a() {}\n");
        let b = scratch.file("b.rs", "fn b() {}\n");
        let c = scratch.file("c.rs", "fn c() {}\n");
        let mut space = Workspace::new(scratch.0.clone());
        let id_a = open_id(&mut space, &a);
        let id_b = open_id(&mut space, &b);
        let _id_c = open_id(&mut space, &c);
        space.open_settings();

        space.pin_tab(id_a);
        // Close Other on B keeps pinned A and B, closes C and Settings.
        space.run_tab_action(Action::CloseOther, id_b);
        let ids: Vec<u64> = space.documents.iter().map(|d| d.id).collect();
        assert_eq!(ids, vec![id_a, id_b]);
        assert!(!space.settings_open);

        // Close All keeps pinned A and closes unpinned B.
        space.run_tab_action(Action::CloseAll, id_b);
        let ids: Vec<u64> = space.documents.iter().map(|d| d.id).collect();
        assert_eq!(ids, vec![id_a]);
        assert_eq!(space.active, Some(id_a));

        // Explicit Close on pinned A closes it and clears its pinned flag.
        space.run_tab_action(Action::Close, id_a);
        assert!(space.documents.is_empty());
        assert!(!space.is_pinned(id_a));
    }

    #[test]
    fn close_all_and_close_other_keep_dirty_tabs_and_report_unsaved_changes() {
        let scratch = Scratch::new("ws-tab-dirty-close");
        let a = scratch.file("a.rs", "fn a() {}\n");
        let b = scratch.file("b.rs", "fn b() {}\n");
        let c = scratch.file("c.rs", "fn c() {}\n");
        let mut space = Workspace::new(scratch.0.clone());
        let id_a = open_id(&mut space, &a);
        let id_b = open_id(&mut space, &b);
        let id_c = open_id(&mut space, &c);

        // Make B dirty.
        space.documents[1]
            .controller
            .set_text("fn b() { /* edited */ }\n");
        assert!(space.documents[1].is_dirty());

        // Close Other on A closes clean C, keeps A and dirty B.
        space.close_other_tabs(id_a);
        let ids: Vec<u64> = space.documents.iter().map(|d| d.id).collect();
        assert_eq!(ids, vec![id_a, id_b]);
        assert_eq!(space.closing, Some(id_b));
        assert!(
            space
                .message
                .as_deref()
                .is_some_and(|m| m.contains("b.rs") && m.contains("unsaved changes"))
        );

        // Close All closes clean A, still keeps dirty B.
        space.close_all_tabs();
        let ids: Vec<u64> = space.documents.iter().map(|d| d.id).collect();
        assert_eq!(ids, vec![id_b]);
        assert_eq!(space.active, Some(id_b));
        let _ = id_c;
    }

    #[test]
    fn tab_menu_facts_counts_unpinned_and_other_unpinned_tabs() {
        let scratch = Scratch::new("ws-tab-facts");
        let a = scratch.file("a.rs", "fn a() {}\n");
        let b = scratch.file("b.rs", "fn b() {}\n");
        let mut space = Workspace::new(scratch.0.clone());
        let id_a = open_id(&mut space, &a);
        let id_b = open_id(&mut space, &b);
        space.pin_tab(id_a);

        assert_eq!(
            space.tab_menu_facts(id_a),
            Facts {
                pinned: true,
                can_pin: true,
                unpinned_tabs: 1,
                other_unpinned_tabs: 1,
            }
        );
        assert_eq!(
            space.tab_menu_facts(id_b),
            Facts {
                pinned: false,
                can_pin: true,
                unpinned_tabs: 1,
                other_unpinned_tabs: 0,
            }
        );

        space.open_settings();
        assert_eq!(
            space.tab_menu_facts(Workspace::SETTINGS_TAB),
            Facts {
                pinned: false,
                can_pin: false,
                unpinned_tabs: 2,
                other_unpinned_tabs: 1,
            }
        );
    }

    #[test]
    fn reordering_moves_tabs_within_their_group_and_refuses_cross_group_or_invalid_moves() {
        let scratch = Scratch::new("ws-tab-reorder");
        let a = scratch.file("a.rs", "fn a() {}\n");
        let b = scratch.file("b.rs", "fn b() {}\n");
        let c = scratch.file("c.rs", "fn c() {}\n");
        let d = scratch.file("d.rs", "fn d() {}\n");
        let mut space = Workspace::new(scratch.0.clone());
        let id_a = open_id(&mut space, &a);
        let id_b = open_id(&mut space, &b);
        let id_c = open_id(&mut space, &c);
        let id_d = open_id(&mut space, &d);

        let ids = |s: &Workspace| {
            s.documents
                .iter()
                .map(|doc| doc.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&space), vec![id_a, id_b, id_c, id_d]);

        // Left-to-right: A onto C -> [B, C, A, D].
        assert!(space.reorder_tab(id_a, id_c));
        assert_eq!(ids(&space), vec![id_b, id_c, id_a, id_d]);
        assert_eq!(space.active, Some(id_d), "active tab is preserved");

        // Right-to-left: D onto B -> [D, B, C, A].
        assert!(space.reorder_tab(id_d, id_b));
        assert_eq!(ids(&space), vec![id_d, id_b, id_c, id_a]);

        // Same tab or settings tab is a no-op.
        assert!(!space.reorder_tab(id_b, id_b));
        assert!(!space.reorder_tab(id_b, Workspace::SETTINGS_TAB));
        assert!(!space.reorder_tab(Workspace::SETTINGS_TAB, id_b));
        assert!(!space.reorder_tab(id_b, 9999));

        // Pin D and B -> [D, B | C, A]. Reordering within pinned group works; across boundary is refused.
        space.pin_tab(id_d);
        space.pin_tab(id_b);
        assert_eq!(ids(&space), vec![id_d, id_b, id_c, id_a]);
        assert!(space.reorder_tab(id_d, id_b));
        assert_eq!(ids(&space), vec![id_b, id_d, id_c, id_a]);
        assert!(!space.reorder_tab(id_b, id_c));
        assert!(!space.reorder_tab(id_c, id_b));
        assert_eq!(ids(&space), vec![id_b, id_d, id_c, id_a]);
    }
}
