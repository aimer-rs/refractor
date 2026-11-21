use super::problem_section::{card_width, estimated_height as problems_height, problem_section};
use crate::components::popup::{Anchor, Popup};
use crate::hover::{Card, HoverCell};
use crate::markdown;
use crate::settings::ThemeMode;
use crate::theme::CodeTheme;
use aimer::input::mouse_region::MouseRegion;
use aimer::style::{LayoutSpacing, Theme};
use aimer::{
    BuildContext, Column, Container, Dimension, Positioned, SizedBox, Stack, State, StateUpdater,
    StateView, StatefulWidget, Widget, ZeroSizedBox,
};
use std::rc::Rc;

/// How wide the card is, and the padding inside it.
const WIDTH: f32 = 480.0;
const PADDING: u32 = 12;
/// How tall the card is at most. What is longer is scrolled in it, and the card is as tall as this.
const MAX_HEIGHT: f32 = 360.0;
/// How much over [`MAX_HEIGHT`] the estimate has to be for the card to be scrolled, so that a card that is about as tall as the most
/// it may be is not made a scrolled one for the sake of an estimate that is a little over.
const SCROLLS_OVER: f32 = 1.08;
/// What a line of prose and a line of code are worth in height, what a block of code adds round its lines, and what is between two
/// blocks: the height of a card is estimated from these, because it is not known until the card is laid out and where it goes
/// above the name has to be decided before.
const PROSE_LINE: f32 = 17.0;
const CODE_LINE: f32 = 16.0;
const CODE_BLOCK: f32 = 28.0;
const BLOCK_GAP: f32 = 6.0;
const CHROME: f32 = 2.0 * PADDING as f32 + 2.0;
/// How many characters of a line of prose fit in the card, for the lines that wrap.
const LINE_CHARS: usize = 62;

/// The card of documentation for the name the pointer rests on, over the editor beside the name.
///
/// It is drawn over the text area, which fills the place it is in, and it is redrawn when the cell says that the card changed.
#[derive(Clone, StatefulWidget)]
pub struct HoverCard {
    cell: Rc<HoverCell>,
    mode: ThemeMode,
}

impl HoverCard {
    pub fn new(cell: Rc<HoverCell>, mode: ThemeMode) -> Self {
        Self { cell, mode }
    }
}

struct HoverCardState {
    cell: Rc<HoverCell>,
    mode: ThemeMode,
    /// What makes a build differ from the one before, so the framework builds it again.
    changes: u32,
}

impl State<HoverCard> for HoverCardState {
    fn create(widget: HoverCard) -> Self {
        Self {
            cell: widget.cell,
            mode: widget.mode,
            changes: 0,
        }
    }

    fn on_mount(&mut self, updater: StateUpdater<Self>) {
        self.cell
            .on_change(Popup::redraw_watcher(updater, |state| state.changes += 1));
    }

    fn adopt_config_from(&mut self, new: Self) {
        self.cell = new.cell;
        self.mode = new.mode;
    }

