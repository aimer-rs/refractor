//! The lines under the code that the language server says is wrong: red under an error, yellow under a warning, and a quiet one under
//! what it thinks is worth saying and not worth worrying about.
//!
//! What the server says is of the text it has, and a moment after the user types it has said nothing of what was typed. What was
//! said last is kept, with the text it is of, and the area moves it over the edits made since, so that a line stays under its
//! word while it is typed beside, and is replaced when the server has said what is wrong with the new text.

use std::cell::RefCell;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use aimer::Color;
use aimer::input::input::{SpanStyle, StyledRange, TextSnapshot, Underline, Underlines};
use aimer::input::{ControllerListener, TextEditingController};

use crate::services::cargo::check::Severity;
use crate::services::cargo::model::Manifest;
use crate::ui::components::popup::Redraw;
use crate::services::lsp::{Diagnostic, Lsp, Weight};

/// The colors of the lines, by how wrong the thing is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineColors {
    pub error: Color,
    pub warning: Color,
    /// What the server calls information and hints, which the editor calls weak warnings.
    pub weak: Color,
}

impl LineColors {
    fn of(&self, weight: Weight) -> Color {
        match weight {
            Weight::Error => self.error,
            Weight::Warning => self.warning,
            Weight::Weak => self.weak,
        }
    }
}

/// One line to draw: the bytes, how wrong they are, and what the server said of it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Mark {
    range: Range<usize>,
    weight: Weight,
    message: String,
    code: Option<String>,
    /// Unused/unnecessary code is grayed out rather than underlined.
    unused: bool,
}

/// A problem at a place, as it is said in a card: what the server said of it, and where it is in the text as it is now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub weight: Weight,
    pub message: String,
    /// The server's name for the kind of problem, such as an error number.
    pub code: Option<String>,
    /// The bytes the line is under, in the text as it is now.
    pub range: Range<usize>,
}

/// What the server said, and the text it said it about.
struct Loaded {
    marks: Vec<Mark>,
    text: TextSnapshot,
}

struct Context {
    /// The server that says what is wrong; none for a manifest, which the schema checks.
    lsp: Option<Lsp>,
    path: PathBuf,
    controller: TextEditingController,
    colors: LineColors,
}

/// The lines under the code of one pane's file. There is one for each pane, and it lives as long as the pane does.
#[derive(Default)]
pub struct ProblemLines {
    context: RefCell<Option<Context>>,
    loaded: RefCell<Option<Loaded>>,
    /// What was made of `loaded` for the colors it has, so that a build that changes nothing does not make it again.
    built: RefCell<Option<Underlines>>,
    listening: std::cell::Cell<bool>,
    /// What draws the pane again when the lines change.
    changed: Redraw,
    /// What is read of the text where the file is a manifest.
    manifest: Manifest,
    /// Which check of the manifest is wanted, so that one that is overtaken by another is not made.
    generation: std::cell::Cell<u64>,
    /// What looks at the text of a manifest for an edit.
    watching: RefCell<Option<ControllerListener>>,
}

impl ProblemLines {
    /// The most lines a file has: one with more has the worst of them.
    pub const MOST: usize = 1000;

    /// Gives the cell the pane it is for, or tells it what changed of it. A new color makes the lines again.
    pub fn attach(
        self: &Rc<Self>,
        lsp: Lsp,
        path: PathBuf,
        controller: TextEditingController,
        colors: LineColors,
    ) {
        let recolored = self
            .context
            .borrow()
            .as_ref()
            .is_some_and(|context| context.colors != colors);
        if recolored {
            *self.built.borrow_mut() = None;
        }
        let first = !self.listening.replace(true);
        *self.context.borrow_mut() = Some(Context {
            lsp: Some(lsp.clone()),
            path,
            controller,
            colors,
        });
        if first {
            // The server is asked what is wrong each time what it says may have changed, and what it said before is looked at now.
            let this: Weak<Self> = Rc::downgrade(self);
            lsp.on_diagnostics(Rc::new(move || match this.upgrade() {
                Some(this) => {
                    this.refresh();
                    true
                }
                None => false,
            }));
            self.refresh();
        }
    }

    /// How long after the last edit of a manifest it is checked again.
    pub const MANIFEST_DELAY: std::time::Duration = std::time::Duration::from_millis(300);

    /// As [`ProblemLines::attach`], for a manifest: nothing is asked of a server, and the text is checked by the schema, now and a moment
    /// after each edit.
    pub fn attach_manifest(
        self: &Rc<Self>,
        path: PathBuf,
        controller: TextEditingController,
        colors: LineColors,
    ) {
        let recolored = self
            .context
            .borrow()
            .as_ref()
            .is_some_and(|context| context.colors != colors);
        if recolored {
            *self.built.borrow_mut() = None;
        }
        let first = !self.listening.replace(true);
        *self.context.borrow_mut() = Some(Context {
            lsp: None,
            path,
            controller: controller.clone(),
            colors,
        });
        if first {
            let (this, last) = (Rc::downgrade(self), RefCell::new(controller.value()));
            let watched = controller.clone();
            *self.watching.borrow_mut() = Some(controller.listen(move |_| {
                // Only a change of the text is an edit: a move of the caret changes what the controller holds too.
                let now = watched.value();
                if now.same_text(&last.borrow()) {
                    return;
                }
                *last.borrow_mut() = now;
                if let Some(this) = this.upgrade() {
                    this.schedule(Self::MANIFEST_DELAY);
                }
            }));
            self.refresh();
        }
    }

