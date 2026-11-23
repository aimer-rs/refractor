//! The modal dialog that jumps to a line or `line:column` (for example `80` or `80:80`) in the active editor document.
//!
//! Presented through [`aimer::modal::Modal`] so clicking outside the dialog or pressing Escape dismisses it cleanly,
//! matching [`super::tree_prompt_card::TreePromptCard`].

use std::cell::RefCell;
use std::rc::Rc;

use aimer::events::element::NamedKey;
use aimer::input::input::TextField;
use aimer::modal::{Modal, ModalHandle, ModalScope};
use aimer::style::{LayoutSpacing, TextOverflow, Theme};
use aimer::{
    Alignment, BoxAlignment, BuildContext, Column, Container, Expanded, ProviderHandle, Row,
    SizedBox, State, StateUpdater, StateView, StatefulWidget, Text, Widget, ZeroSizedBox,
};

use crate::ui::components::popup::Popup;
use crate::ui::fonts;
use crate::model::go_to_line::{
    GoToLineAsking, GoToLinePrompt, LineColVerdict, parse_line_column, resolve_byte_offset,
};
use crate::ui::theme::CodeTheme;
use crate::model::workspace::Workspace;

const WIDTH: f32 = 380.0;
const FIELD_HEIGHT: u32 = 32;
const PADDING: u32 = 12;

struct ActiveModal {
    asking: Rc<GoToLineAsking>,
    handle: ModalHandle,
}

/// Host widget that presents the Go to Line/Column dialog as a [`Modal`] screen.
#[derive(Clone, StatefulWidget)]
pub struct GoToLineCard {
    prompt: Rc<GoToLinePrompt>,
}

impl GoToLineCard {
    pub fn new(prompt: Rc<GoToLinePrompt>) -> Self {
        Self { prompt }
    }
}

struct GoToLineCardState {
    prompt: Rc<GoToLinePrompt>,
    active: Rc<RefCell<Option<ActiveModal>>>,
    /// Incremented on each prompt change so the host synchronizes the modal.
    changes: u32,
}

impl Drop for GoToLineCardState {
    fn drop(&mut self) {
        if let Some(active) = self.active.borrow_mut().take() {
            active.handle.dismiss();
        }
    }
}

impl State<GoToLineCard> for GoToLineCardState {
    fn create(widget: GoToLineCard) -> Self {
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
                .child(GoToLineDialog::new(
                    Rc::clone(&this.prompt),
                    Rc::clone(&asking),
                ))
                .show();
            *this.active.borrow_mut() = Some(ActiveModal { asking, handle });
        }

        ZeroSizedBox
    }
}

#[derive(Clone, StatefulWidget)]
struct GoToLineDialog {
    prompt: Rc<GoToLinePrompt>,
    asking: Rc<GoToLineAsking>,
}

impl GoToLineDialog {
    fn new(prompt: Rc<GoToLinePrompt>, asking: Rc<GoToLineAsking>) -> Self {
        Self { prompt, asking }
    }
}

struct GoToLineDialogState {
    prompt: Rc<GoToLinePrompt>,
    asking: Rc<GoToLineAsking>,
    /// True once mounted in the modal host; temporary states created for `adopt_config_from` stay false.
    mounted: bool,
    changes: u32,
}

impl Drop for GoToLineDialogState {
    fn drop(&mut self) {
        if self.mounted {
            self.prompt.close_if_same(&self.asking);
        }
    }
}

impl State<GoToLineDialog> for GoToLineDialogState {
    fn create(widget: GoToLineDialog) -> Self {
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

        let position_label = {
            let space = store.read();
            space
                .active_document()
                .map(|doc| {
                    let (line, col) = doc.controller.cursor_position();
                    let total = doc.controller.line_count();
                    format!("Ln {line}, Col {col} of {total}")
                })
                .unwrap_or_default()
        };

        let on_key = {
            let (prompt, asking, store) = (Rc::clone(&prompt), Rc::clone(&asking), store.clone());
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

        let typed = Rc::clone(&prompt);
        let field = TextField::new()
            .controller(asking.controller.clone())
            .focus_node(asking.focus.clone())
            .auto_focus(true)
            .hint("line or line:column")
            .hint_style(fonts::ui().font_size(13).color(theme.muted))
            .text_style(fonts::ui().font_size(13).color(theme.text))
            .decoration(theme.editor)
            .padding(LayoutSpacing::new().left(8).right(8).top(7))
            .on_changed(move |_: String| typed.typed())
            .on_key(on_key);

        let (note, note_color) = match prompt.complaint() {
            Some(complaint) => (complaint, theme.error_line),
            None => ("Return to jump, Escape to cancel".to_owned(), theme.muted),
        };

        let header = Row::new()
            .vertical_alignment(BoxAlignment::Center)
            .gaps(LayoutSpacing::all(8))
            .children([Text::new("Go to Line").text_style(
                fonts::ui()
                    .font_size(13)
                    .color(theme.text)
                    .font_weight(aimer::FontWeight::Bold),
            )]);

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
    }
}

fn submit(
    prompt: &Rc<GoToLinePrompt>,
    asking: &Rc<GoToLineAsking>,
    store: &ProviderHandle<Workspace>,
) {
    let typed = asking.controller.value().text().to_string();
    match parse_line_column(&typed) {
        LineColVerdict::Empty => prompt.close(),
        LineColVerdict::Invalid(why) => prompt.complain(why),
        LineColVerdict::Valid { line, column } => {
            let document = store.read().active_document().cloned();
            prompt.close();
            if let Some(doc) = document {
                let snapshot = doc.controller.snapshot();
                let byte = resolve_byte_offset(&snapshot, line, column);
                doc.show(byte..byte);
            }
        }
    }
}
