//! What a card is made of for a place: the problems there, what can be done about them, and the words of the name, each as it is known.
//!
//! A problem is known at once, since it is what the server said before, and is shown at once; the fixes and the words are asked of the
//! server and put in the card as they come, each time the whole of it is said again, so that the card does not wait for the slowest.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use aimer::input::TextEditingController;
use aimer::input::input::HoverSpot;

use super::{Card, Fix, Problem};
use crate::lsp::Lsp;
use crate::problem_lines::ProblemLines;

/// The card for a place, asked for when the pointer rests on it.
pub struct CardSource {
    lines: Rc<ProblemLines>,
    lsp: Rc<dyn Fn() -> Lsp>,
    path: PathBuf,
    controller: TextEditingController,
    /// The request for the fixes that is waiting, so that it can be dropped.
    fixes: Rc<Cell<Option<i64>>>,
}

/// What is known of the card so far.
#[derive(Default)]
struct Parts {
    markdown: String,
    fixes: Vec<Fix>,
}

impl CardSource {
    pub fn new(
        lines: Rc<ProblemLines>,
        lsp: Rc<dyn Fn() -> Lsp>,
        path: PathBuf,
        controller: TextEditingController,
    ) -> Self {
        Self {
            lines,
            lsp,
            path,
            controller,
            fixes: Rc::default(),
        }
    }

    /// Asks for the card for `spot`, and calls `publish` with it each time more of it is known: with none where there is nothing at the
    /// place, which is a place that has no problem and that the server has no words for.
    pub fn ask(&self, spot: HoverSpot, publish: Rc<dyn Fn(Option<Card>)>) {
        let lsp = (self.lsp)();
        let start = spot.range.start;
        let found = self.lines.at(start);
        if found.is_empty() {
            lsp.hover(&self.path, start, move |said| {
                publish(said.map(|hover| Card::docs(spot, hover.markdown)))
            });
            return;
        }
        let problems: Vec<Problem> = found
            .iter()
            .map(|found| Problem {
                weight: found.weight,
                message: found.message.clone(),
                code: found.code.clone(),
            })
            .collect();
        let parts: Rc<RefCell<Parts>> = Rc::default();
        let emit: Rc<dyn Fn()> = {
            let (parts, spot, publish) = (Rc::clone(&parts), spot.clone(), Rc::clone(&publish));
            Rc::new(move || {
                let parts = parts.borrow();
                publish(Some(Card {
                    spot: spot.clone(),
                    markdown: parts.markdown.clone(),
                    problems: problems.clone(),
                    fixes: parts.fixes.clone(),
                }));
            })
        };
        emit();
        let (words, words_emit) = (Rc::clone(&parts), Rc::clone(&emit));
        lsp.hover(&self.path, start, move |said| {
            if let Some(hover) = said {
                words.borrow_mut().markdown = hover.markdown;
                words_emit();
            }
        });
        let (offered, offered_emit, controller, waiting) = (
            Rc::clone(&parts),
            Rc::clone(&emit),
            self.controller.clone(),
            Rc::clone(&self.fixes),
        );
        let id = lsp.code_actions(&self.path, found[0].range.clone(), move |fixes| {
            waiting.set(None);
            let Some(mut fixes) = fixes else {
                return;
            };
            // The one the server prefers first, and the others as it said them.
            fixes.sort_by_key(|fix| !fix.preferred);
            offered.borrow_mut().fixes = fixes
                .into_iter()
                .map(|fix| {
                    let controller = controller.clone();
                    Fix::new(fix.title.clone(), fix.preferred, move || {
                        fix.apply(&controller)
                    })
                })
                .collect();
            offered_emit();
        });
        self.fixes.set(id);
    }

    /// Drops the questions that are waiting: the pointer has gone from the place.
    pub fn cancel(&self) {
        let lsp = (self.lsp)();
        lsp.cancel_hover();
        if let Some(id) = self.fixes.take() {
            lsp.cancel(id);
        }
    }
}

#[cfg(test)]
mod tests;
