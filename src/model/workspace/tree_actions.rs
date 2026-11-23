//! Workspace-level file tree operations: creating, renaming, deleting, cutting/copying/pasting, copying paths, and revealing
//! entries in the system's file manager, keeping open documents, LSP notifications, and expanded folders in sync.

use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::services::cargo::source::ManifestSource;
use crate::model::completion::LspSource;
use crate::model::document::Language;
use crate::ui::tree_menu::Action as TreeAction;
use crate::services::tree_ops::{self, FileClipboard};
use crate::ui::tree_prompt::TreePrompt;
use crate::model::workspace::{SidePanelTab, Workspace, notifier, refresh_status};
use aimer::{FocusNode, ProviderHandle};

impl Workspace {
    /// The floating prompt shared by the file tree, menu bar commands, and prompt card.
    pub fn tree_prompt(&self) -> Rc<TreePrompt> {
        Rc::clone(&self.tree_prompt)
    }

    /// The focus node of the file tree sidebar.
    pub fn tree_focus(&self) -> FocusNode {
        self.tree_focus.clone()
    }

    /// Toggles keyboard focus between the file tree sidebar and the active editor (`CmdOrCtrl+1`).
    pub fn toggle_tree_focus(&mut self) {
        if self.side_panel_tab == SidePanelTab::Files && self.tree_focus.has_focus() {
            if let Some(doc) = self.active_document() {
                doc.focus.request_focus();
            } else {
                self.tree_focus.unfocus();
            }
        } else {
            self.side_panel_tab = SidePanelTab::Files;
            if self.tree_cursor.is_none() && !self.rows.is_empty() {
                let target_idx = self
                    .active_document()
                    .and_then(|doc| {
                        self.rows
                            .iter()
                            .position(|row| row.path == doc.path)
                    })
                    .unwrap_or(0);
                let (path, is_dir) = (
                    self.rows[target_idx].path.clone(),
                    self.rows[target_idx].is_dir,
                );
                self.select_entry(path, is_dir);
            }
            self.tree_focus.request_focus();
        }
        aimer::events::window::request_animation_frame();
    }

    /// Moves to the previous (`delta < 0`) or next (`delta > 0`) open tab, wrapping around the ends.
    pub fn step_tab(&mut self, delta: isize) {
        let mut tabs: Vec<u64> = self
            .documents
            .iter()
            .map(|doc| doc.id)
            .collect();
        if self.settings_open {
            tabs.push(Self::SETTINGS_TAB);
        }
        if tabs.len() < 2 {
            return;
        }
        let current = self
            .active
            .and_then(|id| tabs.iter().position(|&t| t == id))
            .unwrap_or(0);
        let len = tabs.len() as isize;
        let next = ((current as isize + delta).rem_euclid(len)) as usize;
        let next_id = tabs[next];
        let keep_editor_focus = !self.tree_focus.has_focus();
        self.activate(next_id);
        if keep_editor_focus && let Some(doc) = self.active_document() {
            doc.focus.request_focus();
        }
        aimer::events::window::request_animation_frame();
    }

    /// Requests closing the currently active tab (`CmdOrCtrl+W`).
    pub fn close_active_tab(&mut self) {
        if let Some(id) = self.active {
            self.request_close(id);
        }
    }

    /// Selects a single entry, clearing any previous multi-selection.
    pub fn select_entry(&mut self, path: PathBuf, is_dir: bool) {
        self.selected_entries.clear();
        self.selected_entries.insert(path.clone());
        self.tree_cursor = Some(path.clone());
        self.selected_entry = Some((path, is_dir));
    }

    /// Toggles `path` in or out of the multi-selection set (Cmd/Ctrl-click).
    pub fn toggle_select_entry(&mut self, path: PathBuf, is_dir: bool) {
        if !self.selected_entries.remove(&path) {
            self.selected_entries.insert(path.clone());
            self.tree_cursor = Some(path.clone());
            self.selected_entry = Some((path, is_dir));
        } else if self
            .selected_entry
            .as_ref()
            .is_some_and(|(anchor, _)| anchor == &path)
        {
            self.selected_entry = self
                .rows
                .iter()
                .find(|row| self.selected_entries.contains(&row.path))
                .map(|row| (row.path.clone(), row.is_dir));
            self.tree_cursor = self
                .selected_entry
                .as_ref()
                .map(|(p, _)| p.clone());
        }
    }

