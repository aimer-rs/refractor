use super::tab::{HEIGHT, Tab};
use crate::components::rule;
use crate::theme::CodeTheme;
use crate::workspace::Workspace;
use aimer::style::Theme;
use aimer::{
    BuildContext, Column, Container, Dimension, ProviderContext, Row, ScrollBehavior, Scrollable,
    SizedBox, StatelessWidget, Widget,
};
use std::path::PathBuf;

/// The open files, and the settings if they are open, side by side along the top of the editor, and a line under them.
///
/// Only which files are open, and which is shown, are watched here: a tab watches its own changes.
#[derive(Clone, StatelessWidget)]
pub struct TabBar;

impl StatelessWidget for TabBar {
    fn build(&self, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let tabs = ctx.select::<Workspace, Vec<(u64, String, PathBuf, bool)>>(|space| {
            space
                .documents
                .iter()
                .map(|document| {
                    (
                        document.id,
                        document.name.clone(),
                        document.path.clone(),
                        space.active == Some(document.id),
                    )
                })
                .collect()
        });
        // The settings are the last tab, when they are open: `Some` of whether they are shown.
        let settings = ctx.select::<Workspace, Option<bool>>(|space| {
            space
                .settings_open
                .then(|| space.settings_active())
        });
        // A scrollable takes all the height it is given, so it is given the height of a tab.
        let row = SizedBox::new()
            .width(Dimension::Percent(100.0))
            .height(HEIGHT)
            .box_child(
                Container::new().color(theme.bar).child(
                    Scrollable::new()
                        .axis(aimer::ScrollAxis::Horizontal)
                        .horizontal_scroll_bar(None)
                        .scroll_behavior(ScrollBehavior::no_bounce())
                        .child(
                            Row::new().children(
                                tabs.into_iter()
                                    .map(|(id, name, path, active)| {
                                        Tab::new(id, name, path, active).boxed()
                                    })
                                    .chain(settings.map(|active| Tab::settings(active).boxed()))
                                    .collect::<Vec<_>>(),
                            ),
                        ),
                ),
            );
        Column::new().children([row.boxed(), rule::horizontal(theme.divider).boxed()])
    }
}
