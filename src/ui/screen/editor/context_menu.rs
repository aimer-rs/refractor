//! The rows the editor adds to the menu of a right click: the actions of a problem before the clipboard verbs, and finding the usages of a
//! name, going to where it is defined and renaming it, after them. Which rows there are is [`crate::ui::editor_menu`]'s to say; this does what
//! each of them is for.

use crate::ui::components::marker::tinted;
use crate::ui::components::popup::Popup;
use crate::ui::editor_menu::{Action, Facts, Layout, Row, layout, name_of};
use crate::ui::fonts;
use crate::ui::hover::{CardSource, HoverCell};
use crate::ui::problem_lines::ProblemLines;
use crate::ui::rename::RenamePrompt;
use crate::model::syntax::SyntaxHighlighter;
use crate::ui::theme::CodeTheme;
use crate::ui::usages::Usages;
use aimer::input::TextEditingController;
use aimer::input::input::{ContextMenuExtras, ContextMenuRequest, FieldSpot, HoverSpot};
use aimer::style::{LayoutSpacing, Spacing};
use aimer::{ContextMenuItem, ContextMenuStyle};
use std::ops::Range;
use std::rc::Rc;

/// How tall a line is taken to be for what hangs from the pointer: the card of a problem is put below the line it was asked on.
const LINE: f32 = 18.0;

/// The look of the menu: the panel and the rows are the editor's, as the cards that hang from a name are, and the row under the pointer is
/// marked in the pink of the app.
pub fn style(theme: CodeTheme) -> ContextMenuStyle {
    ContextMenuStyle::list()
        .panel(Popup::frame(theme))
        .label(
            fonts::ui()
                .font_size(Dense::FONT)
                .color(theme.text),
        )
        .disabled_label_color(theme.muted)
        .hint_color(theme.muted)
        .highlight_color(theme.marker.with_alpha(Popup::ROW_HIGHLIGHT))
        .separator_color(theme.divider)
        .highlight_radius(Dense::HIGHLIGHT_RADIUS)
        .highlight_inset(Dense::HIGHLIGHT_INSET)
        .padding(LayoutSpacing::all(Spacing::Px(Dense::PANEL_PADDING)))
        .row_height(Dense::ROW_HEIGHT)
        .item_padding(Dense::ITEM_PADDING)
        .min_width(Dense::MIN_WIDTH)
        .hint_gap(Dense::HINT_GAP)
        .separator_gap(Dense::SEPARATOR_GAP)
        .icon_size(Dense::ICON)
        .icon_gap(Dense::ICON_GAP)
}

/// How much room the menu takes, which is as little as the cards that hang from a name take: its rows are as tall as the rows of the list of
/// usages, with its text, and not as tall as a menu of the system is.
pub struct Dense;

impl Dense {
    pub const FONT: u32 = 13;
    pub const ROW_HEIGHT: f32 = 22.0;
    pub const ITEM_PADDING: f32 = 10.0;
    pub const MIN_WIDTH: f32 = 190.0;
    pub const HINT_GAP: f32 = 20.0;
    pub const SEPARATOR_GAP: f32 = 7.0;
    pub const PANEL_PADDING: u32 = 8;
    pub const ICON: f32 = 14.0;
    pub const ICON_GAP: f32 = 8.0;
    /// The wash under the row at the pointer is a rounded one, kept in from the edge of the panel by about its radius.
    pub const HIGHLIGHT_RADIUS: f32 = 4.0;
    pub const HIGHLIGHT_INSET: f32 = 4.0;
}

/// The rows of a file tree menu, styled for `theme`, with `selection_count` saying how many entries are targeted (0 for blank space,
/// 1 for a single row, >1 for a multi-selection) and `can_paste` saying whether the file clipboard holds an entry.
pub fn tree_items(
    theme: CodeTheme,
    selection_count: usize,
    can_paste: bool,
    on_action: Rc<dyn Fn(crate::ui::tree_menu::Action)>,
) -> Vec<ContextMenuItem> {
    let mac = cfg!(any(target_os = "macos", target_os = "ios"));
    crate::ui::tree_menu::layout(selection_count, can_paste)
        .into_iter()
        .map(|row| {
            let color = if row.enabled { theme.text } else { theme.muted };
            let run = Rc::clone(&on_action);
            let mut item = ContextMenuItem::new(row.action.label(mac))
                .enabled(row.enabled)
                .on_select(move || run(row.action));
            if let Some(icon) = row.action.icon() {
                item = item.icon(tinted(icon, color).scene().clone());
            }
            if row.separated {
                item = item.separated();
            }
            if let Some(hint) = row.action.hint(mac) {
                item = item.hint(hint);
            }
            item
        })
        .collect()
}

