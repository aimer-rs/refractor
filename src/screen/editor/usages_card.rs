use crate::components::popup::{Anchor, Popup};
use crate::fonts;
use crate::theme::CodeTheme;
use super::usage_row::UsageRowView;
use crate::usages::{Heading, UsageEntry, UsageList, Usages};
use crate::workspace::Workspace;
use aimer::share::ShareRef;
use aimer::style::{LayoutSpacing, TextOverflow, TextStyle, Theme};
use aimer::{
    AnyWidget, BoxAlignment, BuildContext, Column, Container, Expanded, NumberExt, Positioned, ProviderContext, ProviderHandle, Row as Line, Scrollable, SizedBox, Stack, State, StateUpdater, StateView, StatefulWidget, Text, Vec2d, Widget, ZeroSizedBox,
};
use std::rc::Rc;

/// How wide the card is, and how tall at most: what is longer is scrolled in it. It is tall enough for the twenty places or so that a
/// name is used in, and as dense as a list of that many has to be.
const WIDTH: f32 = 550.0;
const MAX_HEIGHT: f32 = 380.0;
/// How tall the heading and a place are, the note of how many are not listed, and the space inside the frame round the places.
const HEADING_HEIGHT: u32 = 32;
pub(super) const ROW_HEIGHT: u32 = 20;
const NOTE_HEIGHT: u32 = 22;
const BODY_PADDING: u32 = 6;
pub(super) const UI_SIZE: u32 = 13;
/// How wide the column of the names of files is, and the column of line numbers, and the file's icon.
pub(super) const FILE_COLUMN: u32 = 130;
pub(super) const NUMBER_COLUMN: u32 = 30;
pub(super) const ICON: u32 = 14;
/// What the frame takes of the height: the line round the card, top and bottom.
const LINES: f32 = 2.0;
/// How tall the part of a long list that is shown is: the card less its heading and its frame.
const VIEWPORT: f32 = MAX_HEIGHT - LINES - HEADING_HEIGHT as f32;

/// The card that lists the places that use a name, or implement it, over the editor beside the count that was pressed: a heading that
/// says what the name is, where it is defined and how many places there are, and under it a row for each, with the file, the line
/// and the line of code.
///
/// It is drawn over the text area, which fills the place it is in, and it is drawn again when the cell says that the list opened
/// or shut. A place in it is a button: pressing it goes there, and shuts the list.
#[derive(Clone, StatefulWidget)]
pub struct UsagesCard {
    usages: Rc<Usages>,
}

impl UsagesCard {
    pub fn new(usages: Rc<Usages>) -> Self {
        Self { usages }
    }
}

struct UsagesCardState {
    usages: Rc<Usages>,
    /// What makes a build differ from the one before, so the framework builds it again.
    changes: u32,
}

impl State<UsagesCard> for UsagesCardState {
    fn create(widget: UsagesCard) -> Self {
        Self {
            usages: widget.usages,
            changes: 0,
        }
    }

    fn on_mount(&mut self, updater: StateUpdater<Self>) {
        self.usages
            .on_list_change(Popup::redraw_watcher(updater, |state| state.changes += 1));
        // The keys move the choice, and a row that is chosen below what is shown, or above it, is brought in.
        let scroll = self.usages.scroll();
        self.usages.on_reveal(Rc::new(move |index, count| {
            // The height of a row is what the list measured, which is not what it was asked to be to the last fraction: it is found from
            // how far the list scrolls, and a list that has not been measured has rows as tall as they were asked to be.
            let extent = scroll.max_extent().y;
            let pitch = if extent > 0.0 && count > 0 {
                (extent + VIEWPORT - 2.0 * BODY_PADDING as f32) / count as f32
            } else {
                ROW_HEIGHT as f32
            };
            if let Some(offset) = reveal_offset(index, scroll.offset().y, VIEWPORT, pitch) {
                scroll.jump_to(Vec2d { x: 0.0, y: offset });
            }
        }));
    }

    fn adopt_config_from(&mut self, new: Self) {
        self.usages = new.usages;
    }

