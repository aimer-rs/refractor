use super::context_menu::show_tree_menu;
use super::tree_drag::{
    AutoExpandTimer, DraggedEntries, TreeRowInteraction, accept_drop, drag_feedback,
};
use super::tree_view::{
    AnimatedCollapse, FolderAnimation, ROW_HEIGHT, chevron, chevron_spacer, row_shell,
};
use crate::model::tree::Row;
use crate::model::workspace::{Workspace, notifier};
use crate::services::git::Change;
use crate::services::tree_ops;
use crate::ui::components::icon::material;
use crate::ui::components::popup::Popup;
use crate::ui::file_icons::{file_icon, folder_icon};
use crate::ui::fonts;
use crate::ui::theme::CodeTheme;
use aimer::dnd::{DragSession, DragTarget, DragTargetState};
use aimer::events::element::Modifiers;
use aimer::input::mouse_region::MouseRegion;
use aimer::share::{RcProjectExt, ShareRef};
use aimer::style::{LayoutSpacing, TextOverflow, Theme};
use aimer::{
    AnyWidget, BoxAlignment, BuildContext, ChildBuilder, Color, Column, Expanded, FocusNode, Key,
    Opacity, ProviderContext, ProviderHandle, Row as Line, SizedBox, State, StateView,
    StatefulWidget, StatelessWidget, Text, Widget, list,
};
use std::path::PathBuf;
use std::rc::Rc;

/// Opacity applied to a file-tree row while it is being dragged.
const DRAGGING_OPACITY: f32 = 0.4;

/// Lowers a row's opacity while its path is being dragged, isolated inside `TreeRowInteraction` so starting a drag never rebuilds
/// the element holding pointer capture.
#[derive(Clone, StatelessWidget)]
struct DragDim {
    path: PathBuf,
    child: ChildBuilder,
}

impl StatelessWidget for DragDim {
    fn build(&self, ctx: &BuildContext) -> impl Widget {
        let path = self.path.clone();
        let dragging = ctx.select::<Workspace, bool>(move |space| space.is_dragging(&path));
        Opacity::new()
            .opacity(if dragging { DRAGGING_OPACITY } else { 1.0 })
            .child(self.child.clone())
    }
}

/// How tall a row is.
pub(super) const HEIGHT: u32 = ROW_HEIGHT;

/// The color of a name git has something to say of.
pub fn change_color(change: Option<Change>, theme: &CodeTheme) -> Color {
    match change {
        None => theme.text,
        Some(Change::Ignored) => theme.muted,
        Some(Change::Deleted) => theme.deleted,
        Some(Change::Conflict) => theme.warning,
        Some(_) => theme.accent,
    }
}

/// One row of the file tree: a folder, which opens and closes with a collapsible reveal and rotating
/// chevron, or a file, which opens in a tab.
///
/// The row watches only what concerns it, whether its file is the one shown and what git says of
/// it, so typing does not rebuild the tree.
#[derive(Clone, StatefulWidget)]
pub struct FileRow {
    key: Option<Key>,
    row: ShareRef<Row>,
    tree_focus: FocusNode,
    children: Vec<FileRow>,
    snap_closed: bool,
}

impl FileRow {
    pub fn with_children(
        row: ShareRef<Row>,
        tree_focus: FocusNode,
        children: Vec<FileRow>,
    ) -> Self {
        let key = Some(Key::from(row.path.to_string_lossy().into_owned()));
        Self {
            key,
            row,
            tree_focus,
            children,
            snap_closed: false,
        }
    }

    /// Groups the preorder `rows` into a tree of `FileRow` widgets so each open folder owns its
    /// direct children and can animate their vertical reveal on expand and collapse.
    pub fn from_rows(rows: &Rc<Vec<Row>>, tree_focus: &FocusNode) -> Vec<Self> {
        let mut index = 0;
        Self::build_level(rows, &mut index, 0, tree_focus)
    }

    fn build_level(
        rows: &Rc<Vec<Row>>,
        index: &mut usize,
        depth: usize,
        tree_focus: &FocusNode,
    ) -> Vec<Self> {
        let mut nodes = Vec::new();
        while let Some(current) = rows.get(*index) {
            if current.depth < depth {
                break;
            }
            let i = *index;
            let row_depth = current.depth;
            let is_expanded_dir = current.is_dir && current.expanded;
            *index += 1;
            let row = rows.project(move |r| &r[i]);
            let children = if is_expanded_dir {
                Self::build_level(rows, index, row_depth + 1, tree_focus)
            } else {
                Vec::new()
            };
            nodes.push(Self::with_children(row, tree_focus.clone(), children));
        }
        nodes
    }