    /// Selects the contiguous range of visible tree rows between the current anchor and `path` (Shift-click / Shift-arrow).
    pub fn select_range_to(&mut self, path: PathBuf, is_dir: bool) {
        let Some((anchor_path, _)) = &self.selected_entry else {
            self.select_entry(path, is_dir);
            return;
        };
        let start = self
            .rows
            .iter()
            .position(|row| &row.path == anchor_path);
        let end = self
            .rows
            .iter()
            .position(|row| row.path == path);
        match (start, end) {
            (Some(a), Some(b)) => {
                let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                self.selected_entries.clear();
                for row in &self.rows[lo..=hi] {
                    self.selected_entries.insert(row.path.clone());
                }
                self.tree_cursor = Some(path);
            }
            _ => self.select_entry(path, is_dir),
        }
    }

    /// Clears the file tree selection (both multi-selection and anchor).
    pub fn clear_selection(&mut self) {
        self.selected_entries.clear();
        self.selected_entry = None;
        self.tree_cursor = None;
    }

    /// The index of the current keyboard cursor row in `self.rows`, falling back to the selection anchor or active document.
    pub fn tree_cursor_index(&self) -> Option<usize> {
        let target = self
            .tree_cursor
            .as_deref()
            .or_else(|| {
                self.selected_entry
                    .as_ref()
                    .map(|(p, _)| p.as_path())
            })
            .or_else(|| self.active_document().map(|d| d.path.as_path()))?;
        self.rows
            .iter()
            .position(|row| row.path == target)
    }

    /// Moves the file tree cursor by `delta` rows (`Up`/`Down`/`PageUp`/`PageDown`), optionally extending the selection (`Shift`).
    /// Returns the newly selected row index so the view can scroll it into sight.
    pub fn navigate_tree(&mut self, delta: isize, extend: bool) -> Option<usize> {
        let len = self.rows.len();
        if len == 0 {
            return None;
        }
        let next = match self.tree_cursor_index() {
            Some(current) => current
                .saturating_add_signed(delta)
                .min(len - 1),
            None if delta >= 0 => 0,
            None => len - 1,
        };
        let (path, is_dir) = (self.rows[next].path.clone(), self.rows[next].is_dir);
        if extend {
            self.select_range_to(path, is_dir);
        } else {
            self.select_entry(path, is_dir);
        }
        Some(next)
    }

    /// Moves the file tree cursor to the first (`end == false`, `Home`) or last (`end == true`, `End`) visible row.
    pub fn navigate_tree_edge(&mut self, end: bool, extend: bool) -> Option<usize> {
        let len = self.rows.len();
        if len == 0 {
            return None;
        }
        let next = if end { len - 1 } else { 0 };
        let (path, is_dir) = (self.rows[next].path.clone(), self.rows[next].is_dir);
        if extend {
            self.select_range_to(path, is_dir);
        } else {
            self.select_entry(path, is_dir);
        }
        Some(next)
    }

    /// Handles `Right` arrow on the file tree: expands a collapsed folder, or moves to the first child of an expanded folder.
    pub fn tree_expand_or_descend(&mut self) -> Option<usize> {
        let index = self.tree_cursor_index()?;
        let row = &self.rows[index];
        if !row.is_dir {
            return None;
        }
        if !row.expanded {
            let path = row.path.clone();
            self.toggle(&path);
            return Some(index);
        }
        if let Some(child) = self.rows.get(index + 1)
            && child.depth > row.depth
        {
            let (path, is_dir) = (child.path.clone(), child.is_dir);
            self.select_entry(path, is_dir);
            return Some(index + 1);
        }
        None
    }

    /// Handles `Left` arrow on the file tree: collapses an expanded folder, or moves to the enclosing parent folder.
    pub fn tree_collapse_or_ascend(&mut self) -> Option<usize> {
        let index = self.tree_cursor_index()?;
        let row = &self.rows[index];
        if row.is_dir && row.expanded {
            let path = row.path.clone();
            self.toggle(&path);
            return Some(index);
        }
        let depth = row.depth;
        if depth == 0 {
            return None;
        }
        let parent_idx = (0..index)
            .rev()
            .find(|&i| self.rows[i].depth < depth)?;
        let (path, is_dir) = (
            self.rows[parent_idx].path.clone(),
            self.rows[parent_idx].is_dir,
        );
        self.select_entry(path, is_dir);
        Some(parent_idx)
    }

    /// Selects every currently visible row in the file tree (`CmdOrCtrl+A`).
    pub fn select_all_tree(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        self.selected_entries.clear();
        for row in self.rows.iter() {
            self.selected_entries.insert(row.path.clone());
        }
        if self.selected_entry.is_none() {
            let first = &self.rows[0];
            self.selected_entry = Some((first.path.clone(), first.is_dir));
        }
        if self.tree_cursor.is_none() {
            self.tree_cursor = self.rows.last().map(|r| r.path.clone());
        }
    }

