use super::context_menu::show_tree_menu;
use super::file_row::{FileRow, HEIGHT as ROW_HEIGHT};
use super::tree_drag::{DraggedEntries, accept_drop};
use crate::model::tree::Row;
use crate::model::workspace::{Workspace, notifier, run_tree_action};
use crate::services::tree_ops;
use crate::ui::components::pad::pad;
use crate::ui::fonts;
use crate::ui::screen::editor::side_panel::HEADER_HEIGHT;
use crate::ui::theme::CodeTheme;
use crate::ui::tree_menu::Action as TreeAction;
use aimer::decoration::Decoration;
use aimer::dnd::{DragTarget, DragTargetState};
use aimer::events::pointer::PointerButton;
use aimer::input::gesture::GestureEvent;
use aimer::input::gesture::gesture_detector::GestureDetector;
use aimer::style::{BorderRadius, TextAlign, Theme};
use aimer::{
    BuildContext, ChildBuilder, Color, Column, Container, Dimension, FocusNode, Focusable,
    NumberExt, ProviderContext, ProviderHandle, ScrollController, Scrollable, Shortcuts, SizedBox,
    Stack, State, StateView, StatefulWidget, Text, Vec2d, Widget, list,
};
use std::rc::Rc;

/// Bottom padding inside the scrollable tree so the root drop zone stays reachable below the last row.
const BOTTOM_PAD: u32 = 48;
/// Extra vertical space occupied by the 2px root drop indicator line and its 4px top margin.
const DROP_LINE_EXTENT: f32 = 6.0;
/// How many rows `PageUp` / `PageDown` step by.
const PAGE_STEP: isize = 10;

/// Scrolls `scroll` only when row `index` lies outside the visible viewport.
fn reveal_row(scroll: &ScrollController, index: usize, row_count: usize, top_pad: u32) {
    let row_h = ROW_HEIGHT as f32;
    let content_h =
        top_pad as f32 + row_count as f32 * row_h + DROP_LINE_EXTENT + BOTTOM_PAD as f32;
    let max_y = scroll.max_extent().y;
    if max_y <= 0.0 {
        return;
    }
    let viewport_h = (content_h - max_y).max(row_h);
    let top = top_pad as f32 + index as f32 * row_h;
    let bottom = top + row_h;
    let current = scroll.offset().y;
    if top < current {
        scroll.jump_to(Vec2d { x: 0.0, y: top });
    } else if bottom > current + viewport_h {
        scroll.jump_to(Vec2d {
            x: 0.0,
            y: (bottom - viewport_h).max(0.0),
        });
    }
}

/// Moves keyboard focus to the file tree's `Focusable` anchor after the current scroll-view event dispatch finishes, so the
/// scrollable's nested dispatcher does not swallow the pending focus request before the window dispatcher sees it.
pub(super) fn request_tree_focus(focus: &FocusNode) {
    if let Some(venus) = aimer::Venus::current() {
        let focus = focus.clone();
        venus.spawn(async move {
            focus.request_focus();
            aimer::events::window::request_animation_frame();
        });
    } else {
        focus.request_focus();
    }
}

/// The folder's files, as a scrollable, focusable list of rows with keyboard navigation.
#[derive(Clone, StatefulWidget)]
pub struct FileTree;

struct FileTreeState {
    scroll: ScrollController,
}

impl State<FileTree> for FileTreeState {
    fn create(_widget: FileTree) -> Self {
        Self {
            scroll: ScrollController::new(),
        }
    }

    fn adopt_config_from(&mut self, _new: Self) {}