    fn with_snap_closed(mut self) -> Self {
        self.snap_closed = true;
        self
    }
}

struct FileRowState {
    row: ShareRef<Row>,
    tree_focus: FocusNode,
    hovered: bool,
    auto_expand: Rc<AutoExpandTimer>,
    expanded: bool,
    animation: FolderAnimation,
    children: Vec<FileRow>,
    snap_closed: bool,
}

impl State<FileRow> for FileRowState {
    fn create(widget: FileRow) -> Self {
        let expanded = widget.row.is_dir && widget.row.expanded;
        Self {
            row: widget.row,
            tree_focus: widget.tree_focus,
            hovered: false,
            auto_expand: Rc::default(),
            expanded,
            animation: FolderAnimation::new(expanded),
            children: widget.children,
            snap_closed: widget.snap_closed,
        }
    }

    fn adopt_config_from(&mut self, new: Self) {
        if self.row.path != new.row.path {
            self.auto_expand.cancel();
            self.expanded = new.expanded;
            self.animation = new.animation;
            self.children = new.children;
            self.snap_closed = false;
        } else if new.row.is_dir {
            let now_expanded = new.row.expanded;
            if now_expanded {
                let snap_children = !self.expanded && self.animation.body.value() <= 0.01;
                self.children = if snap_children {
                    new.children
                        .into_iter()
                        .map(FileRow::with_snap_closed)
                        .collect()
                } else {
                    new.children
                };
                if !self.expanded {
                    self.expanded = true;
                    self.animation.expand();
                }
            } else if self.expanded {
                self.expanded = false;
                if new.snap_closed {
                    self.children.clear();
                    self.animation.snap_closed();
                } else {
                    self.animation.collapse();
                }
            } else if new.snap_closed && !self.children.is_empty() {
                self.children.clear();
                self.animation.snap_closed();
            }
        }
        self.row = new.row;
        self.tree_focus = new.tree_focus;
    }

