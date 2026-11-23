//! The Git tool panel in the left sidebar (`CmdOrCtrl+2`), modeled after IntelliJ IDEA's Commit & Push tool window.

use std::path::PathBuf;
use std::rc::Rc;

use aimer::events::element::NamedKey;
use aimer::icons::Icon;
use aimer::input::button::Button;
use aimer::input::input::TextField;
use aimer::style::{
    BorderSlice, BorderStyle, BoxBorder, BoxDecoration, BoxOutline, LayoutSpacing, TextOverflow,
    TextStyle, Theme,
};
use aimer::{
    AnyWidget, BoxAlignment, BuildContext, Center, Color, Column, Container, Dimension, Expanded, FontWeight, Icons, ProviderContext, ProviderHandle, Row, Scrollable, SizedBox, State, StateUpdater, StateView, StatefulWidget, StaticSvg, SvgDocument, Text, Venus, Widget, ZeroSizedBox, list,
};

use super::git_tree_row::GitTreeRow;
use crate::ui::components::icon::icon;
use crate::ui::components::pad::pad;
use crate::ui::components::popup::Popup;
use crate::ui::components::rule;
use crate::ui::fonts;
use crate::services::git::{self, CommitSummary, GitPrompt};
use crate::ui::screen::editor::side_panel::HEADER_HEIGHT;
use crate::ui::theme::CodeTheme;
use crate::model::workspace::{Workspace, refresh_git_panel, refresh_status, save_document};

const FIELD_HEIGHT: u32 = 32;
const OUTGOING_LIST_HEIGHT: u32 = 88;

/// The Git Commit & Push panel shown inside the left sidebar when [`crate::model::workspace::SidePanelTab::Git`] is active.
#[derive(Clone, StatefulWidget)]
pub struct GitPanel {
    prompt: Rc<GitPrompt>,
}

impl GitPanel {
    pub fn new(prompt: Rc<GitPrompt>) -> Self {
        Self { prompt }
    }
}

struct GitPanelState {
    prompt: Rc<GitPrompt>,
    changes: u32,
}

impl State<GitPanel> for GitPanelState {
    fn create(widget: GitPanel) -> Self {
        Self {
            prompt: widget.prompt,
            changes: 0,
        }
    }

    fn on_mount(&mut self, updater: StateUpdater<Self>) {
        self.prompt
            .on_change(Popup::redraw_watcher(updater, |state| state.changes += 1));
    }

    fn adopt_config_from(&mut self, new: Self) {
        self.prompt = Rc::clone(&new.prompt);
    }

