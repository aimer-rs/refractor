//! The modal dialog that asks for a file or folder name (New File, New Folder, Rename) or confirms permanent deletion (Delete).
//!
//! Presented through [`aimer::modal::Modal`] so clicking outside the dialog or pressing Escape dismisses it cleanly without
//! leaking pointer events to the editor behind it, sharing the same frame and typography as [`super::rename_card::RenameCard`]
//! and [`super::search_popup::SearchPopup`].

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use aimer::events::element::NamedKey;
use aimer::input::button::Button;
use aimer::input::input::TextField;
use aimer::modal::{Modal, ModalHandle, ModalScope};
use aimer::style::{
    BorderSlice, BorderStyle, BoxBorder, BoxDecoration, LayoutSpacing, TextOverflow, Theme,
};
use aimer::{
    Alignment, AnyWidget, BoxAlignment, BuildContext, Center, Color, Column, Container, Expanded,
    Icons, ProviderHandle, Row, Shortcuts, SizedBox, State, StateUpdater, StateView,
    StatefulWidget, Text, Widget, ZeroSizedBox,
};

use crate::ui::components::icon::icon;
use crate::ui::components::popup::Popup;
use crate::ui::fonts;
use crate::ui::theme::CodeTheme;
use crate::services::tree_ops::{NameVerdict, validate_name};
use crate::ui::tree_prompt::{TreeAsking, TreePrompt, TreePromptKind};
use crate::model::workspace::{Workspace, notifier, refresh_status};

/// Width of the prompt card in logical pixels.
const WIDTH: f32 = 380.0;
const FIELD_HEIGHT: u32 = 32;
const PADDING: u32 = 12;

struct ActiveModal {
    asking: Rc<TreeAsking>,
    handle: ModalHandle,
}

/// Host widget that presents file tree creation, rename, and deletion prompts as a [`Modal`] screen.
#[derive(Clone, StatefulWidget)]
pub struct TreePromptCard {
    prompt: Rc<TreePrompt>,
}

impl TreePromptCard {
    pub fn new(prompt: Rc<TreePrompt>) -> Self {
        Self { prompt }
    }
}

struct TreePromptCardState {
    prompt: Rc<TreePrompt>,
    active: Rc<RefCell<Option<ActiveModal>>>,
    /// Incremented on each prompt change so the host synchronizes the modal.
    changes: u32,
}

impl Drop for TreePromptCardState {
    fn drop(&mut self) {
        if let Some(active) = self.active.borrow_mut().take() {
            active.handle.dismiss();
        }
    }
}

impl State<TreePromptCard> for TreePromptCardState {
    fn create(widget: TreePromptCard) -> Self {
        Self {
            prompt: widget.prompt,
            active: Rc::new(RefCell::new(None)),
            changes: 0,
        }
    }

    fn on_mount(&mut self, updater: StateUpdater<Self>) {
        let prompt = Rc::clone(&self.prompt);
        let active = Rc::clone(&self.active);
        let redraw = Popup::redraw_watcher(updater, |state| state.changes += 1);
        self.prompt.on_change(Rc::new(move || {
            if prompt.asking().is_none()
                && let Some(old) = active.borrow_mut().take()
            {
                old.handle.dismiss();
            }
            redraw()
        }));
    }

    fn adopt_config_from(&mut self, new: Self) {
        self.prompt = Rc::clone(&new.prompt);
    }

    fn build(this: StateView<Self>, ctx: &BuildContext) -> impl Widget {
        let _changes = this.changes;
        let Some(asking) = this.prompt.asking() else {
            let old = this.active.borrow_mut().take();
            if let Some(old) = old {
                old.handle.dismiss();
            }
            return ZeroSizedBox;
        };

        let already_showing = this
            .active
            .borrow()
            .as_ref()
            .is_some_and(|cur| Rc::ptr_eq(&cur.asking, &asking) && cur.handle.is_showing());
        if !already_showing {
            let old = this.active.borrow_mut().take();
            if let Some(old) = old {
                old.handle.dismiss();
            }
            let scope = ModalScope::capture(ctx);
            let handle = Modal::new()
                .alignment(Alignment::MidCenter)
                .barrier_dismissible(true)
                .escape_dismissible(true)
                .scope(scope)
                .child(TreePromptDialog::new(
                    Rc::clone(&this.prompt),
                    Rc::clone(&asking),
                ))
                .show();
            *this.active.borrow_mut() = Some(ActiveModal { asking, handle });
        }

        ZeroSizedBox
    }
}

