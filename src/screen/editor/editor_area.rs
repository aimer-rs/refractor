use super::breadcrumb::Breadcrumb;
use super::lsp_badge::{LspBadge, RIGHT as BADGE_RIGHT, TOP as BADGE_TOP};
use super::pane::EditorPane;
use super::problems_panel::{PANEL_HEIGHT, PANEL_MAX, PANEL_MIN, ProblemsPanel};
use super::tab_bar::TabBar;
use crate::document::Document;
use crate::fonts;
use crate::theme::CodeTheme;
use crate::workspace::Workspace;
use aimer::style::{LayoutSpacing, TextAlign, Theme};
use aimer::{
    AnyWidget, BuildContext, Column, Container, Dimension, Direction, Expanded, Positioned,
    ProviderContext, Resizable, SizedBox, Stack, StatelessWidget, Text, Widget,
};
use std::path::PathBuf;
use std::rc::Rc;

/// How far the language server's mark floats from the top and the right of the editor, in logical pixels.

/// The right side: the tabs of the open files, and the one being edited under them with where it is,
/// or a note that nothing is open.
///
/// Only which document is shown is watched here, so typing does not rebuild it. The text and the
/// caret are the document's, and the text area follows them itself.
#[derive(Clone, StatelessWidget)]
pub struct EditorArea;

impl StatelessWidget for EditorArea {
    fn build(&self, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let shown = ctx.select::<Workspace, Option<(Rc<Document>, PathBuf)>>(|space| {
            space
                .active_document()
                .map(|document| (Rc::clone(document), space.relative(&document.path)))
        });
        let problems_open = ctx.select::<Workspace, bool>(|space| space.problems_open);
        let mut column = vec![TabBar.boxed()];
        let body: AnyWidget = match shown {
            Some((document, path)) => {
                column.push(Breadcrumb::new(Rc::clone(&document), path).boxed());
                // The mark floats over the corner of the text and takes no room from it.
                Stack::new()
                    .add_child(EditorPane::new(document))
                    .add_child(
                        Positioned::new()
                            .top(BADGE_TOP)
                            .right(BADGE_RIGHT)
                            .child(LspBadge),
                    )
                    .notype()
            }
            None => SizedBox::new()
                .width(Dimension::Percent(100.0))
                .height(Dimension::Percent(100.0))
                .box_child(
                    Container::new()
                        .padding(LayoutSpacing::all(40))
                        .child(
                            Text::new("Open a file from the sidebar")
                                .text_align(TextAlign::MidCenter)
                                .text_style(
                                    fonts::ui()
                                        .font_size(14)
                                        .color(theme.muted),
                                ),
                        ),
                )
        };
        column.push(
            Expanded::new()
                .child(body)
                .boxed(),
        );
        if problems_open {
            // The third panel, under the editor, whose top edge is dragged to make it taller or shorter.
            column.push(
                Container::new()
                    .color(theme.editor)
                    .child(
                        Resizable::new()
                            .height(PANEL_HEIGHT)
                            .width(f32::MAX)
                            .min_height(PANEL_MIN)
                            .max_height(PANEL_MAX)
                            .handle_thickness(10.0)
                            .handle_outset(4.0)
                            .direction(Direction::TOP)
                            .child(ProblemsPanel),
                    )
                    .boxed(),
            );
        }
        Container::new()
            .color(theme.editor)
            .child(Column::new().children(column))
    }
}
