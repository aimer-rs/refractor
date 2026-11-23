mod breadcrumb;
mod completion_caret;
mod context_menu;
mod definition;
mod editor_area;
mod file_row;
mod file_tree;
mod git_panel;
mod git_tree_row;
mod go_to_line_card;
mod hover_card;
mod lsp_badge;
mod pane;
mod param_hint_bubble;
mod pointer_track;
mod problem_section;
mod problems_panel;
mod progress_bar;
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
mod tab_drag;
mod tree_drag;
mod tree_prompt_card;
mod tree_view;
mod usage_row;
mod usages_card;

use self::editor_area::EditorArea;
use self::go_to_line_card::GoToLineCard;
use self::search_layer::SearchLayer;
use self::side_panel::{SidePanel, edge};
use self::status_bar::StatusBar;
use self::tree_prompt_card::TreePromptCard;
use crate::services::settings::{AutoSave, Settings};
use crate::ui::theme::CodeTheme;
use crate::model::workspace::{
    Workspace, open_git_commit, open_git_push, refresh_status, save_document,
    schedule_clear_file_activity, toggle_git_panel,
};
use aimer::input::gesture::GestureEvent;
use aimer::input::gesture::gesture_detector::GestureDetector;
use aimer::style::Theme;
use aimer::{
    BuildContext, Column, Container, Dimension, Direction, Expanded, ProviderContext,
    ProviderHandle, Resizable, Row, Shortcuts, SizedBox, Stack, StatelessWidget, Widget,
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
                    let mut seq = None;
                    reporting.update(|space| {
                        space.report_external(document, &found);
                        if matches!(found, crate::services::file_sync::Synced::Reloaded) {
                            seq = space.file_activity().map(|activity| activity.seq);
                        }
                    });
                    if let Some(seq) = seq {
                        schedule_clear_file_activity(&reporting, seq);
                    }
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
                                .child({
                                    let clear_store = store.clone();
                                    GestureDetector::new()
                                        .on_gesture(move |event: GestureEvent| {
                                            if matches!(event, GestureEvent::TapDown { .. })
                                                && !clear_store.read().selected_entries.is_empty()
                                            {
                                                clear_store.update(|space| space.clear_selection());
                                            }
                                        })
                                        .child(
                                            SizedBox::new()
                                                .height(Dimension::Percent(100.0))
                                                .box_child(EditorArea),
                                        )
                                })
                                .boxed(),
                        ]),
                    )
                    .boxed(),
                {
                    let clear_store = store.clone();
                    GestureDetector::new()
                        .on_gesture(move |event: GestureEvent| {
                            if matches!(event, GestureEvent::TapDown { .. })
                                && !clear_store.read().selected_entries.is_empty()
                            {
                                clear_store.update(|space| space.clear_selection());
                            }
                        })
                        .child(StatusBar)
                        .boxed()
                },
            ]),
        );
        let (tree_prompt, go_to_line) = {
            let space = store.read();
            (space.tree_prompt(), space.go_to_line_prompt())
        };
        let layered = Stack::new()
            .add_child(page)
            .add_child(TreePromptCard::new(tree_prompt))
            .add_child(GoToLineCard::new(go_to_line));
        let esc_store = store.clone();
        let git_tab_store = store.clone();
        let commit_store = store.clone();
        let push_store = store.clone();
        // Window-wide, so saving, toggling the git sidebar (CmdOrCtrl+2), commit/push, and clearing tree selection work wherever focus is.
        Shortcuts::new()
            .window(true)
            .bind_str("CmdOrCtrl+S", move || save_active(&store))
            .bind_str("CmdOrCtrl+2", move || toggle_git_panel(&git_tab_store))
            .bind_str("CmdOrCtrl+K", move || open_git_commit(&commit_store))
            .bind_str("CmdOrCtrl+Shift+K", move || open_git_push(&push_store))
            .bind_str("Escape", move || {
                if !esc_store.read().selected_entries.is_empty() {
                    esc_store.update(|space| space.clear_selection());
                }
            })
            .child(SearchLayer::new(layered))
    }
}