    fn build(this: StateView<Self>, ctx: &BuildContext) -> impl Widget {
        let _changes = this.changes;
        let theme = *CodeTheme::of(ctx);
        let store = ProviderHandle::<Workspace>::of(ctx);
        let branch = ctx
            .select::<Workspace, Option<String>>(|space| space.branch.clone())
            .unwrap_or_else(|| "HEAD".to_owned());

        let prompt = Rc::clone(&this.prompt);
        let entries = prompt.files();
        let unpushed = prompt.unpushed();
        let busy = prompt.is_busy();
        let error = prompt.error();

        let total = entries.len();
        let selected_count = entries.iter().filter(|e| e.selected).count();
        let all_selected = total > 0 && selected_count == total;

        let remote_label = unpushed
            .as_ref()
            .map(|info| format!("\u{2192} {}", info.remote_branch))
            .unwrap_or_default();

        // Branch bar
        let branch_bar = Container::new()
            .width(Dimension::Percent(100.0))
            .padding(pad(6, 10, 6, 10))
            .child(
                Row::new()
                    .vertical_alignment(BoxAlignment::Center)
                    .gaps(LayoutSpacing::all(6))
                    .children(list![
                        icon(Icons::GIT_BRANCH, 13, theme.marker),
                        Text::new(branch).text_style(
                            fonts::ui()
                                .font_size(12)
                                .color(theme.text)
                                .font_weight(FontWeight::Bold),
                        ),
                        Expanded::new().child(
                            Text::new(remote_label).text_style(
                                fonts::ui()
                                    .font_size(11)
                                    .color(theme.muted)
                                    .text_overflow(TextOverflow::Ellipsis),
                            ),
                        ),
                    ]),
            );

        // Changes header with Select All checkbox
        let toggle_all = {
            let prompt = Rc::clone(&prompt);
            Button::new()
                .no_decor()
                .on_press(move || prompt.select_all(!all_selected))
                .build(move |btn_state| {
                    let bg = if btn_state.is_hovered {
                        theme.hover
                    } else {
                        Color::Transparent
                    };
                    Container::new()
                        .width(Dimension::Percent(100.0))
                        .padding(pad(4, 10, 4, 8))
                        .decoration(BoxDecoration::new().background_color(bg))
                        .child(
                            Row::new()
                                .vertical_alignment(BoxAlignment::Center)
                                .gaps(LayoutSpacing::all(6))
                                .children([
                                    icon(
                                        if all_selected {
                                            Icons::SQUARE_CHECK
                                        } else {
                                            Icons::SQUARE
                                        },
                                        16,
                                        if all_selected {
                                            theme.marker
                                        } else {
                                            theme.muted
                                        },
                                    )
                                    .boxed(),
                                    Text::new(format!("Changes ({selected_count}/{total})"))
                                        .text_style(
                                            fonts::ui()
                                                .font_size(11)
                                                .color(theme.muted)
                                                .font_weight(FontWeight::Bold),
                                        )
                                        .boxed(),
                                ]),
                        )
                })
        };

        // Changed files scrollable tree

        let file_list: AnyWidget = if entries.is_empty() {
            Container::new()
                .width(Dimension::Percent(100.0))
                .padding(pad(14, 10, 14, 10))
                .child(
                    Text::new("No uncommitted changes")
                        .text_style(fonts::ui().font_size(12).color(theme.muted)),
                )
                .boxed()
        } else {
            let tree_rows = GitTreeRow::from_entries(&entries, &prompt);
            Scrollable::new()
                .key("refractor-panel-changes-scrollable")
                .vertical_scroll_bar(None)
                .child(
                    Container::new()
                        .width(Dimension::Percent(100.0))
                        .padding(pad(2, 0, 4, 0))
                        .child(Column::new().children(tree_rows)),
                )
                .boxed()
        };

        // Commit message input
        let on_key = {
            let (prompt, store) = (Rc::clone(&prompt), store.clone());
            move |key: &NamedKey, modifiers: &aimer::events::element::Modifiers| match key {
                NamedKey::Enter => {
                    submit_commit(&prompt, &store, modifiers.shift);
                    true
                }
                _ => false,
            }
        };

        let typed_prompt = Rc::clone(&prompt);
        let field = TextField::new()
            .controller(prompt.message_controller())
            .focus_node(prompt.message_focus())
            .hint("Commit message...")
            .hint_style(fonts::ui().font_size(12).color(theme.muted))
            .text_style(fonts::ui().font_size(12).color(theme.text))
            .decoration(theme.editor)
            .focus_decoration(
                BoxDecoration::new()
                    .outline(BoxOutline::all(
                        BorderSlice::new()
                            .style(BorderStyle::Solid)
                            .stroke(2)
                            .color(theme.muted),
                    ))
                    .background_color(theme.editor),
            )
            .padding(LayoutSpacing::new().left(8).right(8).top(7))
            .on_changed(move |_: String| typed_prompt.typed())
            .on_key(on_key);

        let commit_btn = {
            let (prompt, store) = (Rc::clone(&prompt), store.clone());
            sidebar_button(
                if busy { "Working\u{2026}" } else { "Commit" },
                theme.marker.darken(0.22),
                theme.marker.darken(0.12),
                None,
                Color::WHITE,
                FontWeight::Bold,
                move || {
                    if !busy {
                        submit_commit(&prompt, &store, false);
                    }
                },
            )
        };

        let commit_push_btn = {
            let (prompt, store) = (Rc::clone(&prompt), store.clone());
            sidebar_button(
                "Commit & Push",
                Color::Transparent,
                theme.hover,
                Some(theme.divider),
                theme.text,
                FontWeight::Bold,
                move || {
                    if !busy {
                        submit_commit(&prompt, &store, true);
                    }
                },
            )
        };

        let mut commit_box: Vec<AnyWidget> = vec![
            SizedBox::new()
                .width(Dimension::Percent(100.0))
                .height(FIELD_HEIGHT)
                .box_child(field)
                .boxed(),
        ];

        if let Some(err) = error {
            commit_box.push(
                Text::new(err)
                    .text_style(
                        fonts::ui()
                            .font_size(11)
                            .color(theme.error_line)
                            .text_overflow(TextOverflow::Ellipsis),
                    )
                    .boxed(),
            );
        }

        commit_box.push(
            Row::new()
                .vertical_alignment(BoxAlignment::Center)
                .gaps(LayoutSpacing::all(6))
                .children([
                    Expanded::new().child(commit_push_btn).boxed(),
                    Expanded::new().child(commit_btn).boxed(),
                ])
                .boxed(),
        );

        let commit_section = Container::new()
            .width(Dimension::Percent(100.0))
            .padding(pad(8, 8, 8, 8))
            .child(
                Column::new()
                    .gaps(LayoutSpacing::all(6))
                    .children(commit_box),
            );

        // Outgoing / Push section
        let commits = unpushed
            .as_ref()
            .map(|u| u.commits.clone())
            .unwrap_or_default();
        let outgoing_count = commits.len();

        let push_btn = {
            let (prompt, store) = (Rc::clone(&prompt), store.clone());
            sidebar_button(
                if busy { "Pushing\u{2026}" } else { "Push" },
                Color::Transparent,
                theme.hover,
                Some(theme.divider),
                theme.text,
                FontWeight::Bold,
                move || {
                    if !busy {
                        submit_push(&prompt, &store);
                    }
                },
            )
        };

        let outgoing_header = Container::new()
            .width(Dimension::Percent(100.0))
            .padding(pad(6, 8, 6, 10))
            .child(
                Row::new()
                    .vertical_alignment(BoxAlignment::Center)
                    .gaps(LayoutSpacing::all(6))
                    .children([
                        icon(Icons::UPLOAD, 12, theme.muted).boxed(),
                        Expanded::new()
                            .child(
                                Text::new(format!("Outgoing ({outgoing_count})")).text_style(
                                    fonts::ui()
                                        .font_size(11)
                                        .color(theme.muted)
                                        .font_weight(FontWeight::Bold)
                                        .text_overflow(TextOverflow::Ellipsis),
                                ),
                            )
                            .boxed(),
                        push_btn.boxed(),
                    ]),
            );

        let mut sections: Vec<AnyWidget> = list![
            SizedBox::new().height(HEADER_HEIGHT),
            branch_bar,
            rule::horizontal(theme.divider),
            toggle_all,
            Expanded::new().child(file_list),
            rule::horizontal(theme.divider),
            commit_section.boxed(),
            rule::horizontal(theme.divider),
            outgoing_header,
        ];

        if !commits.is_empty() {
            let rows: Vec<AnyWidget> = commits
                .iter()
                .map(|c| outgoing_commit_row(theme, c).boxed())
                .collect();
            sections.push(
                SizedBox::new()
                    .width(Dimension::Percent(100.0))
                    .height(OUTGOING_LIST_HEIGHT)
                    .box_child(
                        Scrollable::new()
                            .vertical_scroll_bar(None)
                            .child(
                                Container::new()
                                    .width(Dimension::Percent(100.0))
                                    .padding(pad(0, 8, 6, 10))
                                    .child(
                                        Column::new()
                                            .gaps(LayoutSpacing::all(3))
                                            .children(rows),
                                    ),
                            ),
                    )
                    .boxed(),
            );
        }

        Column::new().children(sections)
    }
}