    fn build(this: StateView<Self>, ctx: &BuildContext) -> impl Widget {
        let _changes = this.changes;
        let Some(card) = this.cell.with(|card| card.cloned()) else {
            return ZeroSizedBox.boxed();
        };
        let theme = *CodeTheme::of(ctx);
        let words = Card::merged(&card.markdown);
        // What is wrong is above the words, and the words are what is scrolled when there is more than the card may be.
        let has_words = !card.markdown.trim().is_empty() || card.problems.is_empty();
        let problems_at_top = problems_height(&card, this.cell.is_expanded());
        let words_height = if has_words { estimated_height(&words) } else { 0.0 };
        let estimate = problems_at_top + words_height;
        let scrolls = has_words && estimate > MAX_HEIGHT * SCROLLS_OVER;
        let height = if scrolls { MAX_HEIGHT } else { estimate };
        let anchor = Anchor {
            x: card.spot.x,
            y: card.spot.y,
            height: card.spot.height,
        };
        // A card that says only what is wrong is as wide as what it says; one that has words to read is wide enough to read them.
        let width = if card.problems.is_empty() || has_words { WIDTH } else { card_width(&card) };
        let (left, top) = Popup::place(anchor, (width, height), card.spot.extent);
        let (entered, left_it) = (Rc::clone(&this.cell), Rc::clone(&this.cell));
        let mut content = Container::new().width(width);
        content = if scrolls {
            // Longer than the card may be: as tall as that, and what is in it is scrolled.
            content.height(MAX_HEIGHT)
        } else {
            // As tall as what is in it, and not as tall as the room there is.
            content.fit_content()
        };
        let viewer = markdown::hover_viewer(&words, &markdown::hover_theme(this.mode), scrolls);
        // What scrolls is given the height it has to scroll in, which is the card's less what is above it, its padding and its frame:
        // given none, it is as tall as what is in it, has nothing to scroll, and is cut off by the card.
        let viewer = if scrolls {
            SizedBox::new()
                .height((MAX_HEIGHT - CHROME - problems_at_top).max(PROSE_LINE))
                .box_child(viewer)
        } else {
            viewer.boxed()
        };
        let content = if card.problems.is_empty() {
            content
                .padding(LayoutSpacing::all(PADDING))
                .decoration(Popup::frame(theme))
                .child(viewer)
        } else {
            // The problems are the top of the card, and have the padding of their own; the words, if there are any, are under them.
            let mut parts = vec![problem_section(&card, &this.cell, theme)];
            if has_words {
                parts.push(Container::new().padding(LayoutSpacing::all(PADDING)).child(viewer).boxed());
            }
            content.decoration(Popup::frame(theme)).child(Column::new().children(parts).boxed())
        };
        // The pointer on the card keeps it, and off it lets it go as it goes when the pointer leaves the name. The region holds the card,
        // which scrolls: a wheel over a region is its child's.
        let tracked = MouseRegion::new()
            .on_hover_enter(move || entered.pointer_over(true))
            .on_hover_exit(move || left_it.pointer_over(false))
            .child(content);
        // The layer fills what the card is over, so that the card is inside the bounds of what holds it: a wheel is routed by where it
        // is, and a card that hangs out of a small parent is not found by it, and the text under the card would be scrolled.
        Container::new()
            .child(
                Stack::new().add_child(
                    Positioned::new()
                        .left(Dimension::Px(left))
                        .top(Dimension::Px(top))
                        .child(
                            tracked,
                        ),
                ),
            )
            .boxed()
    }
}

/// About how tall the card is for `markdown`: its padding, a line for each line of prose and each time a long one wraps, a line for
/// each line of code and what a block of code adds round them, and a gap between the blocks.
fn estimated_height(markdown: &str) -> f32 {
    let mut height = CHROME;
    let mut in_code = false;
    let mut after_blank = true;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            if !in_code {
                height += CODE_BLOCK + if after_blank { 0.0 } else { BLOCK_GAP };
            }
            in_code = !in_code;
            after_blank = false;
        } else if in_code {
            height += CODE_LINE;
        } else if line.trim().is_empty() {
            after_blank = true;
        } else {
            height += PROSE_LINE * (1 + line.chars().count().saturating_sub(1) / LINE_CHARS) as f32
                + if after_blank { BLOCK_GAP } else { 0.0 };
            after_blank = false;
        }
    }
    height.max(CHROME + PROSE_LINE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_height_is_a_line_for_each_line_of_prose_and_for_each_wrap_of_a_long_one() {
        assert_eq!(estimated_height("one"), CHROME + PROSE_LINE + BLOCK_GAP);
        assert_eq!(
            estimated_height("one\ntwo\nthree"),
            CHROME + 3.0 * PROSE_LINE + BLOCK_GAP
        );
        assert_eq!(
            estimated_height(&"x".repeat(LINE_CHARS * 2 + 1)),
            CHROME + 3.0 * PROSE_LINE + BLOCK_GAP,
            "wrapped over three lines"
        );
        assert_eq!(estimated_height(""), CHROME + PROSE_LINE, "at least a line");
    }

    #[test]
    fn code_is_a_line_for_each_line_and_a_block_adds_round_them_and_paragraphs_are_apart() {
        let code = "```rust\npub fn f()\n\nfn g()\n```";
        assert_eq!(
            estimated_height(code),
            CHROME + CODE_BLOCK + 3.0 * CODE_LINE,
            "a blank line in code is a line of it"
        );
        let both = "```rust\nfn f()\n```\n\nFirst.\n\nSecond.";
        assert_eq!(
            estimated_height(both),
            CHROME + CODE_BLOCK + CODE_LINE + 2.0 * (PROSE_LINE + BLOCK_GAP)
        );
    }

    #[test]
    fn a_card_is_scrolled_only_when_it_is_clearly_taller_than_the_most_it_may_be() {
        let lines = |count: usize| {
            (0..count)
                .map(|n| format!("line {n}"))
                .collect::<Vec<_>>()
                .join("\n\n")
        };
        assert!(
            estimated_height(&lines(8)) <= MAX_HEIGHT * SCROLLS_OVER,
            "a few paragraphs fit"
        );
        assert!(
            estimated_height(&lines(40)) > MAX_HEIGHT * SCROLLS_OVER,
            "a page of them does not"
        );
    }
}