    /// Activates the currently selected tree row (`Enter` / `Space`): toggles a folder, or opens a file (focusing the editor when
    /// `focus_editor` is `true`).
    pub fn activate_selected_tree_row(&mut self, notify: Rc<dyn Fn()>, focus_editor: bool) {
        let Some(index) = self.tree_cursor_index() else {
            return;
        };
        let (path, is_dir) = (self.rows[index].path.clone(), self.rows[index].is_dir);
        if is_dir {
            self.toggle(&path);
        } else if self.open(&path, notify).is_ok() && focus_editor {
            if let Some(doc) = self.active_document() {
                doc.focus.request_focus();
            }
        }
    }

    /// Ensures `path` is selected before opening a context menu: keeps the multi-selection if `path` is already part of it,
    /// otherwise switches to single-selecting `path`.
    pub fn ensure_selected_for_context_menu(&mut self, path: PathBuf, is_dir: bool) {
        if !self.selected_entries.contains(&path) {
            self.select_entry(path, is_dir);
        }
    }

    /// Whether `path` is currently selected in the file tree.
    pub fn is_selected(&self, path: &Path) -> bool {
        self.selected_entries.contains(path)
    }

    /// Selected entries in visible tree order (falling back to any selected paths not currently expanded).
    pub fn selected_entries_list(&self) -> Vec<(PathBuf, bool)> {
        let mut ordered: Vec<(PathBuf, bool)> = self
            .rows
            .iter()
            .filter(|row| self.selected_entries.contains(&row.path))
            .map(|row| (row.path.clone(), row.is_dir))
            .collect();
        for path in &self.selected_entries {
            if !ordered.iter().any(|(p, _)| p == path) && path.symlink_metadata().is_ok() {
                ordered.push((path.clone(), path.is_dir()));
            }
        }
        ordered
    }

    /// Marks `paths` as currently being dragged so their rows render at lower opacity.
    pub fn set_dragging(&mut self, paths: Vec<PathBuf>) {
        self.dragging_paths = paths.into_iter().collect();
    }

    /// Clears the active drag visual state.
    pub fn clear_dragging(&mut self) {
        self.dragging_paths.clear();
    }

    /// Whether `path` is currently being dragged.
    pub fn is_dragging(&self, path: &Path) -> bool {
        self.dragging_paths.contains(path)
    }

    /// The entry that File menu commands act on: the last selected tree entry if it still exists, otherwise the active document.
    pub fn target_for_menu_bar(&self) -> Option<(PathBuf, bool)> {
        if let Some((path, is_dir)) = &self.selected_entry
            && path.symlink_metadata().is_ok()
        {
            return Some((path.clone(), *is_dir));
        }
        self.active_document()
            .map(|doc| (doc.path.clone(), false))
    }

    /// Expands `dir` and every ancestor between `self.root` and `dir` so entries inside `dir` are visible in the tree.
    pub fn expand(&mut self, dir: &Path) {
        if dir == self.root || !dir.starts_with(&self.root) {
            return;
        }
        let mut cursor = Some(dir);
        while let Some(current) = cursor {
            if current == self.root {
                break;
            }
            self.expanded.insert(current.to_path_buf());
            cursor = current.parent();
        }
    }

    /// Stages `paths` on the file tree clipboard for Cut (`cut == true`) or Copy (`cut == false`).
    pub fn copy_to_clipboard(&mut self, paths: Vec<PathBuf>, cut: bool) {
        let pruned = tree_ops::prune_descendants(&paths);
        if pruned.is_empty() {
            self.clipboard = None;
        } else {
            self.clipboard = Some(FileClipboard { paths: pruned, cut });
        }
    }

    /// Whether the file tree clipboard currently holds any entries to paste.
    pub fn can_paste(&self) -> bool {
        self.clipboard
            .as_ref()
            .is_some_and(|clip| !clip.paths.is_empty())
    }

    /// Whether `path` is currently staged for Cut, so its row can be dimmed.
    pub fn is_cut(&self, path: &Path) -> bool {
        self.clipboard
            .as_ref()
            .is_some_and(|clip| clip.cut && clip.paths.iter().any(|p| p == path))
    }

    /// Creates a new file `name` inside `dir`, expands `dir`, refreshes the tree, and opens the new file in a tab.
    pub fn create_file(
        &mut self,
        dir: &Path,
        name: &str,
        notify: Rc<dyn Fn()>,
    ) -> io::Result<PathBuf> {
        let created = tree_ops::create_file(dir, name)?;
        self.expand(dir);
        self.select_entry(created.clone(), false);
        self.refresh();
        let _ = self.open(&created, notify);
        Ok(created)
    }

