//! What is shown of the name the pointer rests on: the documentation the language server has of it, in a card beside the name.
//!
//! The text area says when the pointer has rested on a name and when it has gone ([`HoverSpot`]). The card is asked for then,
//! and shown when the server has answered; it goes when the pointer has left both the name and the card, after a moment that lets
//! the pointer travel from the one to the other, which is what lets what is in the card be scrolled and selected.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use aimer::Venus;
use aimer::input::input::HoverSpot;

use crate::ui::components::popup::Redraw;
use crate::services::lsp::{Hover, Weight};

mod source;

pub use self::source::CardSource;

/// A problem the server says there is at the place, as the card says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub weight: Weight,
    pub message: String,
    /// The server's name for the kind of problem, such as an error number.
    pub code: Option<String>,
}

/// Something that can be done about a problem, which the card offers.
#[derive(Clone)]
pub struct Fix {
    /// Tells one from another, so that a card that is made again with the same fixes is the same card.
    id: u64,
    pub title: String,
    /// Whether the server says it is the one to do.
    pub preferred: bool,
    run: Rc<dyn Fn() -> bool>,
}

impl Fix {
    /// A fix called `title` that does `run`, which says whether it did anything.
    pub fn new(
        title: impl Into<String>,
        preferred: bool,
        run: impl Fn() -> bool + 'static,
    ) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            title: title.into(),
            preferred,
            run: Rc::new(run),
        }
    }

    /// Does it. False where it could not be done: the text is not the one it was made for.
    pub fn run(&self) -> bool {
        (self.run)()
    }
}

impl PartialEq for Fix {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl std::fmt::Debug for Fix {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Fix")
            .field("title", &self.title)
            .field("preferred", &self.preferred)
            .finish()
    }
}

/// What is shown beside a place: the problems that are there and what can be done about them, and the words the server has of the name,
/// as markdown. Any of them may be missing.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub spot: HoverSpot,
    pub markdown: String,
    pub problems: Vec<Problem>,
    /// The preferred fix first.
    pub fixes: Vec<Fix>,
}

/// A fenced block of code in markdown, as far as the card needs to know it.
struct Fence<'a> {
    /// The line that opens it, as it was, and the mark it is made of: `` ` `` or `~`, and how many.
    opening: &'a str,
    mark: char,
    length: usize,
    language: &'a str,
    body: Vec<&'a str>,
    closed: bool,
}

impl Card {
    /// A card of the words of a name and nothing else.
    pub fn docs(spot: HoverSpot, markdown: String) -> Self {
        Self {
            spot,
            markdown,
            problems: Vec::new(),
            fixes: Vec::new(),
        }
    }