    /// Checks the manifest after `delay`, unless another check is wanted by then. Where there is no runtime there is no moment, and nothing
    /// is done.
    fn schedule(self: &Rc<Self>, delay: std::time::Duration) {
        let Some(venus) = aimer::Venus::current() else {
            return;
        };
        let wanted = self.generation.get() + 1;
        self.generation.set(wanted);
        let (this, waited) = (Rc::downgrade(self), venus.sleep(delay));
        venus.spawn(async move {
            let _ = waited.await;
            if let Some(this) = this.upgrade()
                && this.generation.get() == wanted
            {
                this.refresh();
            }
        });
    }

    /// What is read of the text where the file is a manifest, which is kept for the text it is of.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Adds what is called to draw the pane again when the lines change, which says whether the pane is still there.
    pub fn on_change(&self, hook: Rc<dyn Fn() -> bool>) {
        self.changed.add(hook);
    }

    /// Looks at what the server says is wrong, and draws the pane again if it is not what it was. What was kept stays while the
    /// server is behind the text and has said nothing of it yet.
    fn refresh(&self) {
        let Some((lsp, path, controller)) = self.context.borrow().as_ref().map(|context| {
            (
                context.lsp.clone(),
                context.path.clone(),
                context.controller.clone(),
            )
        }) else {
            return;
        };
        let (marks, text) = match lsp {
            Some(lsp) => {
                let Some((found, text)) = lsp.diagnostics(&path) else {
                    return;
                };
                (marks_of(&found, &text), text)
            }
            None => {
                let text = controller.snapshot();
                (self.manifest_marks(&text), text)
            }
        };
        let same = match self.loaded.borrow().as_ref() {
            Some(old) => old.marks == marks && old.text.diff(&text).is_none(),
            None => marks.is_empty(),
        };
        if same {
            return;
        }
        *self.loaded.borrow_mut() = Some(Loaded { marks, text });
        *self.built.borrow_mut() = None;
        self.changed.fire();
    }

    /// The lines for what the schema finds wrong in the manifest `text`.
    fn manifest_marks(&self, text: &TextSnapshot) -> Vec<Mark> {
        let marks = self
            .manifest
            .problems(text)
            .into_iter()
            .filter_map(|problem| {
                Some(Mark {
                    range: under(&problem.span, text)?,
                    weight: match problem.severity {
                        Severity::Error => Weight::Error,
                        Severity::Warning => Weight::Warning,
                    },
                    message: problem.message,
                    code: None,
                    unused: false,
                })
            })
            .collect();
        ordered(marks)
    }

    /// The problems at the byte `byte` of the text as it is now: those whose line is under it, the worst first. What the server said is of
    /// the text it had, and a range is moved over what was typed since, as the lines are.
    pub fn at(&self, byte: usize) -> Vec<Found> {
        let loaded = self.loaded.borrow();
        let Some(loaded) = loaded.as_ref() else {
            return Vec::new();
        };
        let Some(controller) = self
            .context
            .borrow()
            .as_ref()
            .map(|context| context.controller.clone())
        else {
            return Vec::new();
        };
        let edit = loaded.text.diff(&controller.snapshot());
        let mut found: Vec<Found> = loaded
            .marks
            .iter()
            .filter_map(|mark| {
                let range = moved(&mark.range, edit.as_ref())?;
                (range.start <= byte && byte < range.end).then(|| Found {
                    weight: mark.weight,
                    message: mark.message.clone(),
                    code: mark.code.clone(),
                    range,
                })
            })
            .collect();
        found.sort_by_key(|found| (found.weight, found.range.start));
        found
    }

    /// The lines to draw, in the colors the pane has: none until the server has said some.
    pub fn underlines(&self) -> Option<Underlines> {
        if let Some(built) = self.built.borrow().as_ref() {
            return Some(built.clone());
        }
        let colors = self.context.borrow().as_ref()?.colors;
        let loaded = self.loaded.borrow();
        let loaded = loaded.as_ref()?;
        let items = loaded
            .marks
            .iter()
            .filter(|mark| !mark.unused)
            .map(|mark| Underline::new(mark.range.clone(), colors.of(mark.weight)))
            .collect();
        let built = Underlines::new(items).of_text(loaded.text.clone());
        *self.built.borrow_mut() = Some(built.clone());
        Some(built)
    }