    fn build(this: StateView<Self>, ctx: &BuildContext) -> impl Widget {
        let _changes = this.changes;
        let Some(list) = this.usages.list() else {
            return ZeroSizedBox.boxed();
        };
        let theme = *CodeTheme::of(ctx);
        let store = ProviderHandle::<Workspace>::of(ctx);
        let natural = natural_height(&list);
        let scrolls = natural > MAX_HEIGHT;
        let height = natural.min(MAX_HEIGHT);
        // A point, which a force click is, has no height of its own: the card hangs below the line it was on.
        let line = if list.spot.height > 0.0 { list.spot.height } else { 18.0 };
        let anchor = Anchor {
            x: list.spot.x,
            y: list.spot.y,
            height: line,
        };
        let (left, top) = Popup::place(anchor, (WIDTH, height), list.spot.extent);

        // The list keeps the places, which are shared with the cell and not copied, and a row is built when it is in view: a name used in
        // three hundred places costs the rows that are shown, and not the three hundred.
        let usages = Rc::clone(&this.usages);
        let entries = list.entries.clone();
        let body = Column::new()
            .list(list.entries.clone())
            .builder(move |entry| entry_row(entry, &entries, &theme, &store, &usages));
        let body_height = height - LINES - HEADING_HEIGHT as f32;
        let places: AnyWidget = if scrolls {
            // What scrolls is given the height it scrolls in, which is the card's less the heading and the lines.
            SizedBox::new()
                .height(body_height)
                .box_child(
                    Scrollable::new().controller(this.usages.scroll()).child(
                        Container::new()
                            .padding(LayoutSpacing::all(BODY_PADDING))
                            .child(body),
                    ),
                )
                .boxed()
        } else {
            Container::new()
                .padding(LayoutSpacing::all(BODY_PADDING))
                .child(body)
                .boxed()
        };
        let content = Container::new().width(WIDTH);
        let content = if scrolls {
            content.height(MAX_HEIGHT)
        } else {
            content.fit_content()
        };
        let card = content
            .decoration(Popup::frame(theme))
            .child(Column::new().children([heading(&list.heading, &theme).boxed(), places]));
        // The layer fills what the card is over, so that the card is inside the bounds of what holds it and a wheel on it is found.
        Container::new()
            .child(
                Stack::new().add_child(
                    Positioned::new()
                        .left(left.px())
                        .top(top.px())
                        .child(card),
                ),
            )
            .boxed()
    }
}

/// About how tall the card is for `list`: its rows are all the same height, so this is exact.
fn natural_height(list: &UsageList) -> f32 {
    let rows = list.len();
    let note = if list.omitted > 0 { NOTE_HEIGHT } else { 0 };
    LINES
        + (HEADING_HEIGHT + 2 * BODY_PADDING + note + rows as u32 * ROW_HEIGHT) as f32
}

/// The text of the list: in the font of the window, which the lines of code are in too, as a list of this many is read at a glance.
pub(super) fn text() -> TextStyle {
    fonts::ui().font_size(UI_SIZE)
}

/// The heading: what the name is and where it is defined, and how many places there are.
fn heading(heading: &Heading, theme: &CodeTheme) -> impl Widget + use<> {
    Container::new()
        .height(HEADING_HEIGHT)
        .decoration(theme.muted.with_alpha(0.04))
        .padding(LayoutSpacing::new().left(14).right(14))
        .child(
            Line::new()
                .vertical_alignment(BoxAlignment::Center)
                .gaps(LayoutSpacing::all(8))
                .children([
                    Expanded::new()
                        .child(
                            Line::new()
                                .vertical_alignment(BoxAlignment::Center)
                                .gaps(LayoutSpacing::all(8))
                                .children([
                                    Text::new(heading.title())
                                        .text_style(
                                            text()
                                                .font_weight(aimer::style::FontWeight::Bold)
                                                .color(theme.text),
                                        )
                                        .boxed(),
                                    Text::new(format!("in {}", heading.file()))
                                        .text_style(
                                            text()
                                                .color(theme.muted)
                                                .text_overflow(TextOverflow::Clip),
                                        )
                                        .boxed(),
                                ]),
                        )
                        .boxed(),
                    Text::new(heading.count_text())
                        .text_style(text().color(theme.muted).font_size(12))
                        .boxed(),
                ]),
        )
}

/// A line of the list: a place, as a button that goes there, or the note of how many are not listed.
fn entry_row(
    entry: &UsageEntry,
    entries: &ShareRef<[UsageEntry]>,
    theme: &CodeTheme,
    store: &ProviderHandle<Workspace>,
    usages: &Rc<Usages>,
) -> AnyWidget {
    match entry {
        UsageEntry::Place(row) => {
            // A handle to the row inside the list that is shared: nothing of it is copied.
            let index = row.index;
            let row = entries.clone().project(move |entries| match &entries[index] {
                UsageEntry::Place(row) => row,
                UsageEntry::More(_) => unreachable!("the index was taken from a place of this list, which is not changed"),
            });
            UsageRowView::new(row, *theme, store.clone(), Rc::clone(usages)).boxed()
        }
        UsageEntry::More(omitted) => Container::new()
            .height(NOTE_HEIGHT)
            .padding(LayoutSpacing::new().left(12))
            .child(Text::new(format!("and {omitted} more")).text_style(text().color(theme.muted)))
            .boxed(),
    }
}

