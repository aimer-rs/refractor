use super::tab::{HEIGHT, Tab};
use super::tab_drag::{TabAutoScroll, TabBarViewport};
use crate::ui::components::rule;
use crate::ui::theme::CodeTheme;
use crate::model::workspace::Workspace;
use aimer::style::Theme;
use aimer::{
    BuildContext, Column, Container, Dimension, ProviderContext, Row, ScrollBehavior,
    ScrollController, Scrollable, SizedBox, State, StateView, StatefulWidget, Widget,
};
use std::path::PathBuf;
use std::rc::Rc;

/// The open files, and the settings if they are open, side by side along the top of the editor, and a line under them.
///
/// Only which files are open, and which is shown, are watched here: a tab watches its own changes.
#[derive(Clone, StatefulWidget)]
pub struct TabBar;

struct TabBarState {
    scroll: ScrollController,
    auto_scroll: Rc<TabAutoScroll>,
}

impl State<TabBar> for TabBarState {
    fn create(_widget: TabBar) -> Self {
        let scroll = ScrollController::new();
        let auto_scroll = Rc::new(TabAutoScroll::new(scroll.clone()));
        Self {
            scroll,
            auto_scroll,
        }
    }

    fn adopt_config_from(&mut self, _new: Self) {}

    fn build(this: StateView<Self>, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let tabs = ctx.select::<Workspace, Vec<(u64, String, PathBuf, bool, bool)>>(|space| {
            space
                .documents
                .iter()
                .map(|document| {
                    (
                        document.id,
                        document.name.clone(),
                        document.path.clone(),
                        space.active == Some(document.id),
                        space.is_pinned(document.id),
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
        let auto_scroll = Rc::clone(&this.auto_scroll);
        // A scrollable takes all the height it is given, so it is given the height of a tab.
        let row = SizedBox::new()
            .width(Dimension::Percent(100.0))
            .height(HEIGHT)
            .box_child(
                Container::new()
                    .color(theme.bar)
                    .child(TabBarViewport::new(
                        this.auto_scroll.bounds(),
                        Scrollable::new()
                            .controller(this.scroll.clone())
                            .axis(aimer::ScrollAxis::Horizontal)
                            .horizontal_scroll_bar(None)
                            .scroll_behavior(ScrollBehavior::no_bounce())
                            .child(
                                Row::new().children(
                                    tabs.into_iter()
                                        .enumerate()
                                        .map(|(index, (id, name, path, active, pinned))| {
                                            Tab::new(
                                                id,
                                                index,
                                                name,
                                                path,
                                                active,
                                                pinned,
                                                Rc::clone(&auto_scroll),
                                            )
                                            .boxed()
                                        })
                                        .chain(settings.map(|active| Tab::settings(active).boxed()))
                                        .collect::<Vec<_>>(),
                                ),
                            ),
                    )),
            );
        Column::new().children([row.boxed(), rule::horizontal(theme.divider).boxed()])
    }
}