/// The card content presented inside the [`Modal`] overlay.
#[derive(Clone, StatefulWidget)]
struct TreePromptDialog {
    prompt: Rc<TreePrompt>,
    asking: Rc<TreeAsking>,
}

impl TreePromptDialog {
    fn new(prompt: Rc<TreePrompt>, asking: Rc<TreeAsking>) -> Self {
        Self { prompt, asking }
    }
}

struct TreePromptDialogState {
    prompt: Rc<TreePrompt>,
    asking: Rc<TreeAsking>,
    /// True once mounted in the modal host; temporary states created for `adopt_config_from` stay false.
    mounted: bool,
    /// Incremented when validation complaints change so the dialog rebuilds in place.
    changes: u32,
}

impl Drop for TreePromptDialogState {
    fn drop(&mut self) {
        if self.mounted {
            self.prompt.close_if_same(&self.asking);
        }
    }
}

impl State<TreePromptDialog> for TreePromptDialogState {
    fn create(widget: TreePromptDialog) -> Self {
        Self {
            prompt: widget.prompt,
            asking: widget.asking,
            mounted: false,
            changes: 0,
        }
    }

    fn on_mount(&mut self, updater: StateUpdater<Self>) {
        self.mounted = true;
        self.prompt
            .on_change(Popup::redraw_watcher(updater, |state| state.changes += 1));
    }

    fn adopt_config_from(&mut self, new: Self) {
        self.prompt = Rc::clone(&new.prompt);
        self.asking = Rc::clone(&new.asking);
    }

    fn build(this: StateView<Self>, ctx: &BuildContext) -> impl Widget {
        let _changes = this.changes;
        let theme = *CodeTheme::of(ctx);
        let store = ProviderHandle::<Workspace>::of(ctx);
        let prompt = Rc::clone(&this.prompt);
        let asking = Rc::clone(&this.asking);

        let card: AnyWidget = match &asking.kind {
            TreePromptKind::Delete { entries } => {
                build_delete_card(theme, &prompt, &asking, &store, entries)
            }
            TreePromptKind::NewFile { .. }
            | TreePromptKind::NewFolder { .. }
            | TreePromptKind::Rename { .. } => build_input_card(theme, &prompt, &asking, &store),
        };

        // For Delete confirmation (which has no TextField), bind Return window-wide while open;
        // Escape and outside clicks are handled by the Modal itself.
        if matches!(asking.kind, TreePromptKind::Delete { .. }) {
            let confirm_prompt = Rc::clone(&prompt);
            let confirm_asking = Rc::clone(&asking);
            let confirm_store = store.clone();
            Shortcuts::new()
                .window(true)
                .bind_str("Enter", move || {
                    submit(&confirm_prompt, &confirm_asking, &confirm_store);
                })
                .child(card)
                .boxed()
        } else {
            card
        }
    }
}

fn build_input_card(
    theme: CodeTheme,
    prompt: &Rc<TreePrompt>,
    asking: &Rc<TreeAsking>,
    store: &ProviderHandle<Workspace>,
) -> AnyWidget {
    let (title, location, placeholder, hint_note) = {
        let space = store.read();
        match &asking.kind {
            TreePromptKind::NewFile { dir } => (
                "New File",
                location_label(&space, dir),
                "filename.rs",
                "Return to create, Escape to cancel",
            ),
            TreePromptKind::NewFolder { dir } => (
                "New Folder",
                location_label(&space, dir),
                "folder-name",
                "Return to create, Escape to cancel",
            ),
            TreePromptKind::Rename { old, is_dir, .. } => (
                if *is_dir {
                    "Rename Folder"
                } else {
                    "Rename File"
                },
                old.clone(),
                "new-name",
                "Return to rename, Escape to cancel",
            ),
            TreePromptKind::Delete { .. } => unreachable!(),
        }
    };

    let controller = asking
        .controller
        .clone()
        .expect("input prompt has a text controller");
    let on_key = {
        let (prompt, asking, store) = (Rc::clone(prompt), Rc::clone(asking), store.clone());
        move |key: &NamedKey, _: &aimer::events::element::Modifiers| match key {
            NamedKey::Enter => {
                submit(&prompt, &asking, &store);
                true
            }
            NamedKey::Escape => {
                prompt.close();
                true
            }
            _ => false,
        }
    };
    let typed = Rc::clone(prompt);
    let field = TextField::new()
        .controller(controller)
        .focus_node(asking.focus.clone())
        .auto_focus(true)
        .hint(placeholder)
        .hint_style(fonts::ui().font_size(13).color(theme.muted))
        .text_style(fonts::ui().font_size(13).color(theme.text))
        .decoration(theme.editor)
        .padding(LayoutSpacing::new().left(8).right(8).top(7))
        .on_changed(move |_: String| typed.typed())
        .on_key(on_key);

    let (note, note_color) = match prompt.complaint() {
        Some(complaint) => (complaint, theme.error_line),
        None => (hint_note.to_owned(), theme.muted),
    };

    let header = Row::new()
        .vertical_alignment(BoxAlignment::Center)
        .gaps(LayoutSpacing::all(8))
        .children([
            Text::new(title)
                .text_style(
                    fonts::ui()
                        .font_size(13)
                        .color(theme.text)
                        .font_weight(aimer::FontWeight::Bold),
                )
                .boxed(),
            Expanded::new()
                .child(
                    Text::new(location).text_style(
                        fonts::ui()
                            .font_size(12)
                            .color(theme.muted)
                            .text_overflow(TextOverflow::Clip),
                    ),
                )
                .boxed(),
        ]);

    Container::new()
        .width(WIDTH)
        .fit_content()
        .decoration(Popup::frame(theme))
        .padding(LayoutSpacing::all(PADDING))
        .child(
            Column::new()
                .gaps(LayoutSpacing::all(8))
                .children([
                    header.boxed(),
                    SizedBox::new()
                        .height(FIELD_HEIGHT)
                        .box_child(field)
                        .boxed(),
                    Text::new(note)
                        .text_style(fonts::ui().font_size(11).color(note_color))
                        .boxed(),
                ]),
        )
        .boxed()
}

