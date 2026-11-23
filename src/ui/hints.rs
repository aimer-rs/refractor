//! The inferred types and the other small notes that the language server has for a file, drawn in its lines.
//!
//! A server says them, for the whole file, a moment after the text stops changing and when it has finished loading the project.
//! What it said is kept as the offsets of the text it was asked about, and the area moves them by the edits made since: so the
//! notes keep to their names while the text is typed in, and are asked for again when it has settled.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use aimer::input::TextEditingController;
use aimer::input::input::{Inlay, Inlays, TextSnapshot};
use aimer::{Color, Venus};

use crate::ui::components::popup::Redraw;
use crate::ui::fonts;
use crate::services::lsp::{Hint, HintKind, Lsp, Status};

/// What the hints are drawn in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HintColors {
    /// The text of a hint.
    pub text: Color,
    /// The pill that it is in.
    pub pill: Color,
}

struct Context {
    lsp: Lsp,
    path: PathBuf,
    controller: TextEditingController,
    colors: HintColors,
}

/// What the server said, and the text it said it about.
struct Loaded {
    hints: Vec<Hint>,
    text: TextSnapshot,
}

/// The hints of one pane's file. There is one for each pane, and it lives as long as the pane does.
#[derive(Default)]
pub struct Hints {
    context: RefCell<Option<Context>>,
    loaded: RefCell<Option<Loaded>>,
    /// What was made of `loaded` for the colors it has, so that a build that changes nothing does not make it again.
    built: RefCell<Option<Inlays>>,
    /// The request that is waiting for its answer.
    request: Cell<Option<i64>>,
    /// Which wait for a refresh is the latest: an earlier one finds it is not and does nothing.
    generation: Cell<u64>,
    /// How many times in a row the hints were asked for again because the server could not say or was busy.
    attempts: Cell<u32>,
    /// Whether the server was idle the last time it was looked at, so that a refresh is asked for when it becomes so.
    idle: Cell<bool>,
    /// What draws the pane again when the hints change.
    changed: Redraw,
}

impl Hints {
    /// How long after the text settles the hints are asked for again.
    pub const EDIT_DELAY: Duration = Duration::from_millis(500);
    /// How long after a pane is shown, or the server is idle, they are asked for.
    pub const SHOWN_DELAY: Duration = Duration::from_millis(250);
    /// How long after the server could not say, or was busy, they are asked for again, and how many times at most: the pane is
    /// not built again when the server finishes loading the project, so it looks for itself.
    pub const RETRY_DELAY: Duration = Duration::from_secs(3);
    pub const MOST_RETRIES: u32 = 120;
    /// The most hints of a file that are drawn: a file with more has the first of them.
    pub const MOST: usize = 3000;

    /// Gives the cell the pane it is for, or tells it what changed of it. A new color makes the notes again.
    pub fn attach(
        &self,
        lsp: Lsp,
        path: PathBuf,
        controller: TextEditingController,
        colors: HintColors,
    ) {
        let recolored = self
            .context
            .borrow()
            .as_ref()
            .is_some_and(|context| context.colors != colors);
        if recolored {
            *self.built.borrow_mut() = None;
        }
        *self.context.borrow_mut() = Some(Context {
            lsp,
            path,
            controller,
            colors,
        });
    }

    /// Adds what is called to draw the pane again when the hints change, which says whether the pane is still there.
    pub fn on_change(&self, hook: Rc<dyn Fn() -> bool>) {
        self.changed.add(hook);
    }

    /// Asks for the hints again `delay` from now, unless they are asked for again before. Does nothing outside the runtime. This is
    /// for what is new: a pane shown, an edit that settled, a server that became idle; it starts the count of retries again.
    pub fn schedule(self: &Rc<Self>, delay: Duration) {
        self.attempts.set(0);
        self.schedule_load(delay);
    }

    /// Whether they are to be asked for again, and when: where the server could not say, and where it said while it was still
    /// loading the project, which is not all that it will say. Not for ever.
    fn retry_delay(failed: bool, idle: bool, attempts: u32) -> Option<Duration> {
        (attempts < Self::MOST_RETRIES && (failed || !idle)).then_some(Self::RETRY_DELAY)
    }