/// Opens the file tree's context menu at `at`, for `target` (`Some((path, is_dir))` on a row, or `None` on blank space).
pub fn show_tree_menu(
    theme: CodeTheme,
    at: aimer::Vec2d,
    target: Option<(std::path::PathBuf, bool)>,
    store: aimer::ProviderHandle<crate::model::workspace::Workspace>,
) {
    let (selection_count, can_paste) = {
        let space = store.read();
        let count = match &target {
            Some((path, _)) if space.is_selected(path) => space.selected_entries.len().max(1),
            Some(_) => 1,
            None => 0,
        };
        (count, space.can_paste())
    };
    let on_action: Rc<dyn Fn(crate::ui::tree_menu::Action)> = Rc::new(move |action| {
        crate::model::workspace::run_tree_action(action, target.clone(), &store);
    });
    let _ = aimer::ContextMenu::new()
        .shape(aimer::ContextMenuShape::List)
        .style(style(theme))
        .at(at)
        .items(tree_items(theme, selection_count, can_paste, on_action))
        .show();
}

/// The rows of a tab's context menu, styled for `theme`, with `facts` describing the clicked tab and the rest of the tab bar.
pub fn tab_items(
    theme: CodeTheme,
    facts: crate::ui::tab_menu::Facts,
    on_action: Rc<dyn Fn(crate::ui::tab_menu::Action)>,
) -> Vec<ContextMenuItem> {
    let mac = cfg!(any(target_os = "macos", target_os = "ios"));
    crate::ui::tab_menu::layout(facts)
        .into_iter()
        .map(|row| {
            let color = if row.enabled { theme.text } else { theme.muted };
            let run = Rc::clone(&on_action);
            let mut item = ContextMenuItem::new(row.action.label(facts.pinned))
                .enabled(row.enabled)
                .on_select(move || run(row.action));
            if let Some(icon) = row.action.icon(facts.pinned) {
                item = item.icon(tinted(icon, color).scene().clone());
            }
            if row.separated {
                item = item.separated();
            }
            if let Some(hint) = row.action.hint(mac) {
                item = item.hint(hint);
            }
            item
        })
        .collect()
}

/// Opens a tab's context menu at `at`, targeting the tab `id`.
pub fn show_tab_menu(
    theme: CodeTheme,
    at: aimer::Vec2d,
    id: u64,
    store: aimer::ProviderHandle<crate::model::workspace::Workspace>,
) {
    let facts = store.read().tab_menu_facts(id);
    let on_action: Rc<dyn Fn(crate::ui::tab_menu::Action)> = Rc::new(move |action| {
        store.update(|space| space.run_tab_action(action, id));
    });
    let _ = aimer::ContextMenu::new()
        .shape(aimer::ContextMenuShape::List)
        .style(style(theme))
        .at(at)
        .items(tab_items(theme, facts, on_action))
        .show();
}

/// The rows of the `#[cfg(...)]` rustup target menu, styled for `theme`.
pub fn target_items(
    theme: CodeTheme,
    targets: Vec<String>,
    host: Option<&str>,
    active: Option<&str>,
    on_select: Rc<dyn Fn(Option<String>)>,
) -> Vec<ContextMenuItem> {
    if targets.is_empty() {
        return vec![
            ContextMenuItem::new("No installed rustup targets")
                .enabled(false)
                .icon(tinted(aimer::Icons::CPU, theme.muted).scene().clone()),
        ];
    }
    let effective = active.or(host);
    targets
        .into_iter()
        .map(|triple| {
            let is_default = host == Some(triple.as_str());
            let selected = effective == Some(triple.as_str());
            let (icon, color) = if selected {
                (aimer::Icons::CHECK, theme.marker)
            } else {
                (aimer::Icons::CPU, theme.muted)
            };
            let run = Rc::clone(&on_select);
            let chosen = (!is_default).then(|| triple.clone());
            let mut item = ContextMenuItem::new(triple)
                .icon(tinted(icon, color).scene().clone())
                .on_select(move || run(chosen.clone()));
            if is_default {
                item = item.hint("default");
            }
            item
        })
        .collect()
}

