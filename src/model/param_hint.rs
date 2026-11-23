//! Parameter hint state: tracks when the caret is inside a function or macro call's arguments, asks the language server
//! for `textDocument/signatureHelp`, and keeps the active signature's parameters for the caret popup to draw.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use aimer::Venus;
use aimer::input::TextEditingController;

use crate::ui::components::popup::Redraw;
use crate::services::lsp::{Lsp, SignatureHelp};
use crate::model::syntax::SyntaxHighlighter;

struct Context {
    lsp: Lsp,
    path: PathBuf,
    controller: TextEditingController,
    syntax: SyntaxHighlighter,
}

/// The parameter hint shown above the caret while inside a call's arguments. One per editor pane.
#[derive(Default)]
pub struct ParamHintCell {
    context: RefCell<Option<Context>>,
    shown: RefCell<Option<SignatureHelp>>,
    /// The in-flight `signatureHelp` request id, if any.
    request: Cell<Option<i64>>,
    /// Debounce generation: an older timer sees it was superseded and does nothing.
    generation: Cell<u64>,
    /// Opening `(` byte offset of the call the caret is currently inside, if any.
    current_call: Cell<Option<usize>>,
    /// Opening `(` byte offset of a call the user explicitly dismissed with Escape.
    dismissed_call: Cell<Option<usize>>,
    /// Last `(controller.revision(), caret)` that was queued, avoiding redundant queries.
    last_query: Cell<Option<(u64, usize)>>,
    redraw: Redraw,
}

impl ParamHintCell {
    /// Short delay to coalesce rapid caret movement or typing without feeling sluggish.
    pub const DEBOUNCE: Duration = Duration::from_millis(45);

    /// Connects this cell to the pane's language server, document, controller, and syntax tree.
    pub fn attach(
        &self,
        lsp: Lsp,
        path: PathBuf,
        controller: TextEditingController,
        syntax: SyntaxHighlighter,
    ) {
        *self.context.borrow_mut() = Some(Context {
            lsp,
            path,
            controller,
            syntax,
        });
    }

    /// Registers a watcher that redraws the caret popup whenever the shown parameters change.
    pub fn on_change(&self, hook: Rc<dyn Fn() -> bool>) {
        self.redraw.add(hook);
    }

    /// Reads the currently shown signature help, or `None` when hidden.
    pub fn with<T>(&self, read: impl FnOnce(Option<&SignatureHelp>) -> T) -> T {
        read(self.shown.borrow().as_ref())
    }

    /// Whether a parameter hint is currently shown.
    pub fn is_open(&self) -> bool {
        self.shown.borrow().is_some()
    }

    /// Dismisses the popup for the current call (e.g. on Escape). Returns `true` if a popup was open.
    pub fn dismiss(&self) -> bool {
        self.generation.set(self.generation.get() + 1);
        self.cancel_request();
        if let Some(call) = self.current_call.get() {
            self.dismissed_call.set(Some(call));
        }
        let was_open = self.shown.borrow_mut().take().is_some();
        if was_open {
            self.redraw.fire();
        }
        was_open
    }

    /// Hides the popup and clears all call tracking (e.g. when the caret leaves call arguments).
    pub fn hide(&self) {
        self.generation.set(self.generation.get() + 1);
        self.cancel_request();
        self.current_call.set(None);
        self.dismissed_call.set(None);
        self.last_query.set(None);
        if self.shown.borrow_mut().take().is_some() {
            self.redraw.fire();
        }
    }

    /// Re-evaluates the caret position: hides immediately if outside call arguments or if text is selected;
    /// otherwise schedules a debounced `textDocument/signatureHelp` request.
    pub fn refresh(self: &Rc<Self>) {
        let Some((controller, syntax)) = self
            .context
            .borrow()
            .as_ref()
            .map(|ctx| (ctx.controller.clone(), ctx.syntax.clone()))
        else {
            return;
        };
        let selection = controller.value().selection().range();
        if !selection.is_empty() {
            self.hide();
            return;
        }
        let caret = selection.end;
        let revision = controller.revision();
        if self.last_query.get() == Some((revision, caret)) {
            return;
        }
        let snapshot = controller.snapshot();
        let Some(open_paren) = syntax.call_arguments_at(&snapshot, caret) else {
            self.hide();
            return;
        };
        self.current_call.set(Some(open_paren));
        if self.dismissed_call.get() == Some(open_paren) {
            return;
        }
        if self.dismissed_call.get().is_some() {
            self.dismissed_call.set(None);
        }
        self.last_query.set(Some((revision, caret)));

        let Some(venus) = Venus::current() else {
            return;
        };
        let wanted = self.generation.get() + 1;
        self.generation.set(wanted);
        let (this, waited) = (Rc::downgrade(self), venus.sleep(Self::DEBOUNCE));
        venus.spawn(async move {
            let _ = waited.await;
            if let Some(this) = this.upgrade()
                && this.generation.get() == wanted
            {
                this.query(caret, open_paren);
            }
        });
    }

    fn query(self: &Rc<Self>, caret: usize, open_paren: usize) {
        let Some((lsp, path)) = self
            .context
            .borrow()
            .as_ref()
            .map(|ctx| (ctx.lsp.clone(), ctx.path.clone()))
        else {
            return;
        };
        self.cancel_request();
        let this = Rc::downgrade(self);
        let id = lsp.signature_help(&path, caret, move |help| {
            let Some(this) = this.upgrade() else {
                return;
            };
            this.request.set(None);
            // Ignore late replies if the caret left this call or the user dismissed it.
            if this.current_call.get() != Some(open_paren)
                || this.dismissed_call.get() == Some(open_paren)
            {
                return;
            }
            let changed = *this.shown.borrow() != help;
            if changed {
                *this.shown.borrow_mut() = help;
                this.redraw.fire();
            }
        });
        if id.is_none() {
            // Server was not ready yet; allow a subsequent refresh at the same caret to retry.
            self.last_query.set(None);
        }
        self.request.set(id);
    }

    fn cancel_request(&self) {
        if let Some(id) = self.request.take()
            && let Some(ctx) = self.context.borrow().as_ref()
        {
            ctx.lsp.cancel(id);
        }
    }
}

#[cfg(test)]
#[path = "../tests/model/param_hint.rs"]
mod tests;
