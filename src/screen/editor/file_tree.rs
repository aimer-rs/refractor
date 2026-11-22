use super::file_row::FileRow;
use crate::components::pad::pad;
use crate::fonts;
use crate::theme::CodeTheme;
use crate::tree::Row;
use crate::workspace::Workspace;
use aimer::share::{RcProjectExt, ShareRef, Shared};
use aimer::style::{TextAlign, Theme};
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
        let rows: Vec<_> = (0..rows.len())
            .into_iter()
            .map(move |i| rows.project(move |r| &r[i]))
            .collect();
        let top_pad = if rows.is_empty() { 8 } else { 2 };

        let child = if rows.is_empty() {
            Text::new("This folder is empty")
                .text_align(TextAlign::TopCenter)
                .text_style(fonts::ui().font_size(13).color(theme.muted))
                .boxed()
        } else {
            Column::new()
                .list(rows)
                .builder(move |row| FileRow::new(row.clone()))
                .boxed()
        };

        Scrollable::new()
            .vertical_scroll_bar(None)
            .child(
                Container::new()
                    .padding(pad(top_pad, 0, 12, 0))
                    .child(child),
            )
    }
}
