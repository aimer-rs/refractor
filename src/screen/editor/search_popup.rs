use super::search_go::{self, Later};
use super::search_preview::{self, SearchPreview};
use super::search_row::{ROW_HEIGHT, SearchRow};
use super::search_tabs::{self, SearchTabs};
use crate::components::icon::icon;
use crate::components::popup::Popup;
use crate::components::rule;
use crate::fonts;
use crate::search::{Hit, Search, Tab};
use crate::theme::CodeTheme;
use crate::workspace::{Workspace, notifier, refresh_status};
use aimer::FocusNode;
use aimer::events::element::NamedKey;
use aimer::input::TextEditingController;
use aimer::input::button::Button;
use aimer::input::gesture::DragUpdateData;
use aimer::input::gesture::gesture_detector::GestureDetector;
use aimer::input::input::TextField;
use aimer::style::{LayoutSpacing, TextOverflow, Theme};
use aimer::{
    AnyWidget, BoxAlignment, BuildContext, Color, Column, Container, Dimension, Expanded, Icons,
    ProviderHandle, Row, ScrollController, Scrollable, SizedBox, Stack, State, StateUpdater,
    StateView, StatefulWidget, Text, Vec2d, Widget, ZeroSizedBox,
};
use std::rc::Rc;

/// How wide the popup is, and how far from the top of the window.
const WIDTH: f32 = 560.0;
/// How far the popup may be dragged from the middle of the window, in logical pixels, to each side: far enough to reach the edges
/// of a window of a laptop, and not so far that it is lost.
const DRAG_X: f32 = 640.0;
const DRAG_Y: f32 = 360.0;
/// How tall the list is at most, and how tall it is while the preview is under it, in rows; and the space inside it round the rows.
const ROWS: usize = 9;
const ROWS_WITH_PREVIEW: usize = 5;
const LIST_PAD: u32 = 3;
/// How tall the field of what is typed is, the strip of status under the list, and the line round the popup.
const FIELD_HEIGHT: u32 = 36;
const FOOTER_HEIGHT: u32 = 24;
/// The room over the text of the field that centres it in the row: the row less a line of text, halved.
const FIELD_TEXT_TOP: u32 = (FIELD_HEIGHT - 18) / 2;
const UI_SIZE: u32 = 13;

/// The popup's place after a drag: the offset from the middle of the window, moved by `delta` and kept within what may be reached.
fn dragged(offset: (f32, f32), delta: (f32, f32)) -> (f32, f32) {
    (
        (offset.0 + delta.0).clamp(-DRAG_X, DRAG_X),
        (offset.1 + delta.1).clamp(-DRAG_Y, DRAG_Y),
    )
}

/// How much of the room above and below the popup is above it, when it has not been dragged: a quarter, which is a little higher
/// than the middle, where the eye looks for a search. The room is shared out by the flex of the two spaces, `ABOVE` to the rest.
const ABOVE: f32 = 0.25;

/// How tall the popup is, in logical pixels: its tabs, field, list, preview when it is shown, and footer, the lines between them
/// and the line round it.
fn card_height(previewing: bool) -> f32 {
    let lines = if previewing { 4.0 } else { 3.0 };
    let preview = if previewing {
        search_preview::HEIGHT as f32
    } else {
        0.0
    };
    search_tabs::HEIGHT as f32
        + FIELD_HEIGHT as f32
        + list_height(previewing)
        + preview
        + FOOTER_HEIGHT as f32
        + lines
        + 2.0
}

/// How tall a place is kept for the popup: the most it is, which is with the preview. Where the top of the popup is follows from
/// this, and not from how tall it is at the time.
fn anchor_height() -> f32 {
    card_height(true).max(card_height(false))
}

/// The padding that moves the popup by `offset` from where it rests: across, it is centred, and down, it has `ABOVE` of the room
/// over it. What is put on one side of the card takes room from both spaces round it, so the padding is worked out to leave the
/// card exactly `offset` from where it would be: twice the distance across; and down, the distance over what is left of the
/// room that is not above, or, going up, the distance over `ABOVE`.
fn centering_padding(offset: (f32, f32)) -> LayoutSpacing {
    let side = |distance: f32| distance.round() as u32;
    let across = (offset.0.abs() * 2.0).round() as u32;
    let mut padding = LayoutSpacing::new();
    padding = if offset.0 >= 0.0 {
        padding.left(across)
    } else {
        padding.right(across)
    };
    if offset.1 >= 0.0 {
        padding.top(side(offset.1 / (1.0 - ABOVE)))
    } else {
        padding.bottom(side(-offset.1 / ABOVE))
    }
}