/// Where a long list is to be scrolled to, from `offset`, for the row `index` to be in the `viewport` that shows it: none when it is
/// in view already, so that a key that moves the choice inside what is shown does not move the list under it.
fn reveal_offset(index: usize, offset: f32, viewport: f32, row: f32) -> Option<f32> {
    let padding = BODY_PADDING as f32;
    let top = index as f32 * row;
    let bottom = padding + top + row + padding;
    if top < offset {
        Some(top)
    } else if bottom > offset + viewport {
        Some(bottom - viewport)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::{Place, Position};
    use crate::usages::UsageRow;
    use aimer::input::input::FieldSpot;
    use aimer::share::{ShareRef, Shared};
    use std::path::PathBuf;

    fn list(rows_per_file: &[usize], omitted: usize) -> UsageList {
        let mut entries: Vec<UsageEntry> = Vec::new();
        for (index, count) in rows_per_file.iter().enumerate() {
            for line in 0..*count {
                entries.push(UsageEntry::Place(UsageRow {
                    place: Place {
                        path: PathBuf::from(format!("/p/{index}.rs")),
                        at: Position { line: line as u32, character: 0 },
                        end: Position { line: line as u32, character: 1 },
                    },
                    index: entries.len(),
                    file: ShareRef::from(format!("{index}.rs")),
                    number: line + 1,
                    text: String::new(),
                    mark: 0..0,
                    pieces: ShareRef::from([]),
                }));
            }
        }
        if omitted > 0 {
            entries.push(UsageEntry::More(omitted));
        }
        UsageList {
            heading: Heading { kind: None, name: "t".into(), definition: "t.rs".into(), noun: "usages", count: 1 },
            entries: Shared::<[UsageEntry]>::from(entries.into_boxed_slice()).into_share_ref(),
            omitted,
            spot: FieldSpot { x: 0.0, y: 0.0, width: 0.0, height: 0.0, extent: None },
        }
    }

    #[test]
    fn the_height_of_the_card_is_its_heading_its_padding_each_place_and_a_note_when_some_are_left_out() {
        let frame = LINES + (HEADING_HEIGHT + 2 * BODY_PADDING) as f32;
        assert_eq!(natural_height(&list(&[], 0)), frame);
        assert_eq!(natural_height(&list(&[3, 1], 0)), frame + (4 * ROW_HEIGHT) as f32, "a row for each place, in whichever file");
        assert_eq!(
            natural_height(&list(&[1], 40)) - natural_height(&list(&[1], 0)),
            NOTE_HEIGHT as f32,
            "the note is a row"
        );
    }

    #[test]
    fn a_list_of_more_than_the_card_may_hold_is_scrolled() {
        let rows_that_fit = ((MAX_HEIGHT - LINES) as u32 - HEADING_HEIGHT - 2 * BODY_PADDING) / ROW_HEIGHT;
        assert!(natural_height(&list(&[rows_that_fit as usize], 0)) <= MAX_HEIGHT);
        assert!(natural_height(&list(&[rows_that_fit as usize + 1], 0)) > MAX_HEIGHT);
    }

    #[test]
    fn a_row_that_is_chosen_out_of_view_is_scrolled_into_it_and_one_in_view_leaves_the_list_where_it_is() {
        let row = ROW_HEIGHT as f32;
        let pad = BODY_PADDING as f32;
        // The first rows are in view at the top, and nothing moves for them.
        assert_eq!(reveal_offset(0, 0.0, VIEWPORT, row), None);
        let rows_in_view = ((VIEWPORT - 2.0 * pad) / row) as usize;
        assert_eq!(reveal_offset(rows_in_view - 1, 0.0, VIEWPORT, row), None, "the last that fits");
        // The next one is below what is shown: the list moves up by as much as brings its bottom to the bottom of the view.
        let below = reveal_offset(rows_in_view, 0.0, VIEWPORT, row).expect("below the view");
        assert!(below > 0.0 && below <= row + pad, "by about a row: {below}");
        let bottom_of_row = pad + rows_in_view as f32 * row + row + pad;
        assert_eq!(below + VIEWPORT, bottom_of_row, "so that the row ends where the view ends");
        // Once there, it is in view, and so are the rows above it down to the first one that is.
        assert_eq!(reveal_offset(rows_in_view, below, VIEWPORT, row), None);
        // A row above the top of what is shown brings the list back down to it, and the first row to the very top.
        let above = reveal_offset(0, 100.0, VIEWPORT, row).expect("above the view");
        assert_eq!(above, 0.0, "the first row is at the top, with its padding");
        assert_eq!(reveal_offset(5, 400.0, VIEWPORT, row), Some(5.0 * row), "a row above the view is at its top");
        // Rows that are a little taller than asked, as a long list measures them, bring the last row to the end of the list and not a
        // few rows short of it.
        let measured = 20.32;
        let last = reveal_offset(285, 0.0, VIEWPORT, measured).expect("far below");
        assert!((last - (285.0 * measured + measured + 2.0 * pad - VIEWPORT)).abs() < 0.01, "{last}");
    }
}