fn build_delete_card(
    theme: CodeTheme,
    prompt: &Rc<TreePrompt>,
    asking: &Rc<TreeAsking>,
    store: &ProviderHandle<Workspace>,
    entries: &[(std::path::PathBuf, String, bool)],
) -> AnyWidget {
    let (title, detail) = match entries {
        [(_, name, true)] => (
            "Delete Folder".to_owned(),
            format!("Permanently delete \"{name}\" and all of its contents?"),
        ),
        [(_, name, false)] => (
            "Delete File".to_owned(),
            format!("Permanently delete \"{name}\"?"),
        ),
        many => {
            let count = many.len();
            (
                format!("Delete {count} Items"),
                format!("Permanently delete {count} selected items?"),
            )
        }
    };

    let cancel_btn = {
        let prompt = Rc::clone(prompt);
        action_button(
            "Cancel",
            Color::Transparent,
            theme.hover,
            Some(theme.divider),
            theme.text,
            aimer::FontWeight::Normal,
            move || prompt.close(),
        )
    };
    let delete_btn = {
        let (prompt, asking, store) = (Rc::clone(prompt), Rc::clone(asking), store.clone());
        action_button(
            "Delete",
            theme.marker.darken(0.22),
            theme.marker.darken(0.12),
            None,
            Color::WHITE,
            aimer::FontWeight::Bold,
            move || submit(&prompt, &asking, &store),
        )
    };
    let close_btn = {
        let prompt = Rc::clone(prompt);
        close_button(theme, move || prompt.close())
    };

    let header = Row::new()
        .vertical_alignment(BoxAlignment::Center)
        .children([
            Text::new(title)
                .text_style(
                    fonts::ui()
                        .font_size(13)
                        .color(theme.text)
                        .font_weight(aimer::FontWeight::Bold),
                )
                .boxed(),
            Expanded::new().child(ZeroSizedBox).boxed(),
            close_btn.boxed(),
        ]);

    let mut rows: Vec<AnyWidget> = vec![
        header.boxed(),
        Text::new(detail)
            .text_style(fonts::ui().font_size(12).color(theme.muted))
            .boxed(),
    ];
    if let Some(complaint) = prompt.complaint() {
        rows.push(
            Text::new(complaint)
                .text_style(
                    fonts::ui()
                        .font_size(11)
                        .color(theme.error_line),
                )
                .boxed(),
        );
    }
    rows.push(
        Row::new()
            .vertical_alignment(BoxAlignment::Center)
            .gaps(LayoutSpacing::all(8))
            .children([
                Expanded::new().child(ZeroSizedBox).boxed(),
                cancel_btn.boxed(),
                delete_btn.boxed(),
            ])
            .boxed(),
    );

    Container::new()
        .width(WIDTH)
        .fit_content()
        .decoration(Popup::frame(theme))
        .padding(LayoutSpacing::all(PADDING))
        .child(
            Column::new()
                .gaps(LayoutSpacing::all(10))
                .children(rows),
        )
        .boxed()
}

