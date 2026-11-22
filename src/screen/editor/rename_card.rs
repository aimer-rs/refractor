use crate::components::popup::{Anchor, Popup};
use crate::fonts;
use crate::lsp::Lsp;
use crate::rename::{Asking, RenamePrompt, Verdict, judge};
use crate::theme::CodeTheme;
use crate::workspace::{Workspace, notifier};
use aimer::FocusNode;
use aimer::events::element::NamedKey;
use aimer::input::input::TextField;
use aimer::style::{LayoutSpacing, Theme};
use aimer::{
    BuildContext, Column, Container, NumberExt, Positioned, ProviderHandle, SizedBox, Stack, State,
    StateUpdater, StateView, StatefulWidget, Text, Widget, ZeroSizedBox,
};
use std::path::PathBuf;
use std::rc::Rc;

/// How wide the card is, and how tall: the name in its field, and under it what to press or what is wrong.
const WIDTH: f32 = 320.0;
const FIELD_HEIGHT: u32 = 32;
const NOTE_HEIGHT: u32 = 22;
const PADDING: u32 = 8;
/// What the card is as tall as: its field, its note, the space round them and the lines of the frame.
const HEIGHT: f32 = (FIELD_HEIGHT + NOTE_HEIGHT + 2 * PADDING) as f32 + 2.0;
const UI_SIZE: u32 = 13;

/// The card that asks what a name is to be renamed to: its field, with the name in it, beside the name; Return renames, and Escape gives it up.
///
/// It is drawn over the text area, which fills the place it is in, and it is drawn again when the prompt opens, shuts or has something to say
/// of what is typed. The rename itself is asked of the language server, and the answer is made in the documents of the workspace.
#[derive(Clone, StatefulWidget)]
pub struct RenameCard {
    prompt: Rc<RenamePrompt>,
    path: PathBuf,
    /// The focus of the text that is renamed in, which has it again when the card is gone.
    editor: FocusNode,
}

impl RenameCard {
    pub fn new(prompt: Rc<RenamePrompt>, path: PathBuf, editor: FocusNode) -> Self {
        Self {
            prompt,
            path,
            editor,
        }
    }
}

struct RenameCardState {
    prompt: Rc<RenamePrompt>,
    path: PathBuf,
    editor: FocusNode,
    /// What makes a build differ from the one before, so the framework builds it again.
    changes: u32,
}

impl State<RenameCard> for RenameCardState {
    fn create(widget: RenameCard) -> Self {
        Self {
            prompt: widget.prompt,
            path: widget.path,
            editor: widget.editor,
            changes: 0,
        }
    }

    fn on_mount(&mut self, updater: StateUpdater<Self>) {
        self.prompt
            .on_change(Popup::redraw_watcher(updater, |state| state.changes += 1));
    }

    fn adopt_config_from(&mut self, new: Self) {
        self.prompt = new.prompt;
        self.path = new.path;
        self.editor = new.editor;
    }

    fn build(this: StateView<Self>, ctx: &BuildContext) -> impl Widget {
        let _changes = this.changes;
        let Some(asking) = this.prompt.asking() else {
            return ZeroSizedBox.boxed();
        };
        let theme = *CodeTheme::of(ctx);
        let store = ProviderHandle::<Workspace>::of(ctx);
        let (prompt, editor, path) = (
            Rc::clone(&this.prompt),
            this.editor.clone(),
            this.path.clone(),
        );
        let line = if asking.spot.height > 0.0 {
            asking.spot.height
        } else {
            18.0
        };
        let (left, top) = Popup::place(
            Anchor {
                x: asking.spot.x,
                y: asking.spot.y,
                height: line,
            },
            (WIDTH, HEIGHT),
            asking.spot.extent,
        );

        let on_key = {
            let (prompt, asking, editor, path, store) = (
                Rc::clone(&prompt),
                Rc::clone(&asking),
                editor.clone(),
                path,
                store.clone(),
            );
            move |key: &NamedKey, _: &aimer::events::element::Modifiers| match key {
                NamedKey::Enter => {
                    submit(&prompt, &asking, &editor, &path, &store);
                    true
                }
                NamedKey::Escape => {
                    prompt.close();
                    editor.request_focus();
                    true
                }
                _ => false,
            }
        };
        let field = {
            let typed = Rc::clone(&prompt);
            TextField::new()
                .controller(asking.controller.clone())
                .focus_node(asking.focus.clone())
                .auto_focus(true)
                .text_style(fonts::ui().font_size(UI_SIZE).color(theme.text))
                .decoration(theme.editor)
                .padding(LayoutSpacing::new().left(8).right(8).top(7))
                .on_changed(move |_: String| typed.typed())
                .on_key(on_key)
        };
        // What is wrong with what was typed, or how to answer.
        let (note, color) = match prompt.complaint() {
            Some(complaint) => (complaint.to_owned(), theme.error_line),
            None => (
                format!(
                    "Rename {} \u{2014} Return to rename, Escape to cancel",
                    asking.old
                ),
                theme.muted,
            ),
        };
        let note = Container::new()
            .height(NOTE_HEIGHT)
            .padding(LayoutSpacing::new().top(5))
            .child(Text::new(note).text_style(fonts::ui().font_size(11).color(color)));
        let card = Container::new()
            .width(WIDTH)
            .fit_content()
            .decoration(Popup::frame(theme))
            .padding(LayoutSpacing::all(PADDING))
            .child(
                Column::new().children([
                    SizedBox::new()
                        .height(FIELD_HEIGHT)
                        .box_child(field)
                        .boxed(),
                    note.boxed(),
                ]),
            );
        // The layer fills what the card is over, so that the card is inside the bounds of what holds it.
        Container::new()
            .child(
                Stack::new().add_child(
                    Positioned::new()
                        .left(left.px())
                        .top(top.px())
                        .child(card),
                ),
            )
            .boxed()
    }
}

/// Answers the question: a name that is the one there is closes it, a name that cannot be one is complained of and the question stays, and a
/// name is asked of the language server, and the edits that it says are made when they come.
fn submit(
    prompt: &Rc<RenamePrompt>,
    asking: &Rc<Asking>,
    editor: &FocusNode,
    path: &std::path::Path,
    store: &ProviderHandle<Workspace>,
) {
    let typed = asking.controller.value().text().to_string();
    match judge(&asking.old, &typed) {
        Verdict::Same => {
            prompt.close();
            editor.request_focus();
        }
        Verdict::Invalid(why) => prompt.complain(why),
        Verdict::Name(name) => {
            prompt.close();
            editor.request_focus();
            let lsp: Lsp = store.read().lsp();
            let (store, notify) = (store.clone(), notifier(store));
            lsp.rename(path, asking.at, &name, move |result| match result {
                Ok(rename) => {
                    // Got ready inside an update of the workspace, and made outside it: a document that is changed tells the workspace, which is an
                    // update, and one inside another ends the app.
                    let mut planned = None;
                    let ready = notify.clone();
                    store.update(|space| planned = Some(space.plan_rename(&rename, ready)));
                    let done = planned
                        .expect("the update was made")
                        .and_then(|plan| plan.apply());
                    store.update(|space| space.report_rename(done));
                }
                Err(why) => store.update(|space| space.message = Some(why)),
            });
        }
    }
}
