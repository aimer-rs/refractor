//! The colors of names that only the language server knows: whether a variable is mutable, whether a type is a struct or an enum.
//!
//! The grammar colors what it can tell from the text. The server's tokens are drawn over that, for the names that a rule of the
//! [scheme](Scheme) is for, and what no rule is for stays as the grammar left it. The tokens are asked for when the text has settled,
//! when the server says that they are not as they were, which is what it does when it has loaded the project and knows the names
//! that it did not, and again while it is busy. Until they come, and between an edit and the next time they come, they are moved
//! over what was typed, and a token that was typed in is left to the grammar.

mod scheme;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::Duration;

use aimer::Venus;
use aimer::input::TextEditingController;
use aimer::input::input::{SpanStyle, StyledRange, TextSnapshot};

pub use self::scheme::{Look, Rule, Scheme};
use crate::components::popup::Redraw;
use crate::lsp::{Lsp, SemanticTokens, Status};
use crate::settings::ThemeMode;

/// A token that a rule is for: its bytes, and which of the styles of the model it is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Styled {
    start: usize,
    end: usize,
    style: u16,
}

/// The tokens of the last answer with the style each has for a mode, and the text they are of.
struct Resolved {
    mode: ThemeMode,
    text: TextSnapshot,
    tokens: Vec<Styled>,
    styles: Vec<SpanStyle>,
}

impl Resolved {
    /// What the scheme of `mode` makes of `found`: the tokens that it has a rule for, each with the style of its rule.
    fn of(found: &SemanticTokens, mode: ThemeMode) -> Self {
        let scheme = Scheme::for_mode(mode);
        let mut styles: Vec<SpanStyle> = Vec::new();
        // What a kind and a set of modifiers make is worked out once, for there are many tokens and few of those.
        let mut known: HashMap<(u32, u32), Option<u16>> = HashMap::new();
        let mut tokens = Vec::with_capacity(found.tokens.len());
        for token in &found.tokens {
            let style = *known
                .entry((token.kind, token.modifiers))
                .or_insert_with(|| {
                    let kind = found.legend.type_name(token.kind)?;
                    let modifiers: Vec<&str> = found
                        .legend
                        .modifier_names(token.modifiers)
                        .collect();
                    let style = scheme.style_for(kind, &modifiers)?;
                    styles.push(style);
                    u16::try_from(styles.len() - 1).ok()
                });
            if let Some(style) = style {
                tokens.push(Styled {
                    start: token.range.start,
                    end: token.range.end,
                    style,
                });
            }
        }
        Self {
            mode,
            text: found.text.clone(),
            tokens,
            styles,
        }
    }

    /// The tokens as they are in `text`, which is the text they are of after the edits that were made: those after an edit are
    /// moved by what it added or took away, those it touched are gone, and those before it are as they were.
    fn moved_to(&self, text: &TextSnapshot) -> Vec<Styled> {
        let Some((old, new)) = self.text.diff(text) else {
            return self.tokens.clone();
        };
        self.tokens
            .iter()
            .filter_map(|token| {
                if token.end <= old.start {
                    Some(*token)
                } else if token.start >= old.end {
                    // Adding and taking away are of whole bytes, and a byte is never taken from before the start of the edit.
                    let start = token.start + new.len() - old.len();
                    Some(Styled {
                        start,
                        end: start + (token.end - token.start),
                        ..*token
                    })
                } else {
                    None
                }
            })
            .collect()
    }
}

/// What `base` says with the tokens drawn over it: each token is a range of the style of its rule, and the ranges of `base` are cut
/// round the tokens, so that what is of the grammar is what no token is over.
///
/// `base` is in the order of the text and has no byte in two of its ranges, as the grammar's highlight does, and `tokens` are in the
/// order of the text and do not overlap.
fn overlaid(base: Vec<StyledRange>, tokens: &[Styled], styles: &[SpanStyle]) -> Vec<StyledRange> {
    if tokens.is_empty() {
        return base;
    }
    let mut merged: Vec<StyledRange> = Vec::with_capacity(base.len() + tokens.len() * 2);
    for run in base {
        let mut from = run.range.start;
        let first = tokens.partition_point(|token| token.end <= run.range.start);
        for token in &tokens[first..] {
            if token.start >= run.range.end {
                break;
            }
            if from < token.start {
                merged.push(StyledRange::new(from..token.start, run.style.clone()));
            }
            from = from.max(token.end);
        }
        if from < run.range.end {
            merged.push(StyledRange::new(from..run.range.end, run.style.clone()));
        }
    }
    merged.extend(tokens.iter().map(|token| {
        StyledRange::new(
            token.start..token.end,
            styles[usize::from(token.style)].clone(),
        )
    }));
    merged.sort_by_key(|run| run.range.start);
    merged
}

/// The semantic tokens of one pane's file. There is one for each pane, and it lives as long as the pane does.
#[derive(Default)]
pub struct Semantic {
    context: RefCell<Option<Context>>,
    /// The last answer of the server.
    loaded: RefCell<Option<SemanticTokens>>,
    /// What was made of `loaded` for a mode.
    resolved: RefCell<Option<Rc<Resolved>>>,
    /// The tokens as they are in the text that was last highlighted, which is moved over the edits once for each text.
    moved: RefCell<Option<(TextSnapshot, Rc<Vec<Styled>>)>>,
    /// The request that is waiting for its answer.
    request: Cell<Option<i64>>,
    /// Which wait for a refresh is the latest: an earlier one finds it is not and does nothing.
    generation: Cell<u64>,
    /// How many times in a row the tokens were asked for again because the server could not say or was busy.
    attempts: Cell<u32>,
    /// Whether the server was idle the last time it was looked at, so that a refresh is asked for when it becomes so.
    idle: Cell<bool>,
    /// Whether the server has been asked to say when its tokens are not as they were.
    listening: Cell<bool>,
    /// What draws the pane again when the tokens change.
    changed: Redraw,
}