    /// Replaces the style of any unused diagnostic ranges intersecting `window` with a grayed-out `color`,
    /// keeping the cost proportional to the visible `window`.
    pub fn dim_unused(
        &self,
        base: Vec<StyledRange>,
        text: &TextSnapshot,
        window: Range<usize>,
        color: Color,
    ) -> Vec<StyledRange> {
        let loaded = self.loaded.borrow();
        let Some(loaded) = loaded.as_ref() else {
            return base;
        };
        if window.is_empty() || !loaded.marks.iter().any(|mark| mark.unused) {
            return base;
        }
        let edit = loaded.text.diff(text);
        let mut ranges: Vec<Range<usize>> = loaded
            .marks
            .iter()
            .filter(|mark| mark.unused)
            .filter_map(|mark| {
                let moved = moved(&mark.range, edit.as_ref())?;
                let start = moved.start.max(window.start);
                let end = moved.end.min(window.end);
                (start < end).then_some(start..end)
            })
            .collect();
        if ranges.is_empty() {
            return base;
        }
        // Coalesce overlapping or adjacent unused ranges so base runs are split cleanly once.
        ranges.sort_by_key(|r| (r.start, r.end));
        let mut merged_unused: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
        for next in ranges {
            match merged_unused.last_mut() {
                Some(last) if next.start <= last.end => last.end = last.end.max(next.end),
                _ => merged_unused.push(next),
            }
        }
        let dim = SpanStyle::new().color(color);
        let mut out: Vec<StyledRange> = Vec::with_capacity(base.len() + merged_unused.len() * 2);
        for run in base {
            let mut from = run.range.start;
            let first = merged_unused.partition_point(|r| r.end <= run.range.start);
            for unused in &merged_unused[first..] {
                if unused.start >= run.range.end {
                    break;
                }
                if from < unused.start {
                    out.push(StyledRange::new(from..unused.start, run.style.clone()));
                }
                from = from.max(unused.end);
            }
            if from < run.range.end {
                out.push(StyledRange::new(from..run.range.end, run.style.clone()));
            }
        }
        out.extend(
            merged_unused
                .into_iter()
                .map(|range| StyledRange::new(range, dim.clone())),
        );
        out.sort_by_key(|run| run.range.start);
        out
    }
}

/// The lines for `found`, which are of `text`: the least wrong first, so that the worse is drawn over it, and those that are as wrong
/// in the order of the text. A file has no more than [`ProblemLines::MOST`], and what is cut is the least wrong.
fn marks_of(found: &[Diagnostic], text: &TextSnapshot) -> Vec<Mark> {
    let marks: Vec<Mark> = found
        .iter()
        .filter_map(|diagnostic| {
            Some(Mark {
                range: under(&diagnostic.range, text)?,
                weight: diagnostic.weight(),
                message: diagnostic.message.clone(),
                code: diagnostic.code.clone(),
                unused: diagnostic.unused,
            })
        })
        .collect();
    ordered(marks)
}

/// `marks` in the order they are drawn, at most [`ProblemLines::MOST`] of them, the worst kept.
fn ordered(mut marks: Vec<Mark>) -> Vec<Mark> {
    // The worst first, to keep the worst; and then the other way, to draw them.
    marks.sort_by_key(|mark| (mark.weight, mark.range.start, mark.range.end));
    marks.truncate(ProblemLines::MOST);
    marks.sort_by_key(|mark| {
        (
            std::cmp::Reverse(mark.weight),
            mark.range.start,
            mark.range.end,
        )
    });
    marks
}

/// Where `range` of the text before an edit is in the text after it: the edit is the bytes that were replaced and the bytes that replaced
/// them. A range after it moves by what it added or took away, one before it stays, one that the edit is inside of is longer or shorter by
/// it, and one that it is only partly in is gone.
fn moved(
    range: &Range<usize>,
    edit: Option<&(Range<usize>, Range<usize>)>,
) -> Option<Range<usize>> {
    let Some((old, new)) = edit else {
        return Some(range.clone());
    };
    let shift = |at: usize| at + new.len() - old.len();
    if range.end <= old.start {
        Some(range.clone())
    } else if range.start >= old.end {
        Some(shift(range.start)..shift(range.end))
    } else if range.start <= old.start && old.end <= range.end {
        let end = shift(range.end);
        (range.start < end).then_some(range.start..end)
    } else {
        None
    }
}

/// The bytes a line is under for a problem at `range` of `text`: the range, cut to the text; and for a problem at a point, the
/// character there, or the one before it at the end of a line, since a line has to be under something. None where there is
/// nothing to be under.
fn under(range: &Range<usize>, text: &TextSnapshot) -> Option<Range<usize>> {
    let len = text.len();
    let (start, end) = (range.start.min(len), range.end.min(len));
    if start < end {
        return Some(start..end);
    }
    let is_break = |at: usize| matches!(text.byte_at(at), Some(b'\n' | b'\r'));
    // The character at the point, if it is on this line.
    if start < len && !is_break(start) {
        let end = (start + 1..=(start + 4).min(len)).find(|&at| text.is_char_boundary(at))?;
        return Some(start..end);
    }
    // Otherwise the one before it, if that is on this line too.
    let before = (start.saturating_sub(4)..start)
        .rev()
        .find(|&at| text.is_char_boundary(at))?;
    (before < start && !is_break(before)).then_some(before..start)
}

#[cfg(test)]
#[path = "../tests/ui/problem_lines.rs"]
mod tests;