/// How tall the list is: always as tall as it may be, whatever it has in it. The popup is centred, so a list that grew with what is
/// typed would move the tabs and the field under the eyes of the one who types; this way only the preview changes its size, and
/// that is what is asked for.
fn list_height(previewing: bool) -> f32 {
    let rows = if previewing { ROWS_WITH_PREVIEW } else { ROWS };
    (rows as u32 * ROW_HEIGHT + 2 * LIST_PAD) as f32
}

/// Where the list is to be scrolled to, from `offset`, for the row `index` to be in the `viewport` that shows it: none when it is
/// in view already, so that a key that moves the choice inside what is shown does not move the list under it.
fn reveal_offset(index: usize, offset: f32, viewport: f32) -> Option<f32> {
    let row = ROW_HEIGHT as f32;
    let pad = LIST_PAD as f32;
    let top = index as f32 * row;
    let bottom = pad + top + row + pad;
    if top < offset {
        Some(top)
    } else if bottom > offset + viewport {
        Some(bottom - viewport)
    } else {
        None
    }
}

/// The search, over the window: the tabs, the field the query is typed in, the hits and, while the preview key is held, the lines
/// of the file under the one that is chosen. The keys of the list are taken from the field: the arrows choose, Return goes, Escape
/// shuts it, and Tab or Alt with the arrows to the sides shows another tab.
///
/// It is made when the search is opened and gone when it is shut, which is what stops what the search is doing.
#[derive(Clone, StatefulWidget)]
pub struct SearchPopup {
    search: Rc<Search>,
    /// Shuts the popup.
    close: Rc<dyn Fn()>,
}

impl SearchPopup {
    pub fn new(search: Rc<Search>, close: Rc<dyn Fn()>) -> Self {
        Self { search, close }
    }
}

struct SearchPopupState {
    search: Rc<Search>,
    close: Rc<dyn Fn()>,
    controller: TextEditingController,
    scroll: ScrollController,
    /// How far the popup was dragged from the middle of the window, to the right and down.
    offset: (f32, f32),
    /// The focus of the field: asked for when the popup opens, because an editor that has the focus does not give it up to a field
    /// that only asks to be focused when it is made.
    focus: FocusNode,
    /// What makes a build differ from the one before, so the framework builds it again.
    changes: u32,
}

impl State<SearchPopup> for SearchPopupState {
    fn create(widget: SearchPopup) -> Self {
        Self {
            search: widget.search,
            close: widget.close,
            controller: TextEditingController::new(),
            scroll: ScrollController::new(),
            offset: (0.0, 0.0),
            focus: FocusNode::new(),
            changes: 0,
        }
    }

    fn on_mount(&mut self, updater: StateUpdater<Self>) {
        self.focus.request_focus();
        // Tab and Shift+Tab are the popup's, and not the focus traversal's, which would take the focus out of the field and into the
        // editor. The way out is Escape. A field says by itself what it claims of Tab each time it is made, so what is held here
        // is what that does not undo.
        self.focus.set_holds_tab(true);
        self.search
            .redraw()
            .add(Popup::redraw_watcher(updater, |state| state.changes += 1));
    }

    fn adopt_config_from(&mut self, new: Self) {
        self.close = new.close;
    }

