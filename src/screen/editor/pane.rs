use super::completion_caret::CompletionCaret;
use super::definition::find_definition;
use crate::completion::Completer;
use crate::components::marker::tinted;
use crate::document::{Document, Language};
use crate::fonts;
use crate::screen::editor::breadcrumb::Breadcrumb;
use crate::syntax::SyntaxHighlighter;
use crate::theme::CodeTheme;
use crate::workspace::{Workspace, notifier};
use aimer::input::input::{GutterMarker, GutterMarkers, LineBar, LineBars, LineNumbers};
use aimer::events::element::NamedKey;
use aimer::input::input::TextSnapshot;
use aimer::input::{ControllerListener, TextArea, TextEditingController};
use aimer::style::{LayoutSpacing, TextStyle, Theme};
use aimer::{
    BuildContext, Color, Container, Icons, Key, ProviderHandle, State, StateUpdater,
    StateView, StatefulWidget, Widget,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The code is set in 13 logical pixels of the monospace face.
const FONT_SIZE: u32 = 12;
/// How wide the column of buttons before the line numbers is.
const MARKER_SIZE: u32 = 10;
/// How wide the bar beside a rendered doc comment is.
const BAR_WIDTH: u32 = 2;

/// One document's code, as a text area. It is keyed by the document, so showing another document
/// makes a text area of its own, and one document keeps its text, caret and undo history in the
/// document for as long as it is open.
#[derive(Clone, StatefulWidget)]
pub struct EditorPane {
    key: Option<Key>,
    document: Rc<Document>,
}

impl EditorPane {
    pub fn new(document: Rc<Document>) -> Self {
        Self {
            key: Some(Key::from(("document", document.id))),
            document,
        }
    }
}

struct EditorPaneState {
    document: Rc<Document>,
    /// Asks for the pane to be built again, which a button in the margin does after it changes how a
    /// doc comment is shown: the highlighter is made again, and so is what it styled.
    updater: Rc<RefCell<Option<StateUpdater<Self>>>>,
    /// How many times a doc comment was toggled: what makes a build differ from the one before.
    toggles: u32,
    /// Watches the caret, for as long as the pane is on screen: asks for the pane to be built again when
    /// the area has to become read-only or editable, and shows a rendered doc comment that an edit
    /// got into as written.
    listener: Option<ControllerListener>,
    /// Whether the last build made the area read-only, for the listener to compare the caret with.
    guarded: Rc<Cell<bool>>,
    /// The text as one snapshot, shared by everything here that reads it line by line.
    snapshots: Rc<SnapshotCache>,
    /// Offers the keywords of the language as the text is typed. Made with the pane, so each
    /// document has its own menu, and none outlives its pane.
    completer: Rc<Completer>,
    /// Keeps the completer following the controller for as long as the pane is on screen.
    completion_listener: Option<ControllerListener>,
}

impl State<EditorPane> for EditorPaneState {
    fn create(widget: EditorPane) -> Self {
        Self {
            completer: Completer::new(widget.document.language, widget.document.controller.clone(), widget.document.completions()),
            document: widget.document,
            updater: Rc::default(),
            toggles: 0,
            listener: None,
            guarded: Rc::default(),
            snapshots: Rc::default(),
            completion_listener: None,
        }
    }

    fn on_mount(&mut self, updater: StateUpdater<Self>) {
        // A rendered doc comment cannot be edited, and only the button in the margin shows it as written.
        // The area is read-only while the caret or the selection is where an edit would change one,
        // which is worked out again when the caret moves, and the pane is built again when that changes.
        if let Some(syntax) = self.document.syntax.clone() {
            let controller = self.document.controller.clone();
            let rebuild = updater.clone();
            let guarded = Rc::clone(&self.guarded);
            let snapshots = Rc::clone(&self.snapshots);
            self.listener = Some(
                controller
                    .clone()
                    .listen(move |_| {
                        if guard_of(&syntax, &controller, &snapshots) != guarded.get() {
                            rebuild.try_set_state(|state| state.toggles += 1);
                        }
                    }),
            );
        }
        let completer = Rc::clone(&self.completer);
        self.completion_listener = Some(
            self.document
                .controller
                .clone()
                .listen(move |_| completer.refresh()),
        );
        // Closures made by the first build hold this cell, which is empty until now.
        *self.updater.borrow_mut() = Some(updater);
    }

    fn adopt_config_from(&mut self, _new: Self) {
        // The same document, shown again: nothing here changes.
    }

    fn build(this: StateView<Self>, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let mut area = TextArea::new()
            // The mark of the language server floats over the top-right of the text: the pointer is an arrow there.
            .pointer_holes(Rc::new(|width, _| super::lsp_badge::covered(width)) as aimer::input::input::PointerHoles)
            .controller(this.document.controller.clone())
            .focus_node(this.document.focus.clone())
            .expand(true)
            .min_lines(1)
            // Code scrolls sideways; prose wraps.
            .wrap(this.document.language == Language::Markdown)
            .decoration(Color::Transparent)
            .text_style(
                TextStyle::new()
                    .font_family(fonts::code_family())
                    .font_size(FONT_SIZE)
                    .color(theme.code),
            )
            .selection_color(theme.selection)
            // The mark that shows where a jump landed is in the color of the theme that marks what is current.
            .locate_color(theme.marker)
            .line_numbers(
                LineNumbers::new()
                    .color(theme.line_number)
                    .active_color(theme.line_number_active)
                    .gaps(8, 18),
            )
            .current_line(theme.current_line)
            .tab_spaces(super::TAB_SPACES)
            .padding(
                LayoutSpacing::new()
                    .top(8)
                    .bottom(8)
                    .right(16),
            );
        if let Some(syntax) = &this.document.syntax {
            let controller = this.document.controller.clone();
            // The build is what the listener compares the caret with, so it is made from the same
            // answer: read-only where an edit would change a rendered comment.
            let guarded = guard_of(syntax, &controller, &this.snapshots);
            this.guarded.set(guarded);
            area = area.read_only(guarded);
            // Backspace and Delete at the edge of a comment join the code beside it to the comment,
            // which the caret there does not say.
            let (guard, controller, snapshots) = (syntax.clone(), controller, Rc::clone(&this.snapshots));
            let completer = Rc::clone(&this.completer);
            area = area.on_key(move |key, _modifiers| {
                // An open menu takes the arrows, Return, Tab and Escape before the text does, and nothing leaves
                // the editor with Escape.
                if takes_key(&completer, key) {
                    return true;
                }
                let backwards = match key {
                    NamedKey::Backspace => true,
                    NamedKey::Delete => false,
                    _ => return false,
                };
                guard.refuses_deletion(
                    &snapshots.of(&controller),
                    controller
                        .value()
                        .selection()
                        .range(),
                    backwards,
                )
            });
            area = area.highlighter(syntax.highlighter());
            // The caret carries the menu, so the menu is wherever the word is being typed.
            let menu = Rc::clone(this.completer.menu());
            area = area.caret(move |context| CompletionCaret::new(context, Rc::clone(&menu), theme.code));
            area = area.gutter_markers(doc_markers(&this, theme));
            // Return in a doc comment goes on with `///`: in a rendered block the marker is out of
            // sight, and a plain new line would end the comment without anyone seeing it.
            area = area.line_break_prefix(crate::doc_comments::continuation);
            area = area.line_bars(doc_bars(&this));
            // A rendered doc comment is its markdown, as the viewer draws it, in place of its lines.
            area = area.embedder(syntax.doc_embedder());
        }
        if this.document.language == Language::Rust {
            // A force click or a command click on a name goes to where it is defined. The server is asked, and the answer is gone to when it
            // comes, which is not inside this: the workspace is only changed from an update of it.
            let store = ProviderHandle::<Workspace>::of(ctx);
            let path = this.document.path.clone();
            let go = move |offset: usize| {
                let lsp = store.read().lsp();
                let store = store.clone();
                find_definition(&lsp, &path, offset, move |place| {
                    let notify = notifier(&store);
                    store.update(|space| {
                        let _ = space.go_to_place(&place, notify);
                    });
                });
            };
            // The force click of a trackpad, and Command-click (Control-click off a Mac), go the same way.
            area = area.on_force_click(go.clone()).on_command_click(go);
            // Only a name is something to jump from: a word of a comment, a string or a raw string is not, and neither
            // is a keyword or a number. The syntax says which, from the parse the highlighting already keeps.
            if let Some(syntax) = &this.document.syntax {
                let (syntax, controller, snapshots) = (syntax.clone(), this.document.controller.clone(), Rc::clone(&this.snapshots));
                area = area.link_range(move |byte| syntax.link_at(&snapshots.of(&controller), byte));
            }
        }
        if this.document.syntax.is_none() {
            // A document that has no syntax has no menu, and its Escape is not the field's either.
            area = area.on_key(|key, _modifiers| matches!(key, NamedKey::Escape));
        }
        if let Some(markdown) = &this.document.markdown {
            // Rendered wherever the caret is not; the tables and code blocks it is not in are the
            // viewer's widgets.
            area = area
                .highlighter(markdown.highlighter())
                .embedder(markdown.embedder());
        }
        // The container gives the area the height of the panel, which it fills and scrolls in.
        Container::new().child(area)
    }
}

/// Whether the editor takes `key` before the text does: an open menu takes the arrows, Return, Tab and Escape, and
/// Escape is taken when there is none. Left to the text field, Escape makes it give up the focus, and the caret goes with
/// it: a press meant for a menu that is not there, because the server has not answered yet, is not to cost the caret.
fn takes_key(completer: &Completer, key: &NamedKey) -> bool {
    completer.handle_key(key) || matches!(key, NamedKey::Escape)
}

/// The snapshot of a document's text, made again only when the text or the caret changed. For a short
/// text `controller.snapshot()` builds a new one on every call, which costs the whole text, and the
/// margin asks for one on every visible line of every frame: with this it costs the lines on screen.
#[derive(Default)]
struct SnapshotCache {
    cached: RefCell<Option<(u64, TextSnapshot)>>,
}

impl SnapshotCache {
    /// The snapshot of the text `controller` holds now.
    fn of(&self, controller: &TextEditingController) -> TextSnapshot {
        // A revision is counted for each change of the value, the caret's too, so it never stands for
        // another text than the one it was made for.
        let revision = controller.revision();
        if let Some((cached, snapshot)) = &*self.cached.borrow() {
            if *cached == revision {
                return snapshot.clone();
            }
        }
        let snapshot = controller.snapshot();
        *self.cached.borrow_mut() = Some((revision, snapshot.clone()));
        snapshot
    }
}

/// Whether an edit made now would change a rendered doc comment, which cannot be edited while it is
/// rendered: the caret or the selection is on one, or next to it where a line break would be deleted.
fn guard_of(syntax: &SyntaxHighlighter, controller: &TextEditingController, snapshots: &SnapshotCache) -> bool {
    syntax.protects(
        &snapshots.of(controller),
        controller
            .value()
            .selection()
            .range(),
    )
}

/// The button before the number of the first line of each doc comment: a book open while the comment
/// is rendered, a pen while it is shown as written. Pressing it changes which, and builds the pane
/// again.
fn doc_markers(this: &StateView<EditorPaneState>, theme: CodeTheme) -> Option<GutterMarkers> {
    let syntax = this.document.syntax.clone()?;
    let document = Rc::clone(&this.document);
    let updater = Rc::clone(&this.updater);
    let snapshots = Rc::clone(&this.snapshots);
    Some(GutterMarkers::new(MARKER_SIZE, move |line| {
        let rendered = syntax.doc_block_at(&snapshots.of(&document.controller), line)?;
        let (icon, color) = if rendered {
            (Icons::BOOK_OPEN, theme.marker)
        } else {
            (Icons::PEN_LINE, theme.marker)
        };
        let (syntax, updater) = (syntax.clone(), Rc::clone(&updater));
        Some(GutterMarker::new(
            tinted(icon, color)
                .scene()
                .clone(),
            move || {
                syntax.toggle_doc_block(line);
                if let Some(updater) = updater.borrow().as_ref() {
                    updater.try_set_state(|state| state.toggles += 1);
                }
            },
        ))
    }))
}

/// The bar beside a rendered doc comment, down its whole height, at the comment's indent.
fn doc_bars(this: &StateView<EditorPaneState>) -> Option<LineBars> {
    let syntax = this.document.syntax.clone()?;
    let document = Rc::clone(&this.document);
    let snapshots = Rc::clone(&this.snapshots);
    let color = crate::markdown::DOC.with_alpha(0.7);
    Some(LineBars::new(BAR_WIDTH, move |line, _context| {
        let indent = syntax.doc_bar(&snapshots.of(&document.controller), line)?;
        Some(LineBar::new(
            color,
            crate::doc_comments::indent_px(indent).round() as u32,
        ))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_is_never_the_fields_so_that_the_editor_does_not_lose_its_caret() {
        let controller = TextEditingController::with_text("");
        let completer = Completer::new(Language::Rust, controller.clone(), None);
        let listener = {
            let completer = Rc::clone(&completer);
            controller.clone().listen(move |_| completer.refresh())
        };
        // Without a menu, Escape is taken all the same: left to the field it would give up the focus, and the caret with it.
        assert!(takes_key(&completer, &NamedKey::Escape), "no menu");
        // With one, it closes it, and that is the whole of what it does.
        controller.replace_range(0..0, "pu");
        assert!(completer.menu().is_open());
        assert!(takes_key(&completer, &NamedKey::Escape));
        assert!(!completer.menu().is_open());
        assert!(takes_key(&completer, &NamedKey::Escape), "and again");
        // Other keys are the field's when there is no menu.
        assert!(!takes_key(&completer, &NamedKey::ArrowLeft));
        assert!(!takes_key(&completer, &NamedKey::Backspace));
        drop(listener);
    }

    #[test]
    fn the_snapshot_is_made_once_for_a_text_and_again_for_an_edit() {
        let controller = TextEditingController::with_text("/// doc\nfn x() {}\n");
        let cache = SnapshotCache::default();
        let first = cache.of(&controller);
        assert!(first.ptr_eq(&cache.of(&controller)), "the same text is the same snapshot: this is what the margin asks for on each line");
        controller.select(3, 3);
        assert_eq!(cache.of(&controller).slice(0..7), "/// doc", "a move of the caret leaves the text as it was");
        controller.set_text("/// docs\nfn x() {}\n");
        let edited = cache.of(&controller);
        assert!(!first.ptr_eq(&edited), "an edit makes a new one");
        assert_eq!(edited.slice(0..8), "/// docs");
        assert!(edited.ptr_eq(&cache.of(&controller)));
    }
}