fn outgoing_commit_row(theme: CodeTheme, commit: &CommitSummary) -> impl Widget + 'static {
    Row::new()
        .vertical_alignment(BoxAlignment::Center)
        .gaps(LayoutSpacing::all(6))
        .children([
            Text::new(commit.short_id.clone())
                .text_style(
                    TextStyle::new()
                        .font_family(fonts::code_family())
                        .font_size(11)
                        .color(theme.marker)
                        .font_weight(FontWeight::Bold),
                )
                .boxed(),
            Expanded::new()
                .child(
                    Text::new(commit.message.clone()).text_style(
                        fonts::ui()
                            .font_size(11)
                            .color(theme.text)
                            .text_overflow(TextOverflow::Ellipsis),
                    ),
                )
                .boxed(),
        ])
}

fn submit_commit(prompt: &Rc<GitPrompt>, store: &ProviderHandle<Workspace>, and_push: bool) {
    let dirty_docs: Vec<_> = store
        .read()
        .documents
        .iter()
        .filter(|doc| doc.is_dirty())
        .cloned()
        .collect();
    for doc in &dirty_docs {
        save_document(store, doc);
    }

    let files: Vec<PathBuf> = prompt.selected_files();
    if files.is_empty() {
        prompt.set_error(Some("Select at least one file".to_owned()));
        return;
    }
    let message = prompt.commit_message();
    if message.trim().is_empty() {
        prompt.set_error(Some("Enter a commit message".to_owned()));
        return;
    }

    let root = store.read().root.clone();
    prompt.set_busy(true);
    prompt.set_error(None);

    let Some(venus) = Venus::current() else {
        match git::commit(&root, &files, &message) {
            Ok(oid) => {
                let short = oid[..7.min(oid.len())].to_owned();
                prompt.clear_message();
                store.update(|space| {
                    space.branch = git::branch_of(&space.root);
                    space.message = Some(format!("Committed {short}"));
                });
                if and_push {
                    if let Err(err) = git::push(&root, None, None) {
                        prompt.set_error(Some(err));
                    } else {
                        store.update(|space| {
                            space.message = Some(format!("Committed {short} and pushed"));
                        });
                    }
                }
                prompt.set_busy(false);
                refresh_git_panel(store);
            }
            Err(err) => {
                prompt.set_busy(false);
                prompt.set_error(Some(err));
            }
        }
        return;
    };

    let prompt = Rc::clone(prompt);
    let store = store.clone();
    let blocking = venus.clone();
    venus.spawn(async move {
        let commit_root = root.clone();
        let res = blocking
            .spawn_blocking(move || git::commit(&commit_root, &files, &message))
            .await;
        match res {
            Ok(oid) => {
                let short = oid[..7.min(oid.len())].to_owned();
                prompt.clear_message();
                store.update(|space| {
                    space.branch = git::branch_of(&space.root);
                    space.message = Some(format!("Committed {short}"));
                });
                if and_push {
                    let push_root = root.clone();
                    match blocking
                        .spawn_blocking(move || git::push(&push_root, None, None))
                        .await
                    {
                        Ok(()) => {
                            store.update(|space| {
                                space.message = Some(format!("Committed {short} and pushed"));
                            });
                        }
                        Err(err) => {
                            prompt.set_error(Some(err));
                        }
                    }
                }
                prompt.set_busy(false);
                refresh_status(&store);
            }
            Err(err) => {
                prompt.set_busy(false);
                prompt.set_error(Some(err));
            }
        }
    });
}

