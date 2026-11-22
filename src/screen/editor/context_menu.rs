//! The rows the editor adds to the menu of a right click: the actions of a problem before the clipboard verbs, and finding the usages of a
//! name, going to where it is defined and renaming it, after them. Which rows there are is [`crate::editor_menu`]'s to say; this does what
//! each of them is for.

use crate::components::marker::tinted;
use crate::components::popup::Popup;
use crate::editor_menu::{Action, Facts, Layout, Row, layout, name_of};
use crate::fonts;
use crate::hover::{CardSource, HoverCell};
use crate::problem_lines::ProblemLines;
use crate::rename::RenamePrompt;
use crate::syntax::SyntaxHighlighter;
use crate::theme::CodeTheme;
use crate::usages::Usages;
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
}
