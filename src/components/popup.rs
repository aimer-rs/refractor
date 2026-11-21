//! What the popups over the editor have in common: the menu that completes a word and the card of documentation for a name.
//!
//! Both hang from a place in the text, are kept inside the field, look as one family, and are drawn again when what they show
//! changes. That is said here once; what each shows, and when, is its own.

use std::cell::RefCell;
use std::rc::Rc;

use aimer::StateUpdater;
use aimer::style::{BorderSlice, BorderStyle, BoxBorder, BoxDecoration};

use crate::theme::CodeTheme;

/// Where a popup hangs from, in the coordinates of its field: the box of a caret or of a name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub x: f32,
    pub y: f32,
    pub height: f32,
}

/// The rules the popups share.
pub struct Popup;

impl Popup {
    /// The gap between what a popup hangs from and the popup.
    pub const GAP: f32 = 4.0;
    /// How opaque the pink is that marks the row that is chosen in a list: the same in each, so that they read as one family.
    pub const ROW_HIGHLIGHT: f32 = 0.16;

    /// Where a popup `width` by `height` goes, as how far from the left and the top of the field, for `anchor` in a field whose text
    /// is shown as far as `extent` (its right and bottom edges), or `None` for a field that is not laid out. It hangs below the
    /// anchor; where it does not fit there and there is more room above, it goes above; and where it would go past the right of the
    /// field it moves left, as far as the anchor is from the left.
    pub fn place(
        anchor: Anchor,
        (width, height): (f32, f32),
        extent: Option<(f32, f32)>,
    ) -> (f32, f32) {
        let below = anchor.y + anchor.height + Self::GAP;
        let Some((right, bottom)) = extent else {
            return (anchor.x, below);
        };
        let (room_below, room_above) = (bottom - below, anchor.y - Self::GAP);
        let top = if height > room_below && room_above > room_below {
            anchor.y - Self::GAP - height
        } else {
            below
        };
        (anchor.x.min(right - width).max(0.0), top)
    }

    /// The look of the frame of a popup: a dark fill and a line around it, so that it stands out of the code.
    pub fn frame(theme: CodeTheme) -> BoxDecoration {
        let edge = || {
            BorderSlice::new()
                .color(theme.divider)
                .style(BorderStyle::Solid)
                .stroke(1.0)
        };
        BoxDecoration::new()
            .background_color(theme.bar)
            .border_radius(8)
            .border(
                BoxBorder::new()
                    .top(edge())
                    .bottom(edge())
                    .left(edge())
                    .right(edge()),
            )
    }

    /// What a popup's widget is told when what it shows changed: it changes a field of its state, which makes the build differ from
    /// the one before so that the framework builds it again, and a frame is asked for, because a key that a menu takes never reaches
    /// the field, which is what asks for one when it edits.
    pub fn redraw_watcher<S: 'static>(
        updater: StateUpdater<S>,
        change: fn(&mut S),
    ) -> Rc<dyn Fn() -> bool> {
        Rc::new(move || {
            let alive = updater.try_set_state(change).is_some();
            if alive {
                aimer::events::window::request_animation_frame();
            }
            alive
        })
    }
}

/// What a model of a popup tells the widget that draws it, which sets it, when it changes.
#[derive(Default)]
pub struct Redraw {
    hook: RefCell<Option<Rc<dyn Fn()>>>,
    /// The hooks that say whether what they draw is still there, which are all called, and are dropped when it is not.
    watchers: RefCell<Vec<Rc<dyn Fn() -> bool>>>,
}

impl Redraw {
    /// Sets what is called to draw again, replacing the one before.
    pub fn set(&self, hook: Rc<dyn Fn()>) {
        *self.hook.borrow_mut() = Some(hook);
    }

    /// Adds what is called to draw again, which says whether what it draws is still there. Every one that is added is called,
    /// and one that says it is gone is not called again: a widget that is built again with the pane that holds it is made anew,
    /// and the one before, which may be the only one that is on screen, is not forgotten for the one that was made last.
    pub fn add(&self, hook: Rc<dyn Fn() -> bool>) {
        self.watchers.borrow_mut().push(hook);
    }