    /// `markdown` as the card shows it: the code blocks that follow each other are one block. A server says what a name is in
    /// pieces, where it is and then what it is, each in its own fence, and drawn so they are two panels of one thing. Only blocks
    /// of one language, with nothing between them but blank lines, are joined, with a blank line where they meet.
    pub fn merged(markdown: &str) -> String {
        enum Piece<'a> {
            Words(Vec<&'a str>),
            Code(Fence<'a>),
        }
        let mut pieces: Vec<Piece> = Vec::new();
        let mut lines = markdown.lines();
        while let Some(line) = lines.next() {
            match Self::opening_of(line) {
                Some((mark, length, language)) => {
                    let mut fence = Fence {
                        opening: line,
                        mark,
                        length,
                        language,
                        body: Vec::new(),
                        closed: false,
                    };
                    for next in lines.by_ref() {
                        if Self::closes(next, mark, length) {
                            fence.closed = true;
                            break;
                        }
                        fence.body.push(next);
                    }
                    pieces.push(Piece::Code(fence));
                }
                None => match pieces.last_mut() {
                    Some(Piece::Words(words)) => words.push(line),
                    _ => pieces.push(Piece::Words(vec![line])),
                },
            }
        }
        // Blank lines between two blocks of code are not words that keep them apart: they are held back, and go with the join.
        let blank = |words: &Vec<&str>| words.iter().all(|line| line.trim().is_empty());
        let mut joined: Vec<Piece> = Vec::new();
        let mut held: Option<Vec<&str>> = None;
        for piece in pieces {
            match piece {
                Piece::Words(words)
                    if held.is_none()
                        && blank(&words)
                        && matches!(joined.last(), Some(Piece::Code(_))) =>
                {
                    held = Some(words)
                }
                Piece::Words(words) => {
                    joined.extend(held.take().map(Piece::Words));
                    joined.push(Piece::Words(words));
                }
                Piece::Code(next) => match joined.last_mut() {
                    Some(Piece::Code(last))
                        if last.closed
                            && next.closed
                            && (last.language == next.language
                                || last.language.is_empty()
                                || next.language.is_empty()) =>
                    {
                        if last.language.is_empty() {
                            last.opening = next.opening;
                            last.language = next.language;
                        }
                        last.body.push("");
                        last.body.extend(next.body);
                        held = None;
                    }
                    _ => {
                        joined.extend(held.take().map(Piece::Words));
                        joined.push(Piece::Code(next));
                    }
                },
            }
        }
        joined.extend(held.map(Piece::Words));
        let mut out: Vec<String> = Vec::new();
        for piece in &joined {
            match piece {
                Piece::Words(words) => out.extend(words.iter().map(|line| (*line).to_owned())),
                Piece::Code(fence) => {
                    out.push(fence.opening.to_owned());
                    out.extend(fence.body.iter().map(|line| (*line).to_owned()));
                    if fence.closed {
                        out.push(fence.mark.to_string().repeat(fence.length));
                    }
                }
            }
        }
        out.join("\n")
    }

    /// What opens a fence on `line`: the mark it is made of, how many, and the language it says. Up to three spaces may come before it.
    fn opening_of(line: &str) -> Option<(char, usize, &str)> {
        let trimmed = line
            .strip_prefix("   ")
            .or_else(|| line.strip_prefix("  "))
            .or_else(|| line.strip_prefix(' '))
            .unwrap_or(line);
        let mark = trimmed
            .chars()
            .next()
            .filter(|mark| matches!(mark, '`' | '~'))?;
        let length = trimmed
            .chars()
            .take_while(|found| *found == mark)
            .count();
        (length >= 3).then(|| {
            (
                mark,
                length,
                trimmed[length..]
                    .split_whitespace()
                    .next()
                    .unwrap_or(""),
            )
        })
    }

    /// Whether `line` closes a fence of `length` of `mark`: at least as many of the mark and nothing else.
    fn closes(line: &str, mark: char, length: usize) -> bool {
        let trimmed = line.trim();
        trimmed.chars().count() >= length && trimmed.chars().all(|found| found == mark)
    }
}

/// The card that is shown, shared between what decides when there is one and the widget that draws it.
#[derive(Default)]
pub struct HoverCell {
    card: RefCell<Option<Card>>,
    redraw: Redraw,
    /// Whether the pointer is on the card.
    over: Cell<bool>,
    /// Counts the times the card was asked to go and the times that was put off, so that a wait that was overtaken does nothing.
    generation: Cell<u64>,
    /// Whether the card lists every fix and not only the first.
    expanded: Cell<bool>,
}

impl HoverCell {
    /// How long the card stays after the pointer has left the name, for the pointer to get to the card.
    pub const GRACE: Duration = Duration::from_millis(350);

    /// Reads the card, which is `None` when none is shown.
    pub fn with<T>(&self, read: impl FnOnce(Option<&Card>) -> T) -> T {
        read(self.card.borrow().as_ref())
    }

    /// Whether a card is shown.
    pub fn is_open(&self) -> bool {
        self.card.borrow().is_some()
    }

    /// Adds what is called to draw the card again, which says whether what it draws is still there. The pane is built again, and a
    /// widget of the card is made with each build: the one that was made last is not always the one that is kept, so every one is
    /// told, and the ones that are gone are forgotten.
    pub fn on_change(&self, redraw: Rc<dyn Fn() -> bool>) {
        self.redraw.add(redraw);
    }

    /// Whether the card lists every fix and not only the first.
    pub fn is_expanded(&self) -> bool {
        self.expanded.get()
    }

    /// Lists every fix, or the first alone again.
    pub fn toggle_expanded(&self) {
        self.expanded.set(!self.expanded.get());
        if self.is_open() {
            self.notify();
        }
    }

    /// Whether the card has more than one fix, which is when there is a list to open.
    pub fn has_more_fixes(&self) -> bool {
        self.card
            .borrow()
            .as_ref()
            .is_some_and(|card| card.fixes.len() > 1)
    }