    fn build(this: StateView<Self>, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let path = this.row.path.clone();
        let (shown, selected) = ctx.select::<Workspace, (bool, bool)>({
            let path = path.clone();
            move |space| {
                let shown = space
                    .active_document()
                    .is_some_and(|document| document.path == path);
                (shown, space.is_selected(&path))
            }
        });
        let (change, inside, cut) = ctx.select::<Workspace, (Option<Change>, bool, bool)>({
            let path = path.clone();
            move |space| {
                (
                    space.change_of(&path),
                    space.has_changes_inside(&path),
                    space.is_cut(&path),
                )
            }
        });
        let store = ProviderHandle::<Workspace>::of(ctx);
        let root = store.read().root.clone();
        let dest_dir = tree_ops::target_dir(&root, Some((&this.row.path, this.row.is_dir)));
        let check_dir = dest_dir.clone();
        let drop_dir = dest_dir;
        let drop_store = store.clone();
        let drop_timer = Rc::clone(&this.auto_expand);
        let hover_timer = Rc::clone(&this.auto_expand);
        let row = this.row.clone();
        let hovered = this.hovered;
        let chevron_controller = this.animation.chevron.clone();
        let press_row = this.row.clone();
        let press_store = store.clone();
        let menu_row = this.row.clone();
        let menu_store = store.clone();
        let drag_row = this.row.clone();
        let drag_data_store = store.clone();
        let drag_state_store = store.clone();
        let tap_focus = this.tree_focus.clone();
        let menu_focus = this.tree_focus.clone();

        let target = DragTarget::<DraggedEntries>::new()
            .will_accept(move |dragged: &DraggedEntries| {
                tree_ops::can_drop_all(&dragged.paths, &check_dir)
            })
            .on_accept(move |dragged: DraggedEntries| {
                drop_timer.cancel();
                accept_drop(dragged, drop_dir.clone(), &drop_store);
            })
            .child(move |drop_state: DragTargetState| {
                let drop_highlight = drop_state.is_hovered && drop_state.will_accept;
                if row.is_dir && !row.expanded && drop_highlight {
                    hover_timer.arm(row.path.clone(), store.clone());
                } else {
                    hover_timer.cancel();
                }

                let background = if drop_highlight {
                    theme.marker.with_alpha(Popup::ROW_HIGHLIGHT)
                } else if shown {
                    theme.selected
                } else if (selected || (hovered && !DragSession::is_active())) && !row.is_dir {
                    theme.hover.with_alpha(0.01)
                } else {
                    Color::Transparent
                };
                // A folder with changes inside is named in the color of a change, though it has none itself.
                // A staged Cut entry is dimmed until pasted or cleared.
                let name_color = if cut {
                    theme.muted
                } else {
                    match (change, row.is_dir && inside) {
                        (None, true) => theme.accent,
                        (change, _) => change_color(change, &theme),
                    }
                };
                let lead: AnyWidget = if row.is_dir {
                    chevron(chevron_controller.clone(), &theme)
                } else {
                    chevron_spacer()
                };
                let kind = material(
                    if row.is_dir {
                        folder_icon(&row.name)
                    } else {
                        file_icon(&row.name)
                    },
                    16,
                );
                // The letter of a change, or a dot on a folder with changes inside it.
                let mark: AnyWidget = match (change.and_then(Change::letter), row.is_dir && inside)
                {
                    (Some(letter), _) => Text::new(letter)
                        .text_style(fonts::ui().font_size(12).color(name_color))
                        .notype(),
                    (None, true) => crate::ui::components::dot::dot(6, theme.accent).notype(),
                    (None, false) => SizedBox::new().width(1).notype(),
                };
                let content = Line::new()
                    .vertical_alignment(BoxAlignment::Center)
                    .gaps(LayoutSpacing::all(6))
                    .children(list![
                        lead,
                        kind,
                        Expanded::new().child(
                            Text::new(row.name.clone()).text_style(
                                fonts::ui()
                                    .font_size(13)
                                    .color(name_color)
                                    .text_overflow(TextOverflow::Clip)
                                    .font_weight(aimer::FontWeight::Normal),
                            ),
                        ),
                        mark,
                    ]);

                MouseRegion::new()
                    .on_hover_enter(move || {
                        if !DragSession::is_active() {
                            this.set_state(|state| state.hovered = true);
                        }
                    })
                    .on_hover_exit(move || {
                        this.set_state(|state| state.hovered = false);
                    })
                    .child(DragDim {
                        path: row.path.clone(),
                        child: ChildBuilder::from_widget(row_shell(
                            row.depth, background, &theme, content,
                        )),
                    })
            });

        // `TreeRowInteraction` sits outside `DragTarget` and `DragDim` so hover/opacity rebuilds never replace the captured drag element.
        let header = TreeRowInteraction::new(
            move || {
                let space = drag_data_store.read();
                let paths = if space.is_selected(&drag_row.path) && space.selected_entries.len() > 1
                {
                    let selected: Vec<PathBuf> = space
                        .selected_entries_list()
                        .into_iter()
                        .map(|(p, _)| p)
                        .collect();
                    tree_ops::prune_descendants(&selected)
                } else {
                    vec![drag_row.path.clone()]
                };
                DraggedEntries {
                    paths,
                    name: drag_row.name.clone(),
                    is_dir: drag_row.is_dir,
                }
            },
            move |entries| drag_feedback(entries, theme),
            move |active| {
                drag_state_store.update(|space| match active {
                    Some(paths) => space.set_dragging(paths),
                    None => space.clear_dragging(),
                });
            },
            move || {
                super::file_tree::request_tree_focus(&tap_focus);
                let modifiers = Modifiers::current();
                if modifiers.shift {
                    press_store.update(|space| {
                        space.select_range_to(press_row.path.clone(), press_row.is_dir)
                    });
                } else if modifiers.command() {
                    press_store.update(|space| {
                        space.toggle_select_entry(press_row.path.clone(), press_row.is_dir)
                    });
                } else if press_row.is_dir {
                    press_store.update(|space| {
                        space.select_entry(press_row.path.clone(), true);
                        space.toggle(&press_row.path);
                    });
                } else {
                    let notify = notifier(&press_store);
                    // The reason a file could not be opened is in the workspace's message.
                    press_store.update(|space| {
                        space.select_entry(press_row.path.clone(), false);
                        let _ = space.open(&press_row.path, notify);
                    });
                }
            },
            move |pos| {
                super::file_tree::request_tree_focus(&menu_focus);
                let target = (menu_row.path.clone(), menu_row.is_dir);
                menu_store.update(|space| {
                    space.ensure_selected_for_context_menu(target.0.clone(), target.1)
                });
                show_tree_menu(theme, pos, Some(target), menu_store.clone());
            },
            target,
        );

        if this.row.is_dir {
            let mut items: Vec<AnyWidget> =
                Vec::with_capacity(if this.children.is_empty() { 1 } else { 2 });
            items.push(header.boxed());
            if !this.children.is_empty() {
                items.push(
                    AnimatedCollapse::new(
                        this.animation.body.clone(),
                        Column::new().children(this.children.clone()),
                    )
                    .boxed(),
                );
            }
            Column::new().children(items).boxed()
        } else {
            header.boxed()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::components::pad::pad;
    use crate::ui::screen::editor::tree_view::{GUIDE_ALPHA, guide_colors_for_depth};
    use aimer::quiver::winit::dpi::{PhysicalPosition, PhysicalSize};
    use aimer::quiver::winit::event::{DeviceId, MouseScrollDelta, TouchPhase, WindowEvent};
    use aimer::{
        AimerApp, Column, Container, Dimension, HeadlessOptions, ScrollController, Scrollable,
    };
    use std::cell::RefCell;
    use std::time::Duration;

    #[derive(Clone, StatefulWidget)]
    struct HoverProbeRow {
        index: usize,
        hovered_rows: Rc<RefCell<Vec<bool>>>,
    }

    struct HoverProbeRowState {
        index: usize,
        hovered: bool,
        hovered_rows: Rc<RefCell<Vec<bool>>>,
    }

    impl State<HoverProbeRow> for HoverProbeRowState {
        fn create(widget: HoverProbeRow) -> Self {
            Self {
                index: widget.index,
                hovered: false,
                hovered_rows: widget.hovered_rows,
            }
        }

        fn adopt_config_from(&mut self, new: Self) {
            self.index = new.index;
            self.hovered_rows = new.hovered_rows;
        }

        fn build(this: StateView<Self>, _ctx: &BuildContext) -> impl Widget {
            let index = this.index;
            let hovered_rows = Rc::clone(&this.hovered_rows);
            let hovered = this.hovered;
            hovered_rows.borrow_mut()[index] = hovered;

            let enter_rows = Rc::clone(&hovered_rows);
            let exit_rows = Rc::clone(&hovered_rows);
            let target = DragTarget::<DraggedEntries>::new()
                .will_accept(|_| false)
                .on_accept(|_| {})
                .child(move |_| {
                    let enter_rows = Rc::clone(&enter_rows);
                    let exit_rows = Rc::clone(&exit_rows);
                    MouseRegion::new()
                        .on_hover_enter(move || {
                            enter_rows.borrow_mut()[index] = true;
                            this.set_state(|s| s.hovered = true);
                        })
                        .on_hover_exit(move || {
                            exit_rows.borrow_mut()[index] = false;
                            this.set_state(|s| s.hovered = false);
                        })
                        .child(
                            Container::new()
                                .height(HEIGHT)
                                .width(Dimension::Percent(100.0))
                                .color(if hovered {
                                    Color::Rgb(80, 80, 80)
                                } else {
                                    Color::Transparent
                                })
                                .child(SizedBox::new()),
                        )
                });

            TreeRowInteraction::new(
                move || DraggedEntries {
                    paths: vec![PathBuf::from(format!("/repo/f{index}.rs"))],
                    name: format!("f{index}.rs"),
                    is_dir: false,
                },
                |_| SizedBox::new().width(80).height(20).boxed(),
                |_| {},
                || {},
                |_| {},
                target,
            )
        }
    }

    #[test]
    fn scrolling_sidebar_rows_leaves_at_most_one_row_hovered() {
        let count = 40;
        let hovered_rows = Rc::new(RefCell::new(vec![false; count]));
        let controller = ScrollController::new();
        let rows: Vec<usize> = (0..count).collect();
        let hovered_in = Rc::clone(&hovered_rows);

        let child = ChildBuilder::from_widget(Column::new().list(rows.clone()).builder(
            move |&idx| HoverProbeRow {
                index: idx,
                hovered_rows: Rc::clone(&hovered_in),
            },
        ));

        let root_widget = Scrollable::new()
            .controller(controller.clone())
            .vertical_scroll_bar(None)
            .child(
                DragTarget::<DraggedEntries>::new()
                    .will_accept(|_| false)
                    .on_accept(|_| {})
                    .child(move |_| {
                        let drop_line = Container::new()
                            .height(2)
                            .width(Dimension::Percent(100.0))
                            .child(SizedBox::new());
                        Container::new()
                            .width(Dimension::Percent(100.0))
                            .padding(pad(2, 0, 48, 0))
                            .child(
                                Column::new().children([child.clone().boxed(), drop_line.boxed()]),
                            )
                    }),
            );

        let mut app = AimerApp::start_headless_with(
            root_widget,
            HeadlessOptions {
                size: PhysicalSize::new(400, 400),
                scale_factor: 2.0,
            },
        );
        app.pump_frames(2);

        let device_id = DeviceId::dummy();
        // Place mouse over row 2 (logical y = 60, physical y = 120 at scale 2.0)
        app.send_window_event(WindowEvent::CursorMoved {
            device_id,
            position: PhysicalPosition::new(100.0, 120.0),
        });
        app.pump_frames(2);

        let initial_hovered: Vec<usize> = hovered_rows
            .borrow()
            .iter()
            .enumerate()
            .filter_map(|(i, &h)| h.then_some(i))
            .collect();
        assert_eq!(initial_hovered, vec![2], "row 2 should start hovered");

        // Scroll down across multiple rows using wheel events (like trackpad scrolling).
        for step in 0..15 {
            let phase = if step == 0 {
                TouchPhase::Started
            } else {
                TouchPhase::Moved
            };
            app.send_window_event(WindowEvent::MouseWheel {
                device_id,
                delta: MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, -48.0)),
                phase,
            });
            app.render_frame();
            let currently_hovered: Vec<usize> = hovered_rows
                .borrow()
                .iter()
                .enumerate()
                .filter_map(|(i, &h)| h.then_some(i))
                .collect();
            assert!(
                currently_hovered.len() <= 1,
                "after scroll step {step} (offset_y={}), expected at most 1 hovered row, got {currently_hovered:?}",
                controller.offset().y
            );
        }
        assert!(
            controller.offset().y > 100.0,
            "expected scroll offset to advance, got {}",
            controller.offset().y
        );
    }

