mod breadcrumb;
mod completion_caret;
mod context_menu;
mod definition;
mod editor_area;
mod file_row;
mod file_tree;
mod hover_card;
mod lsp_badge;
mod pane;
mod problem_section;
mod problems_panel;
mod rename_card;
mod search_go;
mod search_layer;
mod search_popup;
mod search_preview;
mod search_row;
mod search_tabs;
mod side_panel;
mod spinner;
mod status_bar;
mod tab;
mod tab_bar;
mod usage_row;
mod usages_card;

use self::editor_area::EditorArea;
use self::search_layer::SearchLayer;
use self::side_panel::{SidePanel, edge};
use self::status_bar::StatusBar;
use crate::settings::{AutoSave, Settings};
use crate::theme::CodeTheme;
use crate::workspace::{Workspace, refresh_status, save_document};
use aimer::style::Theme;
use aimer::{
    BuildContext, Column, Container, Dimension, Direction, Expanded, ProviderContext,
    ProviderHandle, Resizable, Row, Shortcuts, SizedBox, StatelessWidget, Widget,
};

/// How wide the file tree starts, and the limits of dragging its right edge, in logical pixels.
const SIDE_PANEL_WIDTH: f32 = 250.0;
const SIDE_PANEL_MIN: f32 = 160.0;
const SIDE_PANEL_MAX: f32 = 560.0;

/// The whole window: the file tree on the left, the tabs and the editor beside it, and the status
/// bar along the bottom, with no title bar of its own: the window's buttons are drawn over the
/// corner of the tree.
#[derive(Clone, StatelessWidget)]
pub struct EditorScreen;

/// Saves the document being edited. The save is made here and not inside an update of the
/// workspace, because a document that becomes unchanged tells the workspace so, which is an update.
fn save_active(store: &ProviderHandle<Workspace>) {
    let Some(document) = store.read().active_document().cloned() else {
        return;
    };
    save_document(store, &document);
}

impl StatelessWidget for EditorScreen {
    fn build(&self, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let store = ProviderHandle::<Workspace>::of(ctx);
        // Files are saved by themselves as the settings say, with what the save key saves with.
        let (way, delay) = ctx.select::<Settings, (AutoSave, std::time::Duration)>(|settings| {
            (settings.auto_save(), settings.auto_save_delay())
        });
        let auto_save = store.read().auto_save();
        auto_save.configure(way, delay);
        let saving = store.clone();
        auto_save.set_saver(std::rc::Rc::new(move |document| {
            save_document(&saving, document)
        }));
        let listing = store.clone();
        auto_save.watch_window(std::rc::Rc::new(move || listing.read().documents.clone()));
        // What something else changes in an open file is brought into the editor, a moment after it is changed.
        {
            let (listing, reporting) = (store.clone(), store.clone());
            store.read().file_sync().start(
                std::rc::Rc::new(move || listing.read().documents.clone()),
                std::rc::Rc::new(move |document, found| {
                    reporting.update(|space| space.report_external(document, &found));
                    // What changed the file may have changed what git says of it.
                    refresh_status(&reporting);
                }),
            );
        }
        let page = Container::new().color(theme.editor).child(
            Column::new().children([
                Expanded::new()
                    .child(
                        Row::new().children([
                            // The resizable sits inside a container, as the framework's live-resize test has it: the
                            // pane beside it follows the drag on every frame. Its height is as much as the parent
                            // gives, which is the height of the window.
                            Container::new()
                                .color(theme.sidebar)
                                .child(
                                    Resizable::new()
                                        .width(SIDE_PANEL_WIDTH)
                                        .height(f32::MAX)
                                        .min_width(SIDE_PANEL_MIN)
                                        .max_width(SIDE_PANEL_MAX)
                                        .handle_thickness(14.0)
                                        .handle_outset(6.0)
                                        .direction(Direction::RIGHT)
                                        .child(SidePanel),
                                )
                                .boxed(),
                            edge(theme).boxed(),
                            Expanded::new()
                                .child(
                                    SizedBox::new()
                                        .height(Dimension::Percent(100.0))
                                        .box_child(EditorArea),
                                )
                                .boxed(),
                        ]),
                    )
                    .boxed(),
                StatusBar.boxed(),
            ]),
        );
        // Window-wide, so saving works wherever focus is.
        Shortcuts::new()
            .window(true)
            .bind_str("CmdOrCtrl+S", move || save_active(&store))
            .child(SearchLayer::new(page))
    }
}