    fn build(this: StateView<Self>, ctx: &BuildContext) -> impl Widget {
        let _changes = this.changes;
        let theme = *CodeTheme::of(ctx);
        let store = ProviderHandle::<Workspace>::of(ctx);
        let search = Rc::clone(&this.search);
        let (hits, selected, tab, previewing) = (
            search.hits(),
            search.selected(),
            search.tab(),
            search.shift(),
        );
        let height = list_height(previewing);

        // The tabs and the field.
        let pick: Rc<dyn Fn(Tab)> = {
            let search = Rc::clone(&search);
            Rc::new(move |tab| search.set_tab(tab))
        };
        let on_key = {
            let (search, close, store, scroll) = (
                Rc::clone(&search),
                Rc::clone(&this.close),
                store.clone(),
                this.scroll.clone(),
            );
            move |key: &NamedKey, modifiers: &aimer::events::element::Modifiers| {
                let step = |delta: isize| {
                    search.move_selection(delta);
                    let height = list_height(search.shift());
                    if let Some(offset) =
                        reveal_offset(search.selected(), scroll.offset().y, height)
                    {
                        scroll.jump_to(Vec2d { x: 0.0, y: offset });
                    }
                    true
                };
                match key {
                    NamedKey::ArrowDown => step(1),
                    NamedKey::ArrowUp => step(-1),
                    NamedKey::PageDown => step(ROWS as isize),
                    NamedKey::PageUp => step(-(ROWS as isize)),
                    NamedKey::Enter => {
                        if let Some(hit) = search.current() {
                            go_to(hit, &store, &*close);
                        }
                        true
                    }
                    NamedKey::Escape => {
                        close();
                        true
                    }
                    // Tab shows the next tab and Shift+Tab the one before; Alt with the arrows to the sides does the same.
                    NamedKey::Tab => {
                        search.set_tab(
                            search
                                .tab()
                                .step(if modifiers.shift { -1 } else { 1 }),
                        );
                        this_scroll_to_top(&scroll);
                        true
                    }
                    NamedKey::ArrowRight | NamedKey::ArrowLeft if modifiers.alt => {
                        search.set_tab(
                            search
                                .tab()
                                .step(if *key == NamedKey::ArrowRight { 1 } else { -1 }),
                        );
                        this_scroll_to_top(&scroll);
                        true
                    }
                    _ => false,
                }
            }
        };
        let field = {
            let search = Rc::clone(&search);
            let scroll = this.scroll.clone();
            TextField::new()
                .controller(this.controller.clone())
                .focus_node(this.focus.clone())
                .auto_focus(true)
                .hint("Search Anything...")
                .hint_style(fonts::ui().font_size(14).color(theme.muted))
                .text_style(fonts::ui().font_size(14).color(theme.text))
                .decoration(Color::Transparent)
                // The field is as tall as the row and draws its text at the top, so the room above the text is what puts it on the line of
                // the icon beside it: half of what the row has left over a line of text of this size.
                .padding(LayoutSpacing::new().top(FIELD_TEXT_TOP))
                .on_changed(move |text: String| {
                    search.set_query(&text);
                    this_scroll_to_top(&scroll);
                })
                .on_key(on_key)
        };
        let input = SizedBox::new()
            .width(Dimension::Percent(100.0))
            .height(FIELD_HEIGHT)
            .box_child(
                Container::new()
                    .padding(LayoutSpacing::new().left(12).right(12))
                    .child(
                        Row::new()
                            .vertical_alignment(BoxAlignment::Center)
                            .gaps(LayoutSpacing::all(10))
                            .children([
                                icon(Icons::SEARCH, 15, theme.muted).boxed(),
                                Expanded::new().child(field).boxed(),
                            ]),
                    ),
            );

        // The hits, or the words that say why there are none.
        let body: AnyWidget = if hits.is_empty() {
            let words = if search.query().trim().is_empty() {
                "Type to search files, definitions, actions and text".to_owned()
            } else if search.busy() {
                "Searching\u{2026}".to_owned()
            } else {
                search
                    .note()
                    .map_or_else(|| "No results".to_owned(), str::to_owned)
            };
            // As tall as a list is: what is typed goes from having nothing to show to having a list, and the popup must not change
            // size under the one who types.
            SizedBox::new()
                .width(Dimension::Percent(100.0))
                .height(height)
                .box_child(
                    Container::new()
                        .padding(LayoutSpacing::new().top(26).left(14))
                        .child(
                            Text::new(words).text_style(
                                fonts::ui()
                                    .font_size(UI_SIZE)
                                    .color(theme.muted),
                            ),
                        ),
                )
                .boxed()
        } else {
            let choose: Rc<dyn Fn(usize)> = {
                let search = Rc::clone(&search);
                Rc::new(move |index| search.select(index))
            };
            let go: Rc<dyn Fn(usize)> = {
                let (search, close, store) =
                    (Rc::clone(&search), Rc::clone(&this.close), store.clone());
                let hits = Rc::clone(&hits);
                Rc::new(move |index| {
                    if let Some(hit) = hits.get(index) {
                        search.select(index);
                        go_to(hit.clone(), &store, &*close);
                    }
                })
            };
            let rows = hits.iter().enumerate().map(|(index, hit)| {
                SearchRow::new(
                    hit.clone(),
                    index,
                    index == selected,
                    Rc::clone(&choose),
                    Rc::clone(&go),
                )
                .boxed()
            });
            SizedBox::new()
                .height(height)
                .box_child(
                    Scrollable::new()
                        .controller(this.scroll.clone())
                        .vertical_scroll_bar(None)
                        .child(
                            Container::new()
                                .padding(LayoutSpacing::all(LIST_PAD))
                                .child(Column::new().children(rows)),
                        ),
                )
                .boxed()
        };

        // The lines of the file, while the key is held; the footer says what there is and how to see it.
        let status = if hits.is_empty() {
            String::new()
        } else if search.busy() {
            format!("{} results, searching\u{2026}", hits.len())
        } else {
            format!("{} results", hits.len())
        };
        let hint = if previewing {
            "Return to open"
        } else {
            "Hold \u{21e7} to preview"
        };
        let footer = SizedBox::new()
            .width(Dimension::Percent(100.0))
            .height(FOOTER_HEIGHT)
            .box_child(
                Container::new()
                    .padding(LayoutSpacing::new().left(12).right(12))
                    .child(
                        Row::new()
                            .vertical_alignment(BoxAlignment::Center)
                            .children([
                                Text::new(status)
                                    .text_style(
                                        fonts::ui()
                                            .font_size(11)
                                            .color(theme.muted)
                                            .text_overflow(TextOverflow::Clip),
                                    )
                                    .boxed(),
                                Expanded::new().child(ZeroSizedBox).boxed(),
                                Text::new(hint)
                                    .text_style(fonts::ui().font_size(11).color(theme.muted))
                                    .boxed(),
                            ]),
                    ),
            );
        let mut card: Vec<AnyWidget> = vec![
            // The strip of tabs is what the popup is held by.
            {
                let updater = this.updater();
                GestureDetector::new()
                    .on_drag_update(move |drag: DragUpdateData| {
                        updater.set_state(move |state| {
                            state.offset = dragged(state.offset, (drag.delta_x, drag.delta_y))
                        });
                    })
                    .child(SearchTabs::new(tab, pick))
                    .boxed()
            },
            rule::horizontal(theme.divider).boxed(),
            input.boxed(),
            rule::horizontal(theme.divider).boxed(),
            body,
        ];
        if previewing {
            card.push(rule::horizontal(theme.divider).boxed());
            card.push(SearchPreview::new(search.preview()).boxed());
        }
        card.push(rule::horizontal(theme.divider).boxed());
        card.push(footer.boxed());
        let card = Container::new()
            .width(WIDTH)
            .fit_content()
            .decoration(Popup::frame(theme))
            .child(Column::new().children(card));

        // A press outside the card shuts the popup. The card is over it, so a press on the card does not.
        let barrier = {
            let close = Rc::clone(&this.close);
            Button::new()
                .no_decor()
                .on_press(move || close())
                .build(|_state| {
                    SizedBox::new()
                        .width(Dimension::Percent(100.0))
                        .height(Dimension::Percent(100.0))
                })
        };
        Container::new().child(
            Stack::new().add_child(barrier).add_child(
                // A quarter of the room is over the popup and the rest under it: the two spaces share it by their flex.
                SizedBox::new()
                    .width(Dimension::Percent(100.0))
                    .height(Dimension::Percent(100.0))
                    .box_child(
                        Column::new()
                            .horizontal_alignment(BoxAlignment::Center)
                            .children([
                                Expanded::new()
                                    .flex(ABOVE)
                                    .child(ZeroSizedBox)
                                    .boxed(),
                                // The room that is shared out is what is left of the window by the tallest the card can be, and the card is at the
                                // top of it: so the top edge of the card is where it is whether or not the preview is shown, which only makes
                                // the card longer.
                                Container::new()
                                    .padding(centering_padding(this.offset))
                                    .fit_content()
                                    .child(
                                        SizedBox::new()
                                            .height(anchor_height())
                                            .box_child(card),
                                    )
                                    .boxed(),
                                Expanded::new()
                                    .flex(1.0 - ABOVE)
                                    .child(ZeroSizedBox)
                                    .boxed(),
                            ]),
                    ),
            ),
        )
    }
}

