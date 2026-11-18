use super::file_row::FileRow;
use crate::components::pad::pad;
use crate::fonts;
use crate::theme::CodeTheme;
use crate::tree::Row;
use crate::workspace::Workspace;
use aimer::share::{ShareRef, Shared};
use aimer::style::Theme;
use aimer::{
    BuildContext, Column, Container, ProviderContext, Scrollable, StatelessWidget, Text, Widget,
};
use std::rc::Rc;

/// The folder's files, as a list of rows that scrolls.
#[derive(Clone, StatelessWidget)]
pub struct FileTree;

impl StatelessWidget for FileTree {
    fn build(&self, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        // The rows are made when the folder changes, and shared: this is a handle to them.
        let rows = ctx.select::<Workspace, Rc<Vec<Row>>>(|space| space.rows());
        let items: Vec<_> = if rows.is_empty() {
            vec![
                Text::new("This folder is empty")
                    .text_style(
                        fonts::ui()
                            .font_size(13)
                            .color(theme.muted),
                    )
                    .boxed(),
            ]
        } else {
            rows.iter()
                .map(|row| FileRow::new(ShareRef::from(Shared::new(row.clone()))).boxed())
                .collect()
        };
        Scrollable::new()
            .vertical_scroll_bar(None)
            .child(
                Container::new()
                    .padding(pad(2, 0, 12, 0))
                    .child(Column::new().children(items)),
            )
    }
}
