//! The counts beside definitions: "27 usages | 1 implementation".
//!
//! The server says, for each type, trait and function of a file, how many places use it and how many implement it, as lenses.
//! A file's lenses are asked for as a list and then each is resolved, a few at a time so that the server is not asked for
//! every count at once, and what comes of it is the byte of each name and the places that use it and implement it.

use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::rc::Rc;

use aimer::input::input::TextSnapshot;

use super::client::Lsp;
use super::protocol::{self, LensKind, Place, Unresolved};

/// What is known of one name that has a lens: where it is and who uses and implements it.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Lens {
    /// The byte where the name starts, in the text the lenses were asked for.
    pub at: usize,
    /// The places that use it, if the server counted them.
    pub usages: Option<Vec<Place>>,
    /// The places that implement it, if the server counted them.
    pub implementations: Option<Vec<Place>>,
}

impl Lens {
    /// What the count of usages says: "27 usages", "1 usage" or "no usages". None where they were not counted.
    pub fn usages_caption(&self) -> Option<String> {
        self.usages
            .as_ref()
            .map(|places| match places.len() {
                0 => "no usages".to_owned(),
                1 => "1 usage".to_owned(),
                count => format!("{count} usages"),
            })
    }

    /// What the count of implementations says: "1 implementation" or "3 implementations". None where there were none, or they
    /// were not counted: a name that nothing implements has nothing to say of it.
    pub fn implementations_caption(&self) -> Option<String> {
        match self.implementations.as_ref()?.len() {
            0 => None,
            1 => Some("1 implementation".to_owned()),
            count => Some(format!("{count} implementations")),
        }
    }
}

/// Joins what was counted for the same name, in the order of the text.
fn group(mut found: Vec<(usize, LensKind, Vec<Place>)>) -> Vec<Lens> {
    found.sort_by_key(|(at, kind, _)| (*at, *kind == LensKind::Implementations));
    let mut lenses: Vec<Lens> = Vec::new();
    for (at, kind, places) in found {
        if lenses.last().is_none_or(|last| last.at != at) {
            lenses.push(Lens {
                at,
                ..Lens::default()
            });
        }
        let last = lenses.last_mut().expect("one was just made");
        match kind {
            LensKind::Usages => last.usages = Some(places),
            LensKind::Implementations => last.implementations = Some(places),
        }
    }
    lenses
}

/// How many lenses of a file are resolved at once.
const IN_FLIGHT: usize = 4;
/// The most lenses of a file that are resolved: a file with more has the first of them.
const MOST: usize = 400;

/// A load of the lenses of a file that is going on. It can be [cancelled](Self::cancel).
pub struct LensLoad {
    state: Rc<RefCell<Loading>>,
}

struct Loading {
    lsp: Lsp,
    text: TextSnapshot,
    queue: VecDeque<Unresolved>,
    found: Vec<(usize, LensKind, Vec<Place>)>,
    /// The request for the list of lenses, until it is answered.
    listing: Option<i64>,
    /// The requests for a lens that are waiting.
    waiting: HashSet<i64>,
    /// Whether the list has come.
    listed: bool,
    cancelled: bool,
    done: Option<Box<dyn FnOnce(Option<Vec<Lens>>)>>,
}

impl LensLoad {
    /// Says that what is loading is not wanted: nothing is called, and the server is told to stop.
    pub fn cancel(&self) {
        let (lsp, ids) = {
            let mut state = self.state.borrow_mut();
            state.cancelled = true;
            state.done = None;
            state.queue.clear();
            let mut ids: Vec<i64> = state.waiting.drain().collect();
            ids.extend(state.listing.take());
            (state.lsp.clone(), ids)
        };
        for id in ids {
            lsp.cancel(id);
        }
    }
}