struct Context {
    lsp: Lsp,
    path: PathBuf,
    controller: TextEditingController,
    mode: ThemeMode,
}

impl Semantic {
    /// How long after the text settles the tokens are asked for again: they are moved over what is typed in the meantime.
    pub const EDIT_DELAY: Duration = Duration::from_millis(400);
    /// How long after a pane is shown, or the server is idle, or it says that its tokens are not as they were, they are asked for.
    pub const SHOWN_DELAY: Duration = Duration::from_millis(200);
    /// How long after the server could not say, or was busy, they are asked for again, and how many times at most.
    pub const RETRY_DELAY: Duration = Duration::from_secs(3);
    pub const MOST_RETRIES: u32 = 120;

    /// Gives the cell the pane it is for, or tells it what changed of it. A new mode makes the styles again.
    pub fn attach(
        self: &Rc<Self>,
        lsp: Lsp,
        path: PathBuf,
        controller: TextEditingController,
        mode: ThemeMode,
    ) {
        if !self.listening.replace(true) {
            // The server says when it has found out what the names it did not know are: the tokens are asked for then.
            let this: Weak<Self> = Rc::downgrade(self);
            lsp.on_semantic_refresh(Rc::new(move || match this.upgrade() {
                Some(this) => {
                    this.schedule(Self::SHOWN_DELAY);
                    true
                }
                None => false,
            }));
        }
        *self.context.borrow_mut() = Some(Context {
            lsp,
            path,
            controller,
            mode,
        });
    }

    /// Adds what is called to draw the pane again when the tokens change, which says whether the pane is still there.
    pub fn on_change(&self, hook: Rc<dyn Fn() -> bool>) {
        self.changed.add(hook);
    }

    /// Asks for the tokens again `delay` from now, unless they are asked for again before. Does nothing outside the runtime. This is
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

    /// Looks at the server, from where the pane is built: when it has become idle, the tokens are asked for.
    pub fn watch_server(self: &Rc<Self>) {
        let idle = self.server_idle();
        if idle && !self.idle.replace(true) {
            self.schedule(Self::SHOWN_DELAY);
        } else if !idle {
            self.idle.set(false);
        }
    }

    /// Asks the server for the tokens of the file now. What it said is kept, and the pane is drawn again if it is not what there was;
    /// where it could not say, what was kept stays.
    fn refresh(self: &Rc<Self>) {
        let Some((lsp, path)) = self
            .context
            .borrow()
            .as_ref()
            .map(|context| (context.lsp.clone(), context.path.clone()))
        else {
            return;
        };
        if let Some(old) = self.request.take() {
            lsp.cancel(old);
        }
        let this = Rc::downgrade(self);
        let id = lsp.semantic_tokens(&path, move |found| {
            let Some(this) = this.upgrade() else {
                return;
            };
            this.request.set(None);
            let failed = found.is_none();
            if let Some(found) = found {
                // The server is asked again while it loads and says the same: the pane is drawn again, which highlights its text, for
                // what is new. A text that is another with the same tokens is not new either: the tokens that are kept are moved over it.
                let same = this
                    .loaded
                    .borrow()
                    .as_ref()
                    .is_some_and(|old| {
                        old.tokens == found.tokens && old.text.diff(&found.text).is_none()
                    });
                if !same {
                    *this.loaded.borrow_mut() = Some(found);
                    *this.resolved.borrow_mut() = None;
                    *this.moved.borrow_mut() = None;
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

    /// The tokens as they are in `text`, with the styles they are drawn in, or none until the server has said some.
    fn tokens_in(&self, text: &TextSnapshot) -> Option<(Rc<Vec<Styled>>, Rc<Resolved>)> {
        let mode = self.context.borrow().as_ref()?.mode;
        let resolved = {
            let mut slot = self.resolved.borrow_mut();
            match slot.as_ref() {
                Some(resolved) if resolved.mode == mode => Rc::clone(resolved),
                _ => {
                    let loaded = self.loaded.borrow();
                    let resolved = Rc::new(Resolved::of(loaded.as_ref()?, mode));
                    *self.moved.borrow_mut() = None;
                    *slot = Some(Rc::clone(&resolved));
                    resolved
                }
            }
        };
        if let Some((held, tokens)) = self.moved.borrow().as_ref()
            && (held.ptr_eq(text) || held.diff(text).is_none())
        {
            return Some((Rc::clone(tokens), resolved));
        }
        let tokens = Rc::new(resolved.moved_to(text));
        *self.moved.borrow_mut() = Some((text.clone(), Rc::clone(&tokens)));
        Some((tokens, resolved))
    }

    /// What the grammar says of `range` of `text`, with the tokens over it. Left as it is until the server has said some.
    pub fn overlay(
        &self,
        base: Vec<StyledRange>,
        text: &TextSnapshot,
        range: Range<usize>,
    ) -> Vec<StyledRange> {
        let Some((tokens, resolved)) = self.tokens_in(text) else {
            return base;
        };
        // The tokens of the part that is asked about, and no others, which a long text has many of.
        let first = tokens.partition_point(|token| token.end <= range.start);
        let last = tokens.partition_point(|token| token.start < range.end);
        overlaid(base, &tokens[first..last.max(first)], &resolved.styles)
    }
}

#[cfg(test)]
mod tests;