    /// The fix to do when the key for it is pressed: the first of the card's, if it has any. None where no card is shown.
    pub fn first_fix(&self) -> Option<Fix> {
        self.card
            .borrow()
            .as_ref()
            .and_then(|card| card.fixes.first().cloned())
    }

    /// Shows `card`, in place of the one that is shown, and puts off what was about to take that one away.
    pub fn show(&self, card: Card) {
        self.generation.set(self.generation.get() + 1);
        if self.card.borrow().as_ref() == Some(&card) {
            return;
        }
        // Another place is not the list that was opened at the one before.
        if self
            .card
            .borrow()
            .as_ref()
            .is_none_or(|shown| shown.spot.range != card.spot.range)
        {
            self.expanded.set(false);
        }
        *self.card.borrow_mut() = Some(card);
        self.notify();
    }

    /// Takes the card away at once.
    pub fn hide(&self) {
        self.generation.set(self.generation.get() + 1);
        self.over.set(false);
        self.expanded.set(false);
        if self.card.borrow_mut().take().is_some() {
            self.notify();
        }
    }

    /// Keeps the card for now, as the pointer has come to another name that it is to be shown for.
    pub fn hold(&self) {
        self.generation.set(self.generation.get() + 1);
    }

    /// The pointer is not on the name now: the card goes after [`GRACE`](Self::GRACE), unless the pointer is on it by then, or
    /// the card was asked to stay or go again in the meantime.
    pub fn leave(self: &Rc<Self>) {
        if !self.is_open() {
            return;
        }
        self.generation.set(self.generation.get() + 1);
        if self.over.get() {
            return;
        }
        let asked = self.generation.get();
        let Some(venus) = Venus::current() else {
            self.hide();
            return;
        };
        let waited = venus.sleep(Self::GRACE);
        let cell = Rc::clone(self);
        venus.spawn(async move {
            // A runtime that has no timers answers at once, and the card is not kept at all.
            let _ = waited.await;
            if cell.generation.get() == asked && !cell.over.get() {
                cell.hide();
            }
        });
    }

    /// The pointer has come onto the card, or left it. On it, the card stays; leaving it, the card goes as it does when the
    /// pointer leaves the name.
    pub fn pointer_over(self: &Rc<Self>, over: bool) {
        self.over.set(over);
        self.generation.set(self.generation.get() + 1);
        if !over {
            self.leave();
        }
    }

    fn notify(&self) {
        self.redraw.fire();
    }
}

/// What the text area is told, made into what is done about it: asks `ask` for the words of the name at the byte it is given, and
/// shows them when they come; and when the pointer goes, drops the question with `cancel` and lets the card go.
///
/// `ask` calls what it is given with the answer, and not at all if it is asked again before it has answered.
pub fn follow(
    cell: Rc<HoverCell>,
    ask: impl Fn(usize, Box<dyn FnOnce(Option<Hover>)>) + 'static,
    cancel: impl Fn() + 'static,
) -> impl Fn(Option<HoverSpot>) + 'static {
    follow_with(
        cell,
        move |spot, publish| {
            let start = spot.range.start;
            ask(
                start,
                Box::new(move |said| publish(said.map(|hover| Card::docs(spot, hover.markdown)))),
            );
        },
        cancel,
    )
}

/// [`follow`] for a card that is made of more than the words: `source` is given the place and what to call with the card, which it calls
/// with the whole of it each time something more of it is known, and with none when there is nothing to show.
pub fn follow_with(
    cell: Rc<HoverCell>,
    source: impl Fn(HoverSpot, Rc<dyn Fn(Option<Card>)>) + 'static,
    cancel: impl Fn() + 'static,
) -> impl Fn(Option<HoverSpot>) + 'static {
    move |spot| match spot {
        Some(spot) => {
            // The card that is shown stays until the one for this place comes, or until it is known that there is none.
            cell.hold();
            let shown = Rc::clone(&cell);
            source(
                spot,
                Rc::new(move |card| match card {
                    Some(card) => shown.show(card),
                    None => shown.hide(),
                }),
            );
        }
        None => {
            cancel();
            cell.leave();
        }
    }
}

#[cfg(test)]
#[path = "../tests/ui/hover.rs"]
mod tests;
