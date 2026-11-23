//! Collapsible tree rows for the Git changes sidebar, reusing the shared tree-view animation
//! and colored indent-guide primitives from [`super::tree_view`].

use std::collections::BTreeMap;
use std::path::{Component, PathBuf};
use std::rc::Rc;

use aimer::input::button::Button;
use aimer::input::mouse_region::MouseRegion;
use aimer::style::{LayoutSpacing, TextOverflow, Theme};
use aimer::{
    AnyWidget, BoxAlignment, BuildContext, Color, Column, Container, Dimension, Expanded, Icons, Key,
    ProviderContext, ProviderHandle, Row as Line, SizedBox, State, StateView, StatefulWidget, Text,
    Widget, list,
};

use super::file_row::change_color;
use super::tree_view::{
    AnimatedCollapse, FolderAnimation, chevron, chevron_spacer, row_shell,
};
use crate::ui::components::icon::{icon, material};
use crate::ui::file_icons::{file_icon, folder_icon};
use crate::ui::fonts;
use crate::services::git::{Change, CommitFileEntry, GitPrompt};
use crate::ui::theme::CodeTheme;
use crate::model::workspace::{Workspace, notifier};

/// Aggregate checkbox state for a folder in the Git changes tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderSelection {
    None,
    Partial,
    All,
}

impl FolderSelection {
    pub fn from_counts(selected: usize, total: usize) -> Self {
        if total > 0 && selected == total {
            Self::All
        } else if selected > 0 {
            Self::Partial
        } else {
            Self::None
        }
    }
}

/// One node of the Git changes tree: either a folder aggregating its changed descendants or a changed file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeNode {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub kind: ChangeNodeKind,
}

/// The payload of a [`ChangeNode`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangeNodeKind {
    Folder {
        selected_count: usize,
        total_count: usize,
        children: Vec<ChangeNode>,
    },
    File {
        absolute_path: PathBuf,
        change: Change,
        selected: bool,
    },
}

#[derive(Default)]
struct DirBuilder {
    dirs: BTreeMap<String, DirBuilder>,
    files: Vec<(String, CommitFileEntry)>,
}

impl DirBuilder {
    fn insert(&mut self, components: &[String], entry: CommitFileEntry) {
        let Some((first, rest)) = components.split_first() else {
            return;
        };
        if rest.is_empty() {
            self.files.push((first.clone(), entry));
        } else {
            self.dirs
                .entry(first.clone())
                .or_default()
                .insert(rest, entry);
        }
    }

    fn finish(self, prefix: &PathBuf, depth: usize) -> (Vec<ChangeNode>, usize, usize) {
        let mut dir_entries: Vec<(String, DirBuilder)> = self.dirs.into_iter().collect();
        dir_entries.sort_by(|a, b| {
            a.0.to_lowercase()
                .cmp(&b.0.to_lowercase())
                .then_with(|| a.0.cmp(&b.0))
        });

        let mut file_entries = self.files;
        file_entries.sort_by(|a, b| {
            a.0.to_lowercase()
                .cmp(&b.0.to_lowercase())
                .then_with(|| a.0.cmp(&b.0))
        });

        let mut nodes = Vec::with_capacity(dir_entries.len() + file_entries.len());
        let mut selected_total = 0;
        let mut count_total = 0;

        for (name, builder) in dir_entries {
            let path = if prefix.as_os_str().is_empty() {
                PathBuf::from(&name)
            } else {
                prefix.join(&name)
            };
            let (children, sel, tot) = builder.finish(&path, depth + 1);
            selected_total += sel;
            count_total += tot;
            nodes.push(ChangeNode {
                path,
                name,
                depth,
                kind: ChangeNodeKind::Folder {
                    selected_count: sel,
                    total_count: tot,
                    children,
                },
            });
        }

        for (name, entry) in file_entries {
            if entry.selected {
                selected_total += 1;
            }
            count_total += 1;
            nodes.push(ChangeNode {
                path: entry.file.path,
                name,
                depth,
                kind: ChangeNodeKind::File {
                    absolute_path: entry.file.absolute_path,
                    change: entry.file.change,
                    selected: entry.selected,
                },
            });
        }

        (nodes, selected_total, count_total)
    }
}

