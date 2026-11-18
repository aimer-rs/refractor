use crate::components::marker::tinted;
use crate::document::{Document, Language};
use crate::fonts;
use crate::theme::CodeTheme;
use aimer::input::{ControllerListener, TextArea};
use aimer::input::input::{GutterMarker, GutterMarkers, LineBar, LineBars, LineNumbers};
use aimer::style::{LayoutSpacing, TextStyle, Theme};
use aimer::{BuildContext, Color, Container, Icons, Key, State, StateUpdater, StateView, StatefulWidget, Widget};
use std::cell::RefCell;
use std::rc::Rc;

/// The code is set in 13 logical pixels of the monospace face.
const FONT_SIZE: u32 = 13;
/// How wide the column of buttons before the line numbers is.
const MARKER_SIZE: u32 = 12;
/// How wide the bar beside a rendered doc comment is.
const BAR_WIDTH: u32 = 3;

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
    /// Opens a rendered doc comment that is typed into, for as long as the pane is on screen.
    listener: Option<ControllerListener>,
}

impl State<EditorPane> for EditorPaneState {
    fn create(widget: EditorPane) -> Self {
        Self {
            document: widget.document,
            updater: Rc::default(),
            toggles: 0,
            listener: None,
        }
    }

    fn on_mount(&mut self, updater: StateUpdater<Self>) {
        // A rendered doc comment has nothing in it to type into, so the first edit that lands in one
        // shows it as written, where the edit can be seen. Moving the caret through one does not.
        if let Some(syntax) = self.document.syntax.clone() {
            let controller = self.document.controller.clone();
            let last = std::cell::RefCell::new(controller.snapshot());
            let opened = updater.clone();
            self.listener = Some(controller.clone().listen(move |_| {
                let now = controller.snapshot();
                let changed = !now.ptr_eq(&last.borrow());
                *last.borrow_mut() = now.clone();
                if changed && syntax.open_doc_block_at(&now, controller.cursor_position().0.saturating_sub(1)) {
                    opened.try_set_state(|state| state.toggles += 1);
                }
            }));
        }
        // Closures made by the first build hold this cell, which is empty until now.
        *self.updater.borrow_mut() = Some(updater);
    }

    fn adopt_config_from(&mut self, _new: Self) {
        // The same document, shown again: nothing here changes.
    }

    fn build(this: StateView<Self>, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let mut area = TextArea::new()
            .controller(this.document.controller.clone())
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
            area = area.highlighter(syntax.highlighter());
            area = area.gutter_markers(doc_markers(&this, theme));
            // Return in a doc comment goes on with `///`: in a rendered block the marker is out of
            // sight, and a plain new line would end the comment without anyone seeing it.
            area = area.line_break_prefix(crate::doc_comments::continuation);
            area = area.line_bars(doc_bars(&this));
            // A rendered doc comment is its markdown, as the viewer draws it, in place of its lines.
            let (opener, updater) = (syntax.clone(), Rc::clone(&this.updater));
            area = area.embedder(syntax.doc_embedder(Rc::new(move |line| {
                opener.toggle_doc_block(line);
                if let Some(updater) = updater.borrow().as_ref() {
                    updater.try_set_state(|state| state.toggles += 1);
                }
            })));
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

/// The button before the number of the first line of each doc comment: a book open while the comment
/// is rendered, a pen while it is shown as written. Pressing it changes which, and builds the pane
/// again.
fn doc_markers(this: &StateView<EditorPaneState>, theme: CodeTheme) -> Option<GutterMarkers> {
    let syntax = this.document.syntax.clone()?;
    let document = Rc::clone(&this.document);
    let updater = Rc::clone(&this.updater);
    Some(GutterMarkers::new(MARKER_SIZE, move |line| {
        let rendered = syntax.doc_block_at(&document.controller.value().snapshot(), line)?;
        let (icon, color) = if rendered {
            (Icons::BOOK_OPEN, theme.marker)
        } else {
            (Icons::PEN_LINE, theme.marker)
        };
        let (syntax, updater) = (syntax.clone(), Rc::clone(&updater));
        Some(GutterMarker::new(tinted(icon, color).scene().clone(), move || {
            syntax.toggle_doc_block(line);
            if let Some(updater) = updater.borrow().as_ref() {
                updater.try_set_state(|state| state.toggles += 1);
            }
        }))
    }))
}

/// The bar beside a rendered doc comment, down its whole height, at the comment's indent.
fn doc_bars(this: &StateView<EditorPaneState>) -> Option<LineBars> {
    let syntax = this.document.syntax.clone()?;
    let document = Rc::clone(&this.document);
    let color = crate::markdown::DOC.with_alpha(0.7);
    Some(LineBars::new(BAR_WIDTH, move |line, _context| {
        let indent = syntax.doc_bar(&document.controller.value().snapshot(), line)?;
        Some(LineBar::new(color, crate::doc_comments::indent_px(indent).round() as u32))
    }))
}