/// Brings the list back to its top, for a list that is another.
fn this_scroll_to_top(scroll: &ScrollController) {
    if scroll.offset().y != 0.0 {
        scroll.jump_to(Vec2d { x: 0.0, y: 0.0 });
    }
}

/// Goes to what `hit` is, after the popup is shut: going somewhere else rebuilds what the popup is part of.
fn go_to(hit: Hit, store: &ProviderHandle<Workspace>, close: &dyn Fn()) {
    close();
    let notify = notifier(store);
    let mut later = Later::Nothing;
    store.update(|space| later = search_go::apply(space, &hit, notify));
    match later {
        Later::Nothing => {}
        Later::Save => super::save_active(store),
        Later::Reload => refresh_status(store),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_always_as_tall_as_it_may_be_and_shorter_while_the_preview_is_shown() {
        let pad = (2 * LIST_PAD) as f32;
        assert_eq!(list_height(false), (ROWS as u32 * ROW_HEIGHT) as f32 + pad);
        assert_eq!(
            list_height(true),
            (ROWS_WITH_PREVIEW as u32 * ROW_HEIGHT) as f32 + pad
        );
        assert!(
            list_height(true) < list_height(false),
            "the preview takes room from the list"
        );
    }

    #[test]
    fn a_row_out_of_view_is_scrolled_into_it_and_a_row_in_view_leaves_the_list_where_it_is() {
        let row = ROW_HEIGHT as f32;
        let viewport = list_height(false);
        assert_eq!(reveal_offset(0, 0.0, viewport), None);
        let in_view = ((viewport - 2.0 * LIST_PAD as f32) / row) as usize;
        assert_eq!(
            reveal_offset(in_view - 1, 0.0, viewport),
            None,
            "the last that fits"
        );
        let below = reveal_offset(in_view, 0.0, viewport).expect("below the view");
        assert_eq!(
            below + viewport,
            2.0 * LIST_PAD as f32 + in_view as f32 * row + row,
            "ends where the view ends"
        );
        assert_eq!(reveal_offset(in_view, below, viewport), None);
        assert_eq!(
            reveal_offset(0, 300.0, viewport),
            Some(0.0),
            "back to the top"
        );
        assert_eq!(
            reveal_offset(5, 400.0, viewport),
            Some(5.0 * row),
            "a row above the view is at its top"
        );
    }

    #[test]
    fn a_drag_moves_the_popup_and_cannot_take_it_out_of_reach() {
        assert_eq!(dragged((0.0, 0.0), (10.0, -4.0)), (10.0, -4.0));
        assert_eq!(dragged((10.0, 5.0), (-30.0, 5.0)), (-20.0, 10.0));
        assert_eq!(dragged((0.0, 0.0), (9_999.0, -9_999.0)), (DRAG_X, -DRAG_Y));
    }

    #[test]
    fn the_padding_of_a_card_that_is_centred_makes_room_on_the_side_it_moves_from() {
        // Twice the distance on one side, so that what is centred is that far from the middle; none on the other.
        assert_eq!(
            centering_padding((0.0, 0.0)),
            LayoutSpacing::new().left(0).top(0)
        );
        // Across the room is shared by halves: twice the distance. Down it is shared a quarter above, so the padding above that
        // leaves the card 30 lower is 30 over three quarters, and the padding below that leaves it 30 higher is 30 over a quarter.
        assert_eq!(
            centering_padding((20.0, 30.0)),
            LayoutSpacing::new().left(40).top(40)
        );
        assert_eq!(
            centering_padding((-20.0, -30.0)),
            LayoutSpacing::new().right(40).bottom(120)
        );
        assert_eq!(
            centering_padding((-10.4, 0.0)),
            LayoutSpacing::new().right(21).top(0)
        );
    }

    #[test]
    fn the_popup_fits_a_window_of_a_laptop_with_and_without_the_preview() {
        assert!(card_height(true) < 640.0, "{}", card_height(true));
        assert!(card_height(false) < 640.0, "{}", card_height(false));
    }

    #[test]
    fn the_place_kept_for_the_popup_is_the_tallest_it_gets_so_its_top_does_not_move_with_the_preview()
     {
        assert!(
            card_height(true) > card_height(false),
            "the preview makes it longer"
        );
        assert_eq!(anchor_height(), card_height(true));
        assert!(anchor_height() >= card_height(false));
    }
}