    fn build(this: StateView<Self>, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let store = ProviderHandle::<Workspace>::of(ctx);
        let focus = store.read().tree_focus();
        // The rows are made when the folder changes, and shared: this is a handle to them.
        let rows = ctx.select::<Workspace, Rc<Vec<Row>>>(|space| space.rows());
        let top_pad = if rows.is_empty() { 8 } else { 2 };

        let child = if rows.is_empty() {
            Text::new("This folder is empty")
                .text_align(TextAlign::TopCenter)
                .text_style(fonts::ui().font_size(13).color(theme.muted))
                .boxed()
        } else {
            Column::new()
                .children(FileRow::from_rows(&rows, &focus))
                .boxed()
        };

        let root = store.read().root.clone();
        let check_root = root.clone();
        let drop_store = store.clone();
        // Clicking blank space focuses the tree and clears any selection; right-clicking also opens the menu for the workspace root.
        let blank_store = store.clone();
        let blank_focus = focus.clone();
        let blank = GestureDetector::new()
            .on_gesture(move |event: GestureEvent| {
                if let GestureEvent::Tap { pointer } = event {
                    request_tree_focus(&blank_focus);
                    if !blank_store.read().selected_entries.is_empty() {
                        blank_store.update(|space| space.clear_selection());
                    }
                    if pointer.button == PointerButton::Secondary {
                        show_tree_menu(theme, pointer.pos, None, blank_store.clone());
                    }
                }
            })
            .child(
                SizedBox::new()
                    .width(Dimension::Percent(100.0))
                    .height(Dimension::Percent(100.0)),
            );

        let child = ChildBuilder::from_widget(child);
        // The root drop target sits inside `Scrollable` so drag follow-up events routed by the scrollable reach it when dropping
        // outside any specific row (e.g. on the tree's padding or empty area).
        let scrollable_content = DragTarget::<DraggedEntries>::new()
            .will_accept(move |dragged: &DraggedEntries| {
                tree_ops::can_drop_all(&dragged.paths, &check_root)
            })
            .on_accept(move |dragged: DraggedEntries| {
                accept_drop(dragged, root.clone(), &drop_store);
            })
            .child(move |drop_state: DragTargetState| {
                let indicator = if drop_state.is_hovered && drop_state.will_accept {
                    theme.marker
                } else {
                    Color::Transparent
                };
                let drop_line = Container::new()
                    .height(2)
                    .width(Dimension::Percent(100.0))
                    .margin(pad(4, 10, 0, 10))
                    .decoration([BorderRadius::all(1.px()).notype(), indicator.notype()])
                    .child(SizedBox::new());
                Container::new()
                    .width(Dimension::Percent(100.0))
                    .padding(pad(top_pad, 0, BOTTOM_PAD, 0))
                    .child(Column::new().children(list![
                        SizedBox::new().height(HEADER_HEIGHT),
                        child.clone(),
                        drop_line
                    ]))
            });

        // `Focusable` sits beside `Scrollable` (not wrapping it) so `Scrollable::on_key_press` does not consume Arrow/Home/End/PageUp/PageDown
        // before the tree's own focus-scoped `Shortcuts` handle them.
        let focus_anchor = Focusable::new().node(focus).child(
            SizedBox::new()
                .width(Dimension::Percent(100.0))
                .height(Dimension::Percent(100.0)),
        );
        let tree_body = Stack::new()
            .add_child(focus_anchor)
            .add_child(blank)
            .add_child(
                Scrollable::new()
                    .key("refractor-panel-filetree-scrollable")
                    .controller(this.scroll.clone())
                    .vertical_scroll_bar(None)
                    .child(scrollable_content),
            );

        let step = |delta: isize, extend: bool| {
            let (store, scroll) = (store.clone(), this.scroll.clone());
            move || {
                let mut landed = None;
                let mut count = 0;
                store.update(|space| {
                    landed = space.navigate_tree(delta, extend);
                    count = space.rows().len();
                });
                if let Some(index) = landed {
                    reveal_row(&scroll, index, count, top_pad);
                }
            }
        };
        let edge = |end: bool, extend: bool| {
            let (store, scroll) = (store.clone(), this.scroll.clone());
            move || {
                let mut landed = None;
                let mut count = 0;
                store.update(|space| {
                    landed = space.navigate_tree_edge(end, extend);
                    count = space.rows().len();
                });
                if let Some(index) = landed {
                    reveal_row(&scroll, index, count, top_pad);
                }
            }
        };
        let right = {
            let (store, scroll) = (store.clone(), this.scroll.clone());
            move || {
                let mut landed = None;
                let mut count = 0;
                store.update(|space| {
                    landed = space.tree_expand_or_descend();
                    count = space.rows().len();
                });
                if let Some(index) = landed {
                    reveal_row(&scroll, index, count, top_pad);
                }
            }
        };
        let left = {
            let (store, scroll) = (store.clone(), this.scroll.clone());
            move || {
                let mut landed = None;
                let mut count = 0;
                store.update(|space| {
                    landed = space.tree_collapse_or_ascend();
                    count = space.rows().len();
                });
                if let Some(index) = landed {
                    reveal_row(&scroll, index, count, top_pad);
                }
            }
        };
        let activate = |focus_editor: bool| {
            let store = store.clone();
            move || {
                let notify = notifier(&store);
                store.update(|space| space.activate_selected_tree_row(notify, focus_editor));
            }
        };
        let tree_cmd = |action: TreeAction| {
            let store = store.clone();
            move || {
                let target = store.read().target_for_menu_bar();
                run_tree_action(action, target, &store);
            }
        };
        let select_all = {
            let store = store.clone();
            move || store.update(|space| space.select_all_tree())
        };
        let clear = {
            let store = store.clone();
            move || {
                if !store.read().selected_entries.is_empty() {
                    store.update(|space| space.clear_selection());
                }
            }
        };

        // Focus-scoped shortcuts: active only while the sidebar tree owns keyboard focus.
        let nav = Shortcuts::new()
            .repeat(true)
            .bind_str("Down", step(1, false))
            .bind_str("Up", step(-1, false))
            .bind_str("Shift+Down", step(1, true))
            .bind_str("Shift+Up", step(-1, true))
            .bind_str("Right", right)
            .bind_str("Left", left)
            .bind_str("PageDown", step(PAGE_STEP, false))
            .bind_str("PageUp", step(-PAGE_STEP, false))
            .bind_str("Home", edge(false, false))
            .bind_str("End", edge(true, false))
            .bind_str("Shift+Home", edge(false, true))
            .bind_str("Shift+End", edge(true, true))
            .child(tree_body);

        Shortcuts::new()
            .bind_str("Enter", activate(true))
            .bind_str("Space", activate(false))
            .bind_str("CmdOrCtrl+A", select_all)
            .bind_str("CmdOrCtrl+C", tree_cmd(TreeAction::Copy))
            .bind_str("CmdOrCtrl+X", tree_cmd(TreeAction::Cut))
            .bind_str("CmdOrCtrl+V", tree_cmd(TreeAction::Paste))
            .bind_str("F2", tree_cmd(TreeAction::Rename))
            .bind_str("CmdOrCtrl+Backspace", tree_cmd(TreeAction::Delete))
            .bind_str("Delete", tree_cmd(TreeAction::Delete))
            .bind_str("Escape", clear)
            .child(nav)
    }
}