    /// Creates a new folder `name` inside `dir`, expands `dir`, and refreshes the tree.
    pub fn create_folder(&mut self, dir: &Path, name: &str) -> io::Result<PathBuf> {
        let created = tree_ops::create_folder(dir, name)?;
        self.expand(dir);
        self.select_entry(created.clone(), true);
        self.refresh();
        Ok(created)
    }

    /// Renames `old` to `new_name`, keeping open documents (including unsaved edits), expanded folders, and selection in sync.
    pub fn rename_path(
        &mut self,
        old: &Path,
        new_name: &str,
        notify: Rc<dyn Fn()>,
    ) -> io::Result<PathBuf> {
        let new_path = tree_ops::rename_entry(old, new_name)?;
        self.remap_after_move(old, &new_path, notify);
        self.refresh();
        Ok(new_path)
    }

    /// Deletes `path` from disk, closing any open tabs at or inside `path`, pruning expanded folders, and refreshing the tree.
    pub fn delete_path(&mut self, path: &Path) -> io::Result<()> {
        self.delete_paths(&[path.to_path_buf()])
    }

    /// Deletes all `paths` (pruning nested descendants first), closing any open tabs inside them and refreshing the tree.
    pub fn delete_paths(&mut self, paths: &[PathBuf]) -> io::Result<()> {
        let pruned = tree_ops::prune_descendants(paths);
        for path in &pruned {
            tree_ops::delete_entry(path)?;
            let affected: Vec<u64> = self
                .documents
                .iter()
                .filter(|doc| doc.path == *path || doc.path.starts_with(path))
                .map(|doc| doc.id)
                .collect();
            for id in affected {
                self.close(id);
            }
            self.expanded
                .retain(|entry| entry != path && !entry.starts_with(path));
            self.selected_entries
                .retain(|entry| entry != path && !entry.starts_with(path));
            if self
                .selected_entry
                .as_ref()
                .is_some_and(|(selected, _)| selected == path || selected.starts_with(path))
            {
                self.selected_entry = None;
            }
            if let Some(clip) = &mut self.clipboard {
                clip.paths
                    .retain(|entry| entry != path && !entry.starts_with(path));
                if clip.paths.is_empty() {
                    self.clipboard = None;
                }
            }
        }
        self.refresh();
        Ok(())
    }