impl Lsp {
    /// Asks the server for the lenses of the open file `path` and calls `done` with them, in the order of the text, when
    /// they are all in, or with none when the file is not open or the server could not say. `done` is not called from inside
    /// this unless it is with none, and not at all if the load is cancelled.
    pub fn lenses(&self, path: &Path, done: impl FnOnce(Option<Vec<Lens>>) + 'static) -> LensLoad {
        let Some(text) = self.snapshot(path) else {
            done(None);
            return LensLoad {
                state: Rc::new(RefCell::new(Loading::over(self.clone()))),
            };
        };
        let state = Rc::new(RefCell::new(Loading {
            lsp: self.clone(),
            text,
            queue: VecDeque::new(),
            found: Vec::new(),
            listing: None,
            waiting: HashSet::new(),
            listed: false,
            cancelled: false,
            done: Some(Box::new(done)),
        }));
        let listing = Rc::clone(&state);
        let id = self.ask(
            protocol::CODE_LENS,
            protocol::lens_params(path),
            move |reply| Loading::listed(&listing, reply),
        );
        // Not if the answer came already, which it does where the server could not be asked.
        if let Some(id) = id
            && !state.borrow().listed
        {
            state.borrow_mut().listing = Some(id);
        }
        LensLoad { state }
    }
}

impl Loading {
    /// A load that has nothing to do.
    fn over(lsp: Lsp) -> Self {
        Self {
            lsp,
            text: TextSnapshot::from_text(""),
            queue: VecDeque::new(),
            found: Vec::new(),
            listing: None,
            waiting: HashSet::new(),
            listed: true,
            cancelled: true,
            done: None,
        }
    }

    /// The list of lenses came, or could not.
    fn listed(this: &Rc<RefCell<Self>>, reply: Option<serde_json::Value>) {
        {
            let mut state = this.borrow_mut();
            if state.cancelled {
                return;
            }
            state.listed = true;
            state.listing = None;
            match reply {
                Some(reply) => {
                    state.queue = protocol::unresolved_lenses(&reply)
                        .into_iter()
                        .take(MOST)
                        .collect();
                }
                None => {
                    // The server could not say: what there was is kept, and nothing is given.
                    let done = state.done.take();
                    drop(state);
                    if let Some(done) = done {
                        done(None);
                    }
                    return;
                }
            }
        }
        Self::pump(this);
    }

    /// Asks for the next lenses, as many as may be asked for at once, and gives the result when there are none left.
    fn pump(this: &Rc<RefCell<Self>>) {
        loop {
            let next = {
                let mut state = this.borrow_mut();
                if state.cancelled || state.waiting.len() >= IN_FLIGHT {
                    None
                } else {
                    state.queue.pop_front()
                }
            };
            let Some(lens) = next else {
                break;
            };
            let lsp = this.borrow().lsp.clone();
            let (resolving, reserved) = (Rc::clone(this), Rc::new(std::cell::Cell::new(None)));
            let seen = Rc::clone(&reserved);
            let id = lsp.ask(protocol::CODE_LENS_RESOLVE, lens.raw, move |reply| {
                if let Some(id) = seen.get() {
                    resolving.borrow_mut().waiting.remove(&id);
                }
                Self::resolved(&resolving, reply);
            });
            if let Some(id) = id {
                reserved.set(Some(id));
                this.borrow_mut().waiting.insert(id);
            }
        }
        let finished = {
            let state = this.borrow();
            !state.cancelled && state.listed && state.queue.is_empty() && state.waiting.is_empty()
        };
        if finished {
            let (done, found) = {
                let mut state = this.borrow_mut();
                (state.done.take(), std::mem::take(&mut state.found))
            };
            if let Some(done) = done {
                done(Some(group(found)));
            }
        }
    }

    /// A lens was resolved, or could not be.
    fn resolved(this: &Rc<RefCell<Self>>, reply: Option<serde_json::Value>) {
        {
            let mut state = this.borrow_mut();
            if state.cancelled {
                return;
            }
            if let Some(resolved) = reply.as_ref().and_then(protocol::resolved_lens) {
                let at = state.lsp.byte_of(&state.text, resolved.at);
                state
                    .found
                    .push((at, resolved.kind, resolved.places));
            }
        }
        Self::pump(this);
    }
}

#[cfg(test)]
#[path = "../../tests/services/lsp/lens.rs"]
mod tests;