fn close_button(theme: CodeTheme, on_press: impl Fn() + 'static) -> impl Widget {
    Button::new()
        .no_decor()
        .on_press(on_press)
        .build(move |state| {
            let bg = if state.is_hovered {
                theme.hover
            } else {
                Color::Transparent
            };
            let fg = if state.is_hovered {
                theme.text
            } else {
                theme.muted
            };
            Container::new()
                .padding(LayoutSpacing::all(3))
                .decoration(
                    BoxDecoration::new()
                        .background_color(bg)
                        .border_radius(4),
                )
                .child(icon(Icons::X, 14, fg))
        })
}

fn action_button(
    label: &'static str,
    fill: Color,
    hover_fill: Color,
    border: Option<Color>,
    text_color: Color,
    weight: aimer::FontWeight,
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
                .padding(
                    LayoutSpacing::new()
                        .left(12)
                        .right(12)
                        .top(5)
                        .bottom(5),
                )
                .decoration(decoration)
                .child(
                    Center::new().child(
                        Text::new(label).text_style(
                            fonts::ui()
                                .font_size(12)
                                .color(text_color)
                                .font_weight(weight),
                        ),
                    ),
                )
        })
}

fn location_label(space: &Workspace, dir: &Path) -> String {
    let rel = space.relative(dir);
    if rel.as_os_str().is_empty() || dir == space.root {
        format!("in {}", space.name())
    } else {
        format!("in {}", rel.display())
    }
}

/// Validates and executes the prompt's filesystem operation, updating the workspace and git status on success or showing an error
/// message inside the card on failure.
fn submit(prompt: &Rc<TreePrompt>, asking: &Rc<TreeAsking>, store: &ProviderHandle<Workspace>) {
    match &asking.kind {
        TreePromptKind::NewFile { dir } => {
            let typed = asking
                .controller
                .as_ref()
                .map(|c| c.value().text().to_string())
                .unwrap_or_default();
            match validate_name(&typed, None) {
                NameVerdict::Invalid(why) => prompt.complain(why),
                NameVerdict::Same => prompt.close(),
                NameVerdict::Valid(name) => {
                    let notify = notifier(store);
                    let mut outcome = Ok(());
                    store.update(|space| {
                        outcome = space
                            .create_file(dir, &name, notify)
                            .map(|_| ());
                    });
                    match outcome {
                        Ok(()) => {
                            prompt.close();
                            refresh_status(store);
                        }
                        Err(error) => prompt.complain(error.to_string()),
                    }
                }
            }
        }
        TreePromptKind::NewFolder { dir } => {
            let typed = asking
                .controller
                .as_ref()
                .map(|c| c.value().text().to_string())
                .unwrap_or_default();
            match validate_name(&typed, None) {
                NameVerdict::Invalid(why) => prompt.complain(why),
                NameVerdict::Same => prompt.close(),
                NameVerdict::Valid(name) => {
                    let mut outcome = Ok(());
                    store.update(|space| {
                        outcome = space.create_folder(dir, &name).map(|_| ());
                    });
                    match outcome {
                        Ok(()) => {
                            prompt.close();
                            refresh_status(store);
                        }
                        Err(error) => prompt.complain(error.to_string()),
                    }
                }
            }
        }
        TreePromptKind::Rename { path, old, .. } => {
            let typed = asking
                .controller
                .as_ref()
                .map(|c| c.value().text().to_string())
                .unwrap_or_default();
            match validate_name(&typed, Some(old)) {
                NameVerdict::Invalid(why) => prompt.complain(why),
                NameVerdict::Same => prompt.close(),
                NameVerdict::Valid(name) => {
                    let notify = notifier(store);
                    let mut outcome = Ok(());
                    store.update(|space| {
                        outcome = space
                            .rename_path(path, &name, notify)
                            .map(|_| ());
                    });
                    match outcome {
                        Ok(()) => {
                            prompt.close();
                            refresh_status(store);
                        }
                        Err(error) => prompt.complain(error.to_string()),
                    }
                }
            }
        }
        TreePromptKind::Delete { entries } => {
            let paths: Vec<std::path::PathBuf> = entries
                .iter()
                .map(|(path, _, _)| path.clone())
                .collect();
            let mut outcome = Ok(());
            store.update(|space| {
                outcome = space.delete_paths(&paths);
            });
            match outcome {
                Ok(()) => {
                    prompt.close();
                    refresh_status(store);
                }
                Err(error) => prompt.complain(error.to_string()),
            }
        }
    }
}