/// Opens a menu listing the installed `rustup` targets at `at` so the user can switch the target
/// used by `rust-analyzer` to evaluate `#[cfg(...)]` attributes.
pub fn show_target_menu(
    theme: CodeTheme,
    at: aimer::Vec2d,
    store: aimer::ProviderHandle<crate::model::workspace::Workspace>,
) {
    let (root, lsp) = {
        let space = store.read();
        (space.root.clone(), space.lsp())
    };
    let targets = crate::services::cfg_target::installed_targets(&root);
    let host = crate::services::cfg_target::host_target(&root);
    let active = lsp.target();
    let on_select: Rc<dyn Fn(Option<String>)> = Rc::new(move |target| {
        lsp.set_target(target);
    });
    let _ = aimer::ContextMenu::new()
        .shape(aimer::ContextMenuShape::List)
        .style(style(theme))
        .at(at)
        .items(target_items(
            theme,
            targets,
            host.as_deref(),
            active.as_deref(),
            on_select,
        ))
        .show();
}

/// What the rows are made of and do their work with.
pub struct EditorMenu {
    pub theme: CodeTheme,
    pub syntax: SyntaxHighlighter,
    pub controller: TextEditingController,
    pub problems: Rc<ProblemLines>,
    pub hover: Rc<HoverCell>,
    pub source: Rc<CardSource>,
    pub usages: Rc<Usages>,
    pub prompt: Rc<RenamePrompt>,
    /// What a click on a name does: goes to where it is defined, or lists who uses it where it is the definition.
    pub go: Rc<dyn Fn(usize, FieldSpot)>,
}

impl EditorMenu {
    /// What a menu opened as `request` says is added to it.
    pub fn extras(self: &Rc<Self>, request: &ContextMenuRequest) -> ContextMenuExtras {
        let text = self.controller.snapshot();
        let name = name_of(
            &|byte| self.syntax.link_at(&text, byte),
            request.byte,
            &request.selection,
        );
        // Where there is a problem: under the pointer, where the selection starts, or at the name.
        let problem = [
            request.byte,
            Some(request.selection.start),
            name.as_ref().map(|name| name.start),
        ]
        .into_iter()
        .flatten()
        .find(|byte| !self.problems.at(*byte).is_empty());
        let Layout { before, after } = layout(&Facts {
            name: name.clone(),
            problem: problem.is_some(),
        });
        let item = |row: Row| {
            let (menu, name, spot) = (Rc::clone(self), name.clone(), request.spot.clone());
            let word = name
                .as_ref()
                .map(|name| text.slice(name.clone()).to_string());
            // A row that cannot be chosen has a picture as dim as its label.
            let color = if row.enabled {
                self.theme.text
            } else {
                self.theme.muted
            };
            let item = ContextMenuItem::new(row.action.label())
                .enabled(row.enabled)
                .icon(tinted(row.action.icon(), color).scene().clone())
                .on_select(move || {
                    menu.run(
                        row.action,
                        name.clone(),
                        word.clone(),
                        problem,
                        spot.clone(),
                    )
                });
            match row
                .action
                .hint(cfg!(any(target_os = "macos", target_os = "ios")))
            {
                Some(hint) => item.hint(hint),
                None => item,
            }
        };
        ContextMenuExtras {
            before: before.into_iter().map(&item).collect(),
            after: after.into_iter().map(&item).collect(),
        }
    }

    /// Does what `action` is for, to the `name` that the menu was about, which is `word`, or to the problem at the byte `problem`.
    fn run(
        &self,
        action: Action,
        name: Option<Range<usize>>,
        word: Option<String>,
        problem: Option<usize>,
        spot: FieldSpot,
    ) {
        match (action, name) {
            (Action::ShowContextActions, name) => {
                let Some(at) = problem.or(name.as_ref().map(|name| name.start)) else {
                    return;
                };
                self.show_card(at, spot);
            }
            (Action::FindUsages, Some(name)) => self.usages.find_usages_at(name.start, spot),
            (Action::GoToDefinition, Some(name)) => (self.go)(name.start, spot),
            (Action::Rename, Some(name)) => {
                self.prompt
                    .open(name.start, word.unwrap_or_default(), spot)
            }
            (_, None) => {}
        }
    }