/// Groups flat Git `entries` into a nested tree of folders and files, sorted folders-first then files
/// in case-insensitive alphabetical order (matching [`crate::model::tree::read_dir`]).
pub fn build_change_tree(entries: &[CommitFileEntry]) -> Vec<ChangeNode> {
    let mut root = DirBuilder::default();
    for entry in entries {
        let components: Vec<String> = entry
            .file
            .path
            .components()
            .filter_map(|c| match c {
                Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect();
        if !components.is_empty() {
            root.insert(&components, entry.clone());
        }
    }
    root.finish(&PathBuf::new(), 0).0
}

/// One node of the Git changes tree: a collapsible folder with an aggregate checkbox or a changed file.
#[derive(Clone, StatefulWidget)]
pub struct GitTreeRow {
    key: Option<Key>,
    node: ChangeNode,
    prompt: Rc<GitPrompt>,
}

impl GitTreeRow {
    pub fn new(node: ChangeNode, prompt: Rc<GitPrompt>) -> Self {
        let key = Some(Key::from(node.path.to_string_lossy().into_owned()));
        Self { key, node, prompt }
    }

    /// Builds top-level `GitTreeRow` widgets from flat `entries`.
    pub fn from_entries(entries: &[CommitFileEntry], prompt: &Rc<GitPrompt>) -> Vec<Self> {
        build_change_tree(entries)
            .into_iter()
            .map(|node| Self::new(node, Rc::clone(prompt)))
            .collect()
    }
}

struct GitTreeRowState {
    node: ChangeNode,
    prompt: Rc<GitPrompt>,
    hovered: bool,
    expanded: bool,
    animation: FolderAnimation,
}

impl State<GitTreeRow> for GitTreeRowState {
    fn create(widget: GitTreeRow) -> Self {
        let is_dir = matches!(widget.node.kind, ChangeNodeKind::Folder { .. });
        Self {
            node: widget.node,
            prompt: widget.prompt,
            hovered: false,
            expanded: is_dir,
            animation: FolderAnimation::new(is_dir),
        }
    }

    fn adopt_config_from(&mut self, new: Self) {
        if self.node.path != new.node.path {
            self.expanded = new.expanded;
            self.animation = new.animation;
        }
        self.node = new.node;
        self.prompt = new.prompt;
    }

    fn build(this: StateView<Self>, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let store = ProviderHandle::<Workspace>::of(ctx);
        let hovered = this.hovered;

        match &this.node.kind {
            ChangeNodeKind::Folder {
                selected_count,
                total_count,
                children,
            } => {
                let selection = FolderSelection::from_counts(*selected_count, *total_count);
                let dir_path = this.node.path.clone();
                let dir_name = this.node.name.clone();
                let depth = this.node.depth;
                let prompt = Rc::clone(&this.prompt);
                let chevron_ctrl = this.animation.chevron.clone();
                let body_ctrl = this.animation.body.clone();

                let toggle_expand = move || {
                    this.set_state(|state| {
                        state.expanded = !state.expanded;
                        if state.expanded {
                            state.animation.expand();
                        } else {
                            state.animation.collapse();
                        }
                    });
                };

                let checkbox_icon = match selection {
                    FolderSelection::All => Icons::SQUARE_CHECK,
                    FolderSelection::Partial => Icons::SQUARE_MINUS,
                    FolderSelection::None => Icons::SQUARE,
                };
                let checkbox_color = if selection == FolderSelection::None {
                    theme.muted
                } else {
                    theme.marker
                };
                let target_select = selection != FolderSelection::All;
                let checkbox = Button::new()
                    .no_decor()
                    .on_press(move || prompt.set_dir_selected(&dir_path, target_select))
                    .build(move |_| icon(checkbox_icon, 15, checkbox_color));

                let chevron_btn = Button::new()
                    .no_decor()
                    .on_press(toggle_expand)
                    .build(move |_| chevron(chevron_ctrl.clone(), &theme));

                let label_btn = Button::new()
                    .no_decor()
                    .on_press(toggle_expand)
                    .build(move |_| {
                        Container::new()
                            .width(Dimension::Percent(100.0))
                            .child(
                                Line::new()
                                    .vertical_alignment(BoxAlignment::Center)
                                    .gaps(LayoutSpacing::all(6))
                                    .children(list![
                                        material(folder_icon(&dir_name), 16),
                                        Expanded::new().child(
                                            Text::new(dir_name.clone()).text_style(
                                                fonts::ui()
                                                    .font_size(13)
                                                    .color(theme.text)
                                                    .text_overflow(TextOverflow::Ellipsis)
                                                    .font_weight(aimer::FontWeight::Normal),
                                            ),
                                        ),
                                    ]),
                            )
                    });

                let content = Line::new()
                    .vertical_alignment(BoxAlignment::Center)
                    .gaps(LayoutSpacing::all(6))
                    .children(list![
                        chevron_btn,
                        checkbox,
                        Expanded::new().child(label_btn),
                    ]);

                let background = if hovered {
                    theme.hover
                } else {
                    Color::Transparent
                };
                let header = MouseRegion::new()
                    .on_hover_enter(move || this.set_state(|state| state.hovered = true))
                    .on_hover_exit(move || this.set_state(|state| state.hovered = false))
                    .child(row_shell(depth, background, &theme, content));

                let child_widgets: Vec<GitTreeRow> = children
                    .iter()
                    .cloned()
                    .map(|child| GitTreeRow::new(child, Rc::clone(&this.prompt)))
                    .collect();

                let mut items: Vec<AnyWidget> =
                    Vec::with_capacity(if child_widgets.is_empty() { 1 } else { 2 });
                items.push(header.boxed());
                if !child_widgets.is_empty() {
                    items.push(
                        AnimatedCollapse::new(body_ctrl, Column::new().children(child_widgets))
                            .boxed(),
                    );
                }
                Column::new().children(items).boxed()
            }
            ChangeNodeKind::File {
                absolute_path,
                change,
                selected,
            } => {
                let rel_path = this.node.path.clone();
                let abs_path = absolute_path.clone();
                let file_name = this.node.name.clone();
                let depth = this.node.depth;
                let change = *change;
                let selected = *selected;
                let prompt = Rc::clone(&this.prompt);

                let shown = ctx.select::<Workspace, bool>({
                    let abs_path = abs_path.clone();
                    move |space| {
                        space
                            .active_document()
                            .is_some_and(|document| document.path == abs_path)
                    }
                });

                let name_color = change_color(Some(change), &theme);
                let checkbox = Button::new()
                    .no_decor()
                    .on_press(move || prompt.toggle_file(&rel_path))
                    .build(move |_| {
                        icon(
                            if selected {
                                Icons::SQUARE_CHECK
                            } else {
                                Icons::SQUARE
                            },
                            15,
                            if selected { theme.marker } else { theme.muted },
                        )
                    });

                let open_store = store.clone();
                let file_btn = Button::new()
                    .no_decor()
                    .on_press(move || {
                        if abs_path.is_file() {
                            let notify = notifier(&open_store);
                            open_store.update(|space| {
                                let _ = space.open(&abs_path, notify);
                            });
                        }
                    })
                    .build(move |_| {
                        let mark: AnyWidget = match change.letter() {
                            Some(letter) => Text::new(letter)
                                .text_style(fonts::ui().font_size(12).color(name_color))
                                .notype(),
                            None => SizedBox::new().width(1).notype(),
                        };
                        Container::new()
                            .width(Dimension::Percent(100.0))
                            .child(
                                Line::new()
                                    .vertical_alignment(BoxAlignment::Center)
                                    .gaps(LayoutSpacing::all(6))
                                    .children(list![
                                        material(file_icon(&file_name), 16),
                                        Expanded::new().child(
                                            Text::new(file_name.clone()).text_style(
                                                fonts::ui()
                                                    .font_size(13)
                                                    .color(name_color)
                                                    .text_overflow(TextOverflow::Ellipsis)
                                                    .font_weight(aimer::FontWeight::Normal),
                                            ),
                                        ),
                                        mark,
                                    ]),
                            )
                    });

                let content = Line::new()
                    .vertical_alignment(BoxAlignment::Center)
                    .gaps(LayoutSpacing::all(6))
                    .children(list![
                        chevron_spacer(),
                        checkbox,
                        Expanded::new().child(file_btn),
                    ]);

                let background = if shown {
                    theme.selected
                } else if hovered {
                    theme.hover
                } else {
                    Color::Transparent
                };

                MouseRegion::new()
                    .on_hover_enter(move || this.set_state(|state| state.hovered = true))
                    .on_hover_exit(move || this.set_state(|state| state.hovered = false))
                    .child(row_shell(depth, background, &theme, content))
                    .boxed()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::git::operations::ChangedFile;
    use crate::ui::screen::editor::tree_view::ROW_HEIGHT;
    use crate::model::tree::Scratch;
    use aimer::quiver::winit::dpi::{PhysicalPosition, PhysicalSize};
    use aimer::quiver::winit::event::{DeviceId, ElementState, MouseButton, WindowEvent};
    use aimer::style::AnimatedTheme;
    use aimer::{AimerApp, HeadlessOptions, Provider};
    use std::time::Duration;

    fn entry(path: &str, change: Change, selected: bool) -> CommitFileEntry {
        CommitFileEntry {
            file: ChangedFile {
                path: PathBuf::from(path),
                absolute_path: PathBuf::from("/repo").join(path),
                change,
                staged: false,
            },
            selected,
        }
    }

    #[test]
    fn build_change_tree_groups_nested_paths_and_sorts_folders_first() {
        let entries = vec![
            entry("Cargo.toml", Change::Modified, true),
            entry("src/theme.rs", Change::Modified, true),
            entry("src/screen/editor/file_row.rs", Change::Modified, true),
            entry("src/screen/editor/file_tree.rs", Change::Added, false),
            entry("README.md", Change::Untracked, true),
        ];

        let tree = build_change_tree(&entries);
        assert_eq!(tree.len(), 3);
        assert_eq!(tree[0].name, "src");
        assert_eq!(tree[0].depth, 0);
        assert_eq!(tree[1].name, "Cargo.toml");
        assert_eq!(tree[2].name, "README.md");

        let ChangeNodeKind::Folder {
            selected_count,
            total_count,
            children,
        } = &tree[0].kind
        else {
            panic!("expected src folder");
        };
        assert_eq!((*selected_count, *total_count), (2, 3));
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].name, "screen");
        assert_eq!(children[0].depth, 1);
        assert_eq!(children[1].name, "theme.rs");
        assert_eq!(children[1].depth, 1);

        let ChangeNodeKind::Folder {
            selected_count: screen_sel,
            total_count: screen_tot,
            children: screen_children,
        } = &children[0].kind
        else {
            panic!("expected screen folder");
        };
        assert_eq!((*screen_sel, *screen_tot), (1, 2));
        assert_eq!(screen_children[0].name, "editor");
        assert_eq!(screen_children[0].depth, 2);

        let ChangeNodeKind::Folder {
            children: editor_children,
            ..
        } = &screen_children[0].kind
        else {
            panic!("expected editor folder");
        };
        assert_eq!(editor_children.len(), 2);
        assert_eq!(editor_children[0].name, "file_row.rs");
        assert_eq!(editor_children[0].depth, 3);
        assert_eq!(editor_children[1].name, "file_tree.rs");
        assert_eq!(editor_children[1].depth, 3);
    }

    #[test]
    fn folder_selection_reflects_none_partial_and_all() {
        assert_eq!(FolderSelection::from_counts(0, 0), FolderSelection::None);
        assert_eq!(FolderSelection::from_counts(0, 3), FolderSelection::None);
        assert_eq!(FolderSelection::from_counts(1, 3), FolderSelection::Partial);
        assert_eq!(FolderSelection::from_counts(3, 3), FolderSelection::All);
    }

    #[test]
    fn clicking_folder_in_git_tree_animates_collapse_and_expand_heights() {
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

        let scratch = Scratch::new("git-tree-collapse-anim");
        scratch.file("src/a.rs", "");
        scratch.file("src/b.rs", "");
        let space = Workspace::new(scratch.0.clone());
        let prompt = Rc::new(GitPrompt::default());
        let entries = vec![
            entry("src/a.rs", Change::Modified, true),
            entry("src/b.rs", Change::Modified, true),
        ];
        let rows = GitTreeRow::from_entries(&entries, &prompt);

        let root_widget = Provider::new()
            .create(move || space.clone())
            .child(
                AnimatedTheme::new()
                    .data(CodeTheme::dark())
                    .duration(Duration::ZERO)
                    .child(Column::new().children(rows)),
            );

        let mut app = AimerApp::start_headless_with(
            root_widget,
            HeadlessOptions {
                size: PhysicalSize::new(260, 400),
                scale_factor: 1.0,
            },
        );
        app.pump_frames(2);

        let full_height = (2 * ROW_HEIGHT) as f32;
        assert_eq!(collapse_height(&app), Some(full_height));

        let click_folder_label = |app: &mut aimer::HeadlessAimerApp<_>| {
            let device_id = DeviceId::dummy();
            // Click on the folder name area (x = 80, y = 12) to avoid the checkbox.
            app.send_window_event(WindowEvent::CursorMoved {
                device_id,
                position: PhysicalPosition::new(80.0, 12.0),
            });
            for state in [ElementState::Pressed, ElementState::Released] {
                app.send_window_event(WindowEvent::MouseInput {
                    device_id,
                    state,
                    button: MouseButton::Left,
                });
            }
        };

        // 1. Collapse "src"
        click_folder_label(&mut app);
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
            collapse_heights.windows(2).all(|w| w[1] <= w[0]),
            "collapse heights should decrease monotonically, got {collapse_heights:?}"
        );
        assert_eq!(collapse_heights.last().copied(), Some(0.0));

        // 2. Re-expand "src"
        click_folder_label(&mut app);
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
    }
}