    /// Pastes the entries on the file tree clipboard into `dest_dir` (moving on Cut, copying on Copy).
    pub fn paste_into(
        &mut self,
        dest_dir: &Path,
        notify: Rc<dyn Fn()>,
    ) -> io::Result<Vec<PathBuf>> {
        let Some(clip) = self.clipboard.clone() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "nothing to paste",
            ));
        };
        let sources = tree_ops::prune_descendants(&clip.paths);
        if sources.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "nothing to paste",
            ));
        }
        let mut results = Vec::with_capacity(sources.len());
        if clip.cut {
            for src in &sources {
                let moved = tree_ops::move_entry(src, dest_dir)?;
                self.remap_after_move(src, &moved, Rc::clone(&notify));
                results.push(moved);
            }
            self.clipboard = None;
        } else {
            for src in &sources {
                let copied = tree_ops::copy_entry(src, dest_dir)?;
                results.push(copied);
            }
        }
        self.expand(dest_dir);
        self.selected_entries.clear();
        for path in &results {
            self.selected_entries.insert(path.clone());
        }
        if let Some(last) = results.last() {
            self.selected_entry = Some((last.clone(), last.is_dir()));
        }
        self.refresh();
        Ok(results)
    }

    /// Moves `src` into `dest_dir` (e.g. via drag and drop), updating open documents, expanded folders, selection, and tree rows.
    pub fn move_path_to(
        &mut self,
        src: &Path,
        dest_dir: &Path,
        notify: Rc<dyn Fn()>,
    ) -> io::Result<PathBuf> {
        let mut moved = self.move_paths_to(&[src.to_path_buf()], dest_dir, notify)?;
        Ok(moved.pop().expect("one moved path"))
    }

    /// Moves all `sources` into `dest_dir` (pruning nested descendants first), updating open documents, expanded folders, and selection.
    pub fn move_paths_to(
        &mut self,
        sources: &[PathBuf],
        dest_dir: &Path,
        notify: Rc<dyn Fn()>,
    ) -> io::Result<Vec<PathBuf>> {
        let pruned = tree_ops::prune_descendants(sources);
        let mut moved_all = Vec::with_capacity(pruned.len());
        for src in &pruned {
            let moved = tree_ops::move_entry(src, dest_dir)?;
            self.remap_after_move(src, &moved, Rc::clone(&notify));
            moved_all.push(moved);
        }
        self.expand(dest_dir);
        self.selected_entries.clear();
        for path in &moved_all {
            self.selected_entries.insert(path.clone());
        }
        if let Some(last) = moved_all.last() {
            self.selected_entry = Some((last.clone(), last.is_dir()));
        }
        self.refresh();
        Ok(moved_all)
    }

    /// Copies the display strings of `paths` (joined by newlines) to the system clipboard, reporting any error in the status bar.
    pub fn copy_paths_to_clipboard(&mut self, paths: &[PathBuf]) {
        let text = paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        match aimer::native::clipboard::set_text(&text) {
            Ok(()) => self.message = None,
            Err(error) => {
                self.message = Some(format!("Could not copy path: {error}"));
            }
        }
    }

    /// Reveals `path` in Finder (macOS) or Explorer / file manager (other platforms), reporting any error in the status bar.
    pub fn reveal_in_file_manager(&mut self, path: &Path) {
        let open_directly = path == self.root;
        match tree_ops::reveal_in_file_manager(path, open_directly) {
            Ok(()) => self.message = None,
            Err(error) => {
                self.message = Some(format!("Could not reveal {}: {error}", path.display()));
            }
        }
    }

    /// Updates open documents, LSP registrations, expanded folders, selection, and clipboard after `old` moved to `new_path`.
    fn remap_after_move(&mut self, old: &Path, new_path: &Path, notify: Rc<dyn Fn()>) {
        let updated_expanded: Vec<PathBuf> = self
            .expanded
            .iter()
            .map(|entry| {
                tree_ops::remap_path(entry, old, new_path).unwrap_or_else(|| entry.clone())
            })
            .collect();
        self.expanded = updated_expanded.into_iter().collect();

        self.selected_entries = self
            .selected_entries
            .iter()
            .map(|entry| {
                tree_ops::remap_path(entry, old, new_path).unwrap_or_else(|| entry.clone())
            })
            .collect();
        if let Some((selected, is_dir)) = &mut self.selected_entry
            && let Some(remapped) = tree_ops::remap_path(selected, old, new_path)
        {
            *selected = remapped;
            let _ = is_dir;
        }
        if let Some(clip) = &mut self.clipboard {
            for path in &mut clip.paths {
                if let Some(remapped) = tree_ops::remap_path(path, old, new_path) {
                    *path = remapped;
                }
            }
        }

        for index in 0..self.documents.len() {
            let Some(remapped) = tree_ops::remap_path(&self.documents[index].path, old, new_path)
            else {
                continue;
            };
            let previous = Rc::clone(&self.documents[index]);
            let was_active = self.active == Some(previous.id);
            self.lsp.close(&previous.path);

            let new_id = self.next_id;
            self.next_id += 1;
            let doc = previous.relocated(new_id, &remapped, Rc::clone(&notify));
            let (auto_save, weak) = (Rc::clone(&self.auto_save), Rc::downgrade(&doc));
            doc.on_edit(Rc::new(move || {
                if let Some(doc) = weak.upgrade() {
                    let _ = auto_save.edited(&doc);
                }
            }));
            if doc.language == Language::Rust {
                self.lsp
                    .open(&remapped, &doc.controller, Rc::clone(&notify));
                doc.set_completions(LspSource::new(self.lsp.clone(), remapped.clone()));
            } else if doc.is_cargo_manifest() {
                doc.set_completions(ManifestSource::new(doc.controller.clone()));
            }
            self.documents[index] = doc;
            if was_active {
                self.active = Some(new_id);
            }
        }
    }
}