    #[test]
    fn from_rows_groups_open_folder_descendants_into_nested_children() {
        let rows = Rc::new(vec![
            Row {
                path: PathBuf::from("/repo/src"),
                name: "src".into(),
                depth: 0,
                is_dir: true,
                expanded: true,
            },
            Row {
                path: PathBuf::from("/repo/src/screen"),
                name: "screen".into(),
                depth: 1,
                is_dir: true,
                expanded: true,
            },
            Row {
                path: PathBuf::from("/repo/src/screen/file_row.rs"),
                name: "file_row.rs".into(),
                depth: 2,
                is_dir: false,
                expanded: false,
            },
            Row {
                path: PathBuf::from("/repo/src/main.rs"),
                name: "main.rs".into(),
                depth: 1,
                is_dir: false,
                expanded: false,
            },
            Row {
                path: PathBuf::from("/repo/Cargo.toml"),
                name: "Cargo.toml".into(),
                depth: 0,
                is_dir: false,
                expanded: false,
            },
        ]);
        let tree = FileRow::from_rows(&rows, &FocusNode::new());
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].row.name, "src");
        assert_eq!(tree[0].children.len(), 2);
        assert_eq!(tree[0].children[0].row.name, "screen");
        assert_eq!(tree[0].children[0].children.len(), 1);
        assert_eq!(tree[0].children[0].children[0].row.name, "file_row.rs");
        assert_eq!(tree[0].children[1].row.name, "main.rs");
        assert_eq!(tree[1].row.name, "Cargo.toml");
    }

    #[test]
    fn indent_guide_colors_match_each_indent_level() {
        let theme = CodeTheme::dark();
        assert!(guide_colors_for_depth(0, &theme).is_empty());
        let colors = guide_colors_for_depth(3, &theme);
        assert_eq!(
            colors,
            vec![
                theme.indent_guide(0).with_alpha(GUIDE_ALPHA),
                theme.indent_guide(1).with_alpha(GUIDE_ALPHA),
                theme.indent_guide(2).with_alpha(GUIDE_ALPHA),
            ]
        );
    }

    #[test]
    fn collapsing_and_expanding_folder_retains_children_and_animates_controllers() {
        use aimer::CreateState;

        let open_rows = Rc::new(vec![
            Row {
                path: PathBuf::from("/repo/src"),
                name: "src".into(),
                depth: 0,
                is_dir: true,
                expanded: true,
            },
            Row {
                path: PathBuf::from("/repo/src/lib.rs"),
                name: "lib.rs".into(),
                depth: 1,
                is_dir: false,
                expanded: false,
            },
        ]);
        let focus = FocusNode::new();
        let initial = FileRow::from_rows(&open_rows, &focus).remove(0);
        let mut state = initial.create_state::<FileRowState>();
        assert!(state.expanded);
        assert_eq!(state.children.len(), 1);
        assert_eq!(state.animation.body.value(), 1.0);
        assert_eq!(state.animation.chevron.value(), 0.0);

        let closed_rows = Rc::new(vec![Row {
            path: PathBuf::from("/repo/src"),
            name: "src".into(),
            depth: 0,
            is_dir: true,
            expanded: false,
        }]);
        let collapsed = FileRow::from_rows(&closed_rows, &focus).remove(0);
        state.adopt_config_from(collapsed.create_state::<FileRowState>());
        assert!(!state.expanded);
        assert_eq!(
            state.children.len(),
            1,
            "children stay retained so AnimatedCollapse can animate the body closed"
        );
        assert!(state.animation.body.is_animating());
        assert!(state.animation.chevron.is_animating());
    }

    #[test]
    fn clicking_folder_in_file_tree_animates_expand_and_collapse_heights() {
        use crate::model::tree::Scratch;
        use crate::ui::screen::editor::file_tree::FileTree;
        use aimer::Provider;
        use aimer::quiver::winit::event::{ElementState, MouseButton};
        use aimer::style::AnimatedTheme;

        fn collapse_height<W: Widget + 'static>(app: &aimer::HeadlessAimerApp<W>) -> Option<f32> {
            app.interaction_rects()
                .into_iter()
                .find_map(|(name, rect)| {
                    (name == "AnimatedCollapseElement")
                        .then_some(rect)
                        .flatten()
                        .map(|[_, min_y, _, max_y]| max_y - min_y)
                })
        }

        let scratch = Scratch::new("tree-collapse-anim");
        scratch.file("src/a.rs", "");
        scratch.file("src/b.rs", "");
        let root_path = scratch.0.clone();
        let space = Workspace::new(root_path);
        let tree_focus = space.tree_focus();

        let root_widget = Provider::new()
            .create(move || space.clone())
            .child(
                AnimatedTheme::new()
                    .data(CodeTheme::dark())
                    .duration(Duration::ZERO)
                    .child(FileTree),
            );

        let mut app = AimerApp::start_headless_with(
            root_widget,
            HeadlessOptions {
                size: PhysicalSize::new(260, 400),
                scale_factor: 1.0,
            },
        );
        app.pump_frames(2);

        let click_folder = |app: &mut aimer::HeadlessAimerApp<_>| {
            let device_id = DeviceId::dummy();
            // HEADER_HEIGHT is 38, top_pad is 2, so row 0 ("src") spans y = 40..64.
            app.send_window_event(WindowEvent::CursorMoved {
                device_id,
                position: PhysicalPosition::new(40.0, 52.0),
            });
            for state in [ElementState::Pressed, ElementState::Released] {
                app.send_window_event(WindowEvent::MouseInput {
                    device_id,
                    state,
                    button: MouseButton::Left,
                });
            }
        };

        let full_height = (2 * HEIGHT) as f32;

        // 1. Expand "src"
        click_folder(&mut app);
        let mut expand_heights = Vec::new();
        for _ in 0..80 {
            if app.pump_frames(1) == 0 {
                break;
            }
            if let Some(h) = collapse_height(&app) {
                expand_heights.push(h);
            }
            std::thread::sleep(Duration::from_millis(8));
        }
        assert!(
            expand_heights
                .iter()
                .any(|&h| h > 1.0 && h < full_height - 1.0),
            "expand should pass through intermediate heights, got {expand_heights:?}"
        );
        assert!(
            expand_heights.windows(2).all(|w| w[1] >= w[0]),
            "expand heights should increase monotonically, got {expand_heights:?}"
        );
        assert_eq!(expand_heights.last().copied(), Some(full_height));

        // 2. Collapse "src"
        click_folder(&mut app);
        let mut collapse_heights = Vec::new();
        for _ in 0..80 {
            if app.pump_frames(1) == 0 {
                break;
            }
            if let Some(h) = collapse_height(&app) {
                collapse_heights.push(h);
            }
            std::thread::sleep(Duration::from_millis(8));
        }
        assert!(
            collapse_heights
                .iter()
                .any(|&h| h > 1.0 && h < full_height - 1.0),
            "collapse should pass through intermediate heights, got {collapse_heights:?}"
        );
        assert!(
            collapse_heights
                .windows(2)
                .all(|w| w[1] <= w[0]),
            "collapse heights should decrease monotonically, got {collapse_heights:?}"
        );
        assert_eq!(collapse_heights.last().copied(), Some(0.0));
        tree_focus.set_focused(false);
    }
}