    /// Whether the server is ready and has nothing it is busy with.
    fn server_idle(&self) -> bool {
        self.context
            .borrow()
            .as_ref()
            .is_some_and(|context| {
                let summary = context.lsp.summary(&context.path);
                summary.status == Some(Status::Ready) && summary.progress.is_none()
            })
    }

    fn schedule_load(self: &Rc<Self>, delay: Duration) {
        let Some(venus) = Venus::current() else {
            return;
        };
        let wanted = self.generation.get() + 1;
        self.generation.set(wanted);
        let (this, waited) = (Rc::downgrade(self), venus.sleep(delay));
        venus.spawn(async move {
            // A runtime that has no timers answers at once.
            let _ = waited.await;
            if let Some(this) = this.upgrade()
                && this.generation.get() == wanted
            {
                this.refresh();
            }
        });
    }

    /// Looks at the server, from where the pane is built: when it has become idle, the hints are asked for.
    pub fn watch_server(self: &Rc<Self>) {
        let idle = self.server_idle();
        if idle && !self.idle.replace(true) {
            self.schedule(Self::SHOWN_DELAY);
        } else if !idle {
            self.idle.set(false);
        }
    }

    /// Asks the server for the hints of the whole file now. What it said is kept, and the pane is drawn again; where it could not
    /// say, what was kept stays.
    fn refresh(self: &Rc<Self>) {
        let Some((lsp, path, controller)) = self.context.borrow().as_ref().map(|context| {
            (
                context.lsp.clone(),
                context.path.clone(),
                context.controller.clone(),
            )
        }) else {
            return;
        };
        if let Some(old) = self.request.take() {
            lsp.cancel(old);
        }
        // The offsets are those of the text the server was asked about, and not of what it is by the time it answers.
        let text = controller.snapshot();
        // To the start of the last line, or past the end of a text that has no line break to end it: a range that goes past the end is
        // to the end, as the protocol has it.
        let lines = text.line_count();
        let this = Rc::downgrade(self);
        let id = lsp.inlay_hints(&path, 0..lines, move |hints| {
            let Some(this) = this.upgrade() else {
                return;
            };
            this.request.set(None);
            let failed = hints.is_none();
            if let Some(mut hints) = hints {
                hints.truncate(Self::MOST);
                // The server is asked again while it loads, and says the same: the pane is drawn again, which lays its text out, for
                // what is new and not for that. Nothing is none where none were said before.
                let same = match this.loaded.borrow().as_ref() {
                    Some(old) => old.hints == hints,
                    None => hints.is_empty(),
                };
                if !same {
                    *this.loaded.borrow_mut() = Some(Loaded { hints, text });
                    *this.built.borrow_mut() = None;
                    this.changed.fire();
                }
            }
            let attempts = this.attempts.get();
            if let Some(delay) = Self::retry_delay(failed, this.server_idle(), attempts) {
                this.attempts.set(attempts + 1);
                this.schedule_load(delay);
            }
        });
        // A request that could not be asked has been answered already, with none, and leaves nothing to wait for.
        self.request.set(id);
    }

    /// The notes to draw, made for the colors the pane has: none until the server has said some.
    pub fn inlays(&self) -> Option<Inlays> {
        if let Some(built) = self.built.borrow().as_ref() {
            return Some(built.clone());
        }
        let colors = self.context.borrow().as_ref()?.colors;
        let loaded = self.loaded.borrow();
        let loaded = loaded.as_ref()?;
        let items = loaded
            .hints
            .iter()
            .map(|hint| {
                // A type hint stands a little apart from its name, which the server may not say.
                let left = hint.pad_left || hint.kind == HintKind::Type;
                Inlay::new(hint.at, hint.label.clone(), colors.text.with_alpha(0.5))
                    .background(colors.pill.with_alpha(0.1))
                    .padded(left, hint.pad_right)
            })
            .collect();
        let built = Inlays::new(items).of_text(loaded.text.clone()).family(fonts::ui_family());
        *self.built.borrow_mut() = Some(built.clone());
        Some(built)
    }
}

#[cfg(test)]
#[path = "../tests/ui/hints.rs"]
mod tests;
