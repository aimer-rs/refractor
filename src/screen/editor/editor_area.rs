use super::breadcrumb::Breadcrumb;
use super::pane::EditorPane;
use super::tab_bar::TabBar;
use crate::document::Document;
use crate::fonts;
use crate::theme::CodeTheme;
use crate::workspace::Workspace;
use aimer::style::{LayoutSpacing, TextAlign, Theme};
use aimer::{
    AnyWidget, BuildContext, Column, Container, Dimension, Expanded, ProviderContext, SizedBox,
    StatelessWidget, Text, Widget,
};
use std::path::PathBuf;
use std::rc::Rc;

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
        let mut column = vec![TabBar.boxed()];
        let body: AnyWidget = match shown {
            Some((document, path)) => {
                column.push(Breadcrumb::new(Rc::clone(&document), path).boxed());
                EditorPane::new(document).notype()
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
                .notype(),
        };
        column.push(
            Expanded::new()
                .child(body)
                .boxed(),
        );
        Container::new()
            .color(theme.editor)
            .child(Column::new().children(column))
    }
}