fn submit_push(prompt: &Rc<GitPrompt>, store: &ProviderHandle<Workspace>) {
    let root = store.read().root.clone();
    let (remote, branch, remote_branch) = match prompt.unpushed() {
        Some(info) => (Some(info.remote), Some(info.branch), info.remote_branch),
        None => (None, None, "remote".to_owned()),
    };

    prompt.set_busy(true);
    prompt.set_error(None);

    let Some(venus) = Venus::current() else {
        match git::push(&root, remote.as_deref(), branch.as_deref()) {
            Ok(()) => {
                prompt.set_busy(false);
                store.update(|space| {
                    space.message = Some(format!("Pushed to {remote_branch}"));
                });
                refresh_git_panel(store);
            }
            Err(err) => {
                prompt.set_busy(false);
                prompt.set_error(Some(err));
            }
        }
        return;
    };

    let prompt = Rc::clone(prompt);
    let store = store.clone();
    let blocking = venus.clone();
    venus.spawn(async move {
        let res = blocking
            .spawn_blocking(move || git::push(&root, remote.as_deref(), branch.as_deref()))
            .await;
        match res {
            Ok(()) => {
                prompt.set_busy(false);
                store.update(|space| {
                    space.message = Some(format!("Pushed to {remote_branch}"));
                });
                refresh_status(&store);
            }
            Err(err) => {
                prompt.set_busy(false);
                prompt.set_error(Some(err));
            }
        }
    });
}

fn sidebar_button(
    label: &'static str,
    fill: Color,
    hover_fill: Color,
    border: Option<Color>,
    text_color: Color,
    weight: FontWeight,
    on_press: impl Fn() + 'static,
) -> impl Widget {
    Button::new()
        .no_decor()
        .on_press(on_press)
        .build(move |state| {
            let bg = if state.is_hovered { hover_fill } else { fill };
            let mut decoration = BoxDecoration::new()
                .background_color(bg)
                .border_radius(5);
            if let Some(border_color) = border {
                let edge = || {
                    BorderSlice::new()
                        .color(border_color)
                        .style(BorderStyle::Solid)
                        .stroke(1.0)
                };
                decoration = decoration.border(
                    BoxBorder::new()
                        .top(edge())
                        .bottom(edge())
                        .left(edge())
                        .right(edge()),
                );
            }
            Container::new()
                .padding(pad(5, 8, 5, 8))
                .decoration(decoration)
                .child(
                    Center::new().child(
                        Text::new(label).text_style(
                            fonts::ui()
                                .font_size(11)
                                .color(text_color)
                                .font_weight(weight)
                                .text_overflow(TextOverflow::Clip),
                        ),
                    ),
                )
        })
}