    /// Shows the problem at the byte `at`, and what the server offers to do about it, in the card that a pointer resting there shows, with
    /// every fix listed where there are more than one.
    fn show_card(&self, at: usize, spot: FieldSpot) {
        let range = self
            .problems
            .at(at)
            .first()
            .map_or(at..at + 1, |found| found.range.clone());
        let hover_spot = HoverSpot {
            range,
            x: spot.x,
            y: spot.y,
            width: 0.0,
            height: LINE,
            extent: spot.extent,
        };
        let hover = Rc::clone(&self.hover);
        self.source.ask(
            hover_spot,
            Rc::new(move |card| {
                let Some(card) = card else {
                    return;
                };
                hover.show(card);
                // The fixes are what was asked for: all of them, and not the first alone.
                if hover.has_more_fixes() && !hover.is_expanded() {
                    hover.toggle_expanded();
                }
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_is_no_roomier_than_the_list_of_usages_that_hangs_from_the_same_name() {
        let style = style(CodeTheme::dark());
        assert_eq!(style.row_height, Dense::ROW_HEIGHT);
        // The list of usages has rows of 20 and text of 13: a menu with rows of 28, as the system's is, looked twice the size beside it.
        assert!(style.row_height <= 22.0, "{}", style.row_height);
        assert!(style.min_width <= 200.0, "{}", style.min_width);
        assert_eq!(style.label.font_size, 13, "the same text as the cards");
        assert!(
            style.separator_gap < style.row_height / 2.0,
            "a line between groups is a thin one"
        );
        assert!(
            style.icon_size < style.row_height,
            "an icon fits its row with room round it"
        );
    }

    #[test]
    fn the_wash_under_the_row_at_the_pointer_has_rounded_corners_and_stands_in_from_the_edge() {
        let style = style(CodeTheme::dark());
        assert_eq!((style.highlight_radius, style.highlight_inset), (4.0, 4.0));
        assert!(
            style.highlight_inset >= style.highlight_radius,
            "so that a corner is not cut by the edge of the panel"
        );
    }

    #[test]
    fn the_file_tree_menu_items_have_icons_hints_and_lines_between_their_groups() {
        let noop: Rc<dyn Fn(crate::ui::tree_menu::Action)> = Rc::new(|_| {});
        let on_row = tree_items(CodeTheme::dark(), 1, true, Rc::clone(&noop));
        assert_eq!(on_row.len(), crate::ui::tree_menu::Action::ALL.len());
        assert!(on_row.iter().all(|item| item.is_enabled()));
        let has_icon: Vec<bool> = on_row
            .iter()
            .map(|item| item.icon_scene().is_some())
            .collect();
        assert_eq!(
            has_icon,
            [false, false, false, false, false, false, true, true, true]
        );
        assert!(
            on_row
                .iter()
                .all(|item| item.hint_text().is_some())
        );
        let separated: Vec<&str> = on_row
            .iter()
            .filter(|item| item.has_separator())
            .map(|item| item.label())
            .collect();
        let show = if cfg!(any(target_os = "macos", target_os = "ios")) {
            "Show in Finder"
        } else {
            "Show in Explorer"
        };
        assert_eq!(separated, ["Cut", show, "Rename\u{2026}"]);

        let on_blank = tree_items(CodeTheme::dark(), 0, false, noop);
        let enabled: Vec<(&str, bool)> = on_blank
            .iter()
            .map(|item| (item.label(), item.is_enabled()))
            .collect();
        assert_eq!(
            enabled,
            [
                ("New File", true),
                ("New Folder", true),
                ("Cut", false),
                ("Copy", false),
                ("Paste", false),
                ("Copy Path", true),
                (show, true),
                ("Rename\u{2026}", false),
                ("Delete", false),
            ]
        );
    }

    #[test]
    fn the_tab_menu_items_have_icons_labels_and_a_line_before_pin() {
        let noop: Rc<dyn Fn(crate::ui::tab_menu::Action)> = Rc::new(|_| {});
        let items = tab_items(
            CodeTheme::dark(),
            crate::ui::tab_menu::Facts {
                pinned: false,
                can_pin: true,
                unpinned_tabs: 2,
                other_unpinned_tabs: 1,
            },
            Rc::clone(&noop),
        );
        let labels: Vec<&str> = items.iter().map(|item| item.label()).collect();
        assert_eq!(labels, ["Close", "Close All", "Close Other", "Pin"]);
        assert!(items.iter().all(|item| item.is_enabled()));
        let has_icon: Vec<bool> = items
            .iter()
            .map(|item| item.icon_scene().is_some())
            .collect();
        assert_eq!(has_icon, [true, false, false, true]);
        assert!(items[0].hint_text().is_some());
        let separated: Vec<&str> = items
            .iter()
            .filter(|item| item.has_separator())
            .map(|item| item.label())
            .collect();
        assert_eq!(separated, ["Pin"]);

        let pinned = tab_items(
            CodeTheme::dark(),
            crate::ui::tab_menu::Facts {
                pinned: true,
                can_pin: true,
                unpinned_tabs: 0,
                other_unpinned_tabs: 0,
            },
            noop,
        );
        assert_eq!(pinned[3].label(), "Unpin");
        assert!(!pinned[1].is_enabled());
        assert!(!pinned[2].is_enabled());
    }
}