/// Dispatches a file tree menu or File menu bar command against `target` (`Some((path, is_dir))` for an entry, or `None` for the
/// workspace root). When `target` is part of a multi-selection, bulk operations (`Cut`, `Copy`, `CopyPath`, `Delete`) act on all
/// selected entries.
pub fn run_tree_action(
    action: TreeAction,
    target: Option<(PathBuf, bool)>,
    store: &ProviderHandle<Workspace>,
) {
    let (root, prompt, selected) = {
        let space = store.read();
        let selected = match &target {
            Some((path, _)) if space.is_selected(path) && space.selected_entries.len() > 1 => {
                space.selected_entries_list()
            }
            Some(entry) => vec![entry.clone()],
            None => Vec::new(),
        };
        (space.root.clone(), space.tree_prompt(), selected)
    };
    let target_ref = target
        .as_ref()
        .map(|(path, is_dir)| (path.as_path(), *is_dir));

    match action {
        TreeAction::NewFile => {
            let dir = tree_ops::target_dir(&root, target_ref);
            prompt.open_new_file(dir);
        }
        TreeAction::NewFolder => {
            let dir = tree_ops::target_dir(&root, target_ref);
            prompt.open_new_folder(dir);
        }
        TreeAction::Cut => {
            if !selected.is_empty() {
                let paths: Vec<PathBuf> = selected.into_iter().map(|(p, _)| p).collect();
                store.update(|space| space.copy_to_clipboard(paths, true));
            }
        }
        TreeAction::Copy => {
            if !selected.is_empty() {
                let paths: Vec<PathBuf> = selected.into_iter().map(|(p, _)| p).collect();
                store.update(|space| space.copy_to_clipboard(paths, false));
            }
        }
        TreeAction::Paste => {
            let dest_dir = tree_ops::target_dir(&root, target_ref);
            let notify = notifier(store);
            let mut ok = false;
            store.update(|space| match space.paste_into(&dest_dir, notify) {
                Ok(_) => {
                    space.message = None;
                    ok = true;
                }
                Err(error) => {
                    space.message = Some(format!("Could not paste: {error}"));
                }
            });
            if ok {
                refresh_status(store);
            }
        }
        TreeAction::CopyPath => {
            let paths: Vec<PathBuf> = if selected.is_empty() {
                vec![root]
            } else {
                selected.into_iter().map(|(p, _)| p).collect()
            };
            store.update(|space| space.copy_paths_to_clipboard(&paths));
        }
        TreeAction::ShowInFileManager => {
            let path = target.map_or(root, |(path, _)| path);
            store.update(|space| space.reveal_in_file_manager(&path));
        }
        TreeAction::Rename => {
            if selected.len() == 1
                && let Some((path, is_dir)) = selected.into_iter().next()
            {
                let old = path.file_name().map_or_else(
                    || path.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                );
                prompt.open_rename(path, old, is_dir);
            }
        }
        TreeAction::Delete => {
            if !selected.is_empty() {
                let entries = selected
                    .into_iter()
                    .map(|(path, is_dir)| {
                        let name = path.file_name().map_or_else(
                            || path.display().to_string(),
                            |name| name.to_string_lossy().into_owned(),
                        );
                        (path, name, is_dir)
                    })
                    .collect();
                prompt.open_delete_many(entries);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tree::Scratch;

    fn notify() -> Rc<dyn Fn()> {
        Rc::new(|| {})
    }

    #[test]
    fn creating_a_file_expands_its_folder_refreshes_rows_and_opens_it() {
        let scratch = Scratch::new("ws-create-file");
        let dir = scratch.0.join("src");
        std::fs::create_dir(&dir).unwrap();
        let mut space = Workspace::new(scratch.0.clone());

        let created = space
            .create_file(&dir, "lib.rs", notify())
            .unwrap();
        assert!(created.is_file());
        assert_eq!(
            space.active_document().map(|d| d.path.clone()),
            Some(created.clone())
        );
        assert!(
            space
                .rows()
                .iter()
                .any(|row| row.path == created),
            "the parent folder was expanded so the new file is visible"
        );
    }

    #[test]
    fn creating_a_folder_expands_its_parent_and_updates_rows() {
        let scratch = Scratch::new("ws-create-folder");
        let dir = scratch.0.join("src");
        std::fs::create_dir(&dir).unwrap();
        let mut space = Workspace::new(scratch.0.clone());

        let created = space.create_folder(&dir, "nested").unwrap();
        assert!(created.is_dir());
        assert!(
            space
                .rows()
                .iter()
                .any(|row| row.path == created)
        );
    }

    #[test]
    fn renaming_an_open_dirty_file_preserves_its_unsaved_edits_and_active_tab() {
        let scratch = Scratch::new("ws-rename-dirty");
        let old = scratch.file("a.rs", "original");
        let mut space = Workspace::new(scratch.0.clone());
        space.open(&old, notify()).unwrap();
        space.documents[0]
            .controller
            .set_text("unsaved edits");
        assert!(space.documents[0].is_dirty());

        let renamed = space
            .rename_path(&old, "b.rs", notify())
            .unwrap();
        assert_eq!(space.documents.len(), 1);
        let active = space.active_document().expect("still active");
        assert_eq!(active.path, renamed);
        assert_eq!(active.name, "b.rs");
        assert_eq!(active.controller.value().text(), "unsaved edits");
        assert!(
            active.is_dirty(),
            "unsaved state is preserved across rename"
        );
    }

    #[test]
    fn renaming_a_folder_remaps_open_files_and_expanded_paths_inside_it() {
        let scratch = Scratch::new("ws-rename-folder");
        let file = scratch.file("pkg/sub/mod.rs", "pub fn hi() {}");
        let pkg = scratch.0.join("pkg");
        let sub = pkg.join("sub");
        let mut space = Workspace::new(scratch.0.clone());
        space.expand(&sub);
        space.open(&file, notify()).unwrap();

        let renamed_pkg = space
            .rename_path(&pkg, "core", notify())
            .unwrap();
        let expected_file = renamed_pkg.join("sub/mod.rs");
        assert_eq!(
            space.active_document().map(|d| d.path.clone()),
            Some(expected_file.clone())
        );
        assert!(
            space
                .rows()
                .iter()
                .any(|row| row.path == expected_file),
            "expanded folders inside the renamed folder stay expanded"
        );
    }

    #[test]
    fn deleting_a_folder_closes_open_files_inside_it_and_clears_clipboard() {
        let scratch = Scratch::new("ws-delete-folder");
        let file = scratch.file("temp/a.rs", "fn a() {}");
        let folder = scratch.0.join("temp");
        let mut space = Workspace::new(scratch.0.clone());
        space.open(&file, notify()).unwrap();
        space.copy_to_clipboard(vec![file.clone()], true);

        space.delete_path(&folder).unwrap();
        assert!(!folder.exists());
        assert!(space.documents.is_empty());
        assert!(space.active.is_none());
        assert!(!space.can_paste());
    }

    #[test]
    fn cut_and_paste_moves_file_updates_open_tab_and_clears_clipboard() {
        let scratch = Scratch::new("ws-cut-paste");
        let file = scratch.file("a.rs", "hello");
        let dest = scratch.0.join("dest");
        std::fs::create_dir(&dest).unwrap();
        let mut space = Workspace::new(scratch.0.clone());
        space.open(&file, notify()).unwrap();

        space.copy_to_clipboard(vec![file.clone()], true);
        assert!(space.is_cut(&file));

        let moved = space.paste_into(&dest, notify()).unwrap();
        assert_eq!(moved, vec![dest.join("a.rs")]);
        assert!(!file.exists());
        assert!(!space.can_paste(), "cut clears clipboard after paste");
        assert_eq!(
            space.active_document().map(|d| d.path.clone()),
            Some(dest.join("a.rs"))
        );
    }

    #[test]
    fn copy_and_paste_duplicates_file_and_keeps_clipboard_for_repeated_pastes() {
        let scratch = Scratch::new("ws-copy-paste");
        let file = scratch.file("a.rs", "hello");
        let mut space = Workspace::new(scratch.0.clone());

        space.copy_to_clipboard(vec![file.clone()], false);
        assert!(!space.is_cut(&file));

        let first = space.paste_into(&scratch.0, notify()).unwrap();
        assert_eq!(first[0].file_name().unwrap(), "a copy.rs");
        assert!(space.can_paste(), "copy keeps clipboard for further pastes");

        let second = space.paste_into(&scratch.0, notify()).unwrap();
        assert_eq!(second[0].file_name().unwrap(), "a copy 2.rs");
    }

    #[test]
    fn multi_selection_supports_toggle_range_and_bulk_move_and_delete() {
        let scratch = Scratch::new("ws-multi-select");
        let a = scratch.file("a.rs", "a");
        let b = scratch.file("b.rs", "b");
        let c = scratch.file("c.rs", "c");
        let dest = scratch.0.join("dest");
        std::fs::create_dir(&dest).unwrap();
        let mut space = Workspace::new(scratch.0.clone());

        space.select_entry(a.clone(), false);
        space.select_range_to(c.clone(), false);
        assert!(space.is_selected(&a) && space.is_selected(&b) && space.is_selected(&c));

        space.toggle_select_entry(b.clone(), false);
        assert!(!space.is_selected(&b));
        assert_eq!(space.selected_entries.len(), 2);

        // Right-clicking an already-selected item preserves the multi-selection.
        space.ensure_selected_for_context_menu(a.clone(), false);
        assert_eq!(space.selected_entries.len(), 2);

        let moved = space
            .move_paths_to(&[a.clone(), c.clone()], &dest, notify())
            .unwrap();
        assert_eq!(moved, vec![dest.join("a.rs"), dest.join("c.rs")]);
        assert!(!a.exists() && !c.exists());
        assert!(dest.join("a.rs").is_file() && dest.join("c.rs").is_file());

        space.delete_paths(&moved).unwrap();
        assert!(!dest.join("a.rs").exists() && !dest.join("c.rs").exists());

        space.select_entry(b.clone(), false);
        assert!(space.is_selected(&b));
        space.clear_selection();
        assert!(!space.is_selected(&b));
        assert!(space.selected_entries.is_empty());
        assert!(space.selected_entry.is_none());
    }

    #[test]
    fn keyboard_navigation_moves_expands_collapses_and_extends_selection() {
        let scratch = Scratch::new("ws-kbd-nav");
        let main_rs = scratch.file("src/main.rs", "fn main() {}");
        let lib_rs = scratch.file("src/lib.rs", "");
        let readme = scratch.file("README.md", "# hi");
        let src_dir = scratch.0.join("src");
        let mut space = Workspace::new(scratch.0.clone());

        // Rows initially: [src (dir), README.md]
        assert_eq!(space.navigate_tree(1, false), Some(0));
        assert!(space.is_selected(&src_dir));

        // Right arrow expands collapsed `src` folder.
        assert_eq!(space.tree_expand_or_descend(), Some(0));
        assert_eq!(space.rows().len(), 4);

        // Right arrow again descends into first child (`lib.rs`).
        assert_eq!(space.tree_expand_or_descend(), Some(1));
        assert!(space.is_selected(&lib_rs));

        // Shift+Down extends selection to `main.rs`.
        assert_eq!(space.navigate_tree(1, true), Some(2));
        assert!(space.is_selected(&lib_rs) && space.is_selected(&main_rs));
        assert!(!space.is_selected(&readme));

        // Left arrow on file ascends to parent `src` folder.
        assert_eq!(space.tree_collapse_or_ascend(), Some(0));
        assert!(space.is_selected(&src_dir));
        assert_eq!(space.selected_entries.len(), 1);

        // Left arrow on expanded `src` folder collapses it.
        assert_eq!(space.tree_collapse_or_ascend(), Some(0));
        assert_eq!(space.rows().len(), 2);

        // End jumps to last row (`README.md`); Enter opens it.
        assert_eq!(space.navigate_tree_edge(true, false), Some(1));
        assert!(space.is_selected(&readme));
        space.activate_selected_tree_row(notify(), false);
        assert_eq!(
            space.active_document().map(|d| d.path.clone()),
            Some(readme)
        );

        // Select all selects every visible row.
        space.select_all_tree();
        assert_eq!(space.selected_entries.len(), 2);
    }

    #[test]
    fn move_path_to_relocates_open_file_expands_destination_and_updates_rows() {
        let scratch = Scratch::new("ws-move-to");
        let file = scratch.file("a.rs", "fn a() {}");
        let dest = scratch.0.join("src");
        std::fs::create_dir(&dest).unwrap();
        let mut space = Workspace::new(scratch.0.clone());
        space.open(&file, notify()).unwrap();
        space.documents[0]
            .controller
            .set_text("fn edited() {}");

        let moved = space
            .move_path_to(&file, &dest, notify())
            .unwrap();
        assert_eq!(moved, dest.join("a.rs"));
        assert!(!file.exists());
        let active = space.active_document().expect("still active");
        assert_eq!(active.path, moved);
        assert_eq!(active.controller.value().text(), "fn edited() {}");
        assert!(
            active.is_dirty(),
            "unsaved changes survive drag-and-drop move"
        );
        assert!(
            space.rows().iter().any(|row| row.path == moved),
            "destination folder expanded so moved entry is visible"
        );
    }

    #[test]
    fn step_tab_cycles_through_open_documents_and_settings_and_close_active_tab_closes_shown() {
        let scratch = Scratch::new("ws-step-tabs");
        let a = scratch.file("a.rs", "a");
        let b = scratch.file("b.rs", "b");
        let mut space = Workspace::new(scratch.0.clone());
        space.open(&a, notify()).unwrap();
        space.open(&b, notify()).unwrap();
        space.open_settings();

        let id_a = space.documents[0].id;
        let id_b = space.documents[1].id;
        assert_eq!(space.active, Some(Workspace::SETTINGS_TAB));

        // Stepping forward wraps from Settings -> a.rs -> b.rs -> Settings.
        space.step_tab(1);
        assert_eq!(space.active, Some(id_a));
        space.step_tab(1);
        assert_eq!(space.active, Some(id_b));
        space.step_tab(-1);
        assert_eq!(space.active, Some(id_a));
        space.step_tab(-1);
        assert_eq!(space.active, Some(Workspace::SETTINGS_TAB));

        // Closing active tab closes Settings, then b.rs.
        space.close_active_tab();
        assert!(!space.settings_open);
        assert_eq!(space.active, Some(id_b));
        space.close_active_tab();
        assert_eq!(space.documents.len(), 1);
        assert_eq!(space.active, Some(id_a));
    }
}