    /// Calls what there is.
    pub fn fire(&self) {
        // Cloned out of the cells first: what draws again may read the model, or set another hook.
        let hook = self.hook.borrow().clone();
        if let Some(hook) = hook {
            hook();
        }
        let watchers: Vec<_> = self.watchers.borrow().clone();
        if watchers.is_empty() {
            return;
        }
        let alive: Vec<bool> = watchers.iter().map(|watcher| watcher()).collect();
        let mut kept = alive.iter();
        self.watchers
            .borrow_mut()
            .retain(|_| kept.next().copied().unwrap_or(true));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const WIDTH: f32 = 500.0;
    const HEIGHT: f32 = 300.0;

    fn at(x: f32, y: f32) -> Anchor {
        Anchor { x, y, height: 20.0 }
    }

    #[test]
    fn a_popup_hangs_below_what_it_hangs_from_where_there_is_room_and_where_the_field_is_not_known()
    {
        let below = |x: f32, y: f32| (x, y + 20.0 + Popup::GAP);
        assert_eq!(
            Popup::place(at(50.0, 100.0), (WIDTH, HEIGHT), Some((1000.0, 800.0))),
            below(50.0, 100.0)
        );
        assert_eq!(
            Popup::place(at(50.0, 700.0), (WIDTH, HEIGHT), None),
            below(50.0, 700.0),
            "a field that has not said where it ends"
        );
    }

    #[test]
    fn a_popup_goes_above_where_it_does_not_fit_below_and_there_is_more_room_above() {
        let top = |anchor: Anchor, bottom: f32| {
            Popup::place(anchor, (WIDTH, HEIGHT), Some((1000.0, bottom))).1
        };
        assert_eq!(
            top(at(50.0, 700.0), 780.0),
            700.0 - Popup::GAP - HEIGHT,
            "its bottom is a gap above the anchor"
        );
        // Neither fits, and there is more room below than above: it stays below.
        assert_eq!(top(at(50.0, 40.0), 200.0), 40.0 + 20.0 + Popup::GAP);
        // Neither fits, and there is more room above: above.
        assert_eq!(top(at(50.0, 180.0), 200.0), 180.0 - Popup::GAP - HEIGHT);
        // It just fits below: it is where it was.
        let bottom = 100.0 + 20.0 + Popup::GAP + HEIGHT;
        assert_eq!(top(at(50.0, 100.0), bottom), 100.0 + 20.0 + Popup::GAP);
    }

    #[test]
    fn a_popup_moves_left_where_it_would_go_past_the_right_of_the_field_but_not_past_its_left() {
        let left = |x: f32, right: f32| {
            Popup::place(at(x, 100.0), (WIDTH, HEIGHT), Some((right, 800.0))).0
        };
        assert_eq!(left(300.0, 300.0 + WIDTH), 300.0, "it just fits");
        assert_eq!(left(300.0, 300.0 + WIDTH - 50.0), 250.0);
        assert_eq!(
            left(30.0, 100.0),
            0.0,
            "no further than the left of the field"
        );
        assert_eq!(left(0.0, 100.0), 0.0);
        assert_eq!(left(900.0, 1000.0), 1000.0 - WIDTH);
    }

    #[test]
    fn every_hook_that_was_added_is_called_and_one_that_says_it_is_gone_is_called_no_more() {
        let redraw = Redraw::default();
        let (first, second) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
        let (a, b) = (Rc::clone(&first), Rc::clone(&second));
        let second_alive = Rc::new(Cell::new(true));
        let still = Rc::clone(&second_alive);
        redraw.add(Rc::new(move || {
            a.set(a.get() + 1);
            true
        }));
        redraw.add(Rc::new(move || {
            b.set(b.get() + 1);
            still.get()
        }));
        redraw.fire();
        assert_eq!((first.get(), second.get()), (1, 1), "both are called, and not only the last that was added");
        second_alive.set(false);
        redraw.fire();
        redraw.fire();
        assert_eq!((first.get(), second.get()), (3, 2), "the one that is gone is called once more, to say so, and no more");
    }

    #[test]
    fn a_model_tells_what_draws_it_once_for_each_call_and_nothing_where_nothing_draws_it() {
        let redraw = Redraw::default();
        redraw.fire();
        let drawn = Rc::new(Cell::new(0));
        let counted = Rc::clone(&drawn);
        redraw.set(Rc::new(move || counted.set(counted.get() + 1)));
        redraw.fire();
        redraw.fire();
        assert_eq!(drawn.get(), 2);
        // What draws again may set the hook again, which is not held against it.
        let again = Rc::new(Redraw::default());
        let inner = Rc::clone(&again);
        again.set(Rc::new(move || inner.set(Rc::new(|| {}))));
        again.fire();
    }
}
