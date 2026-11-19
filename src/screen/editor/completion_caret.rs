use crate::completion::{Menu, MenuCell};
use crate::fonts;
use crate::lsp::{Item, ItemKind};
use crate::theme::CodeTheme;
use aimer::decoration::Decoration;
use aimer::input::button::Button;
use aimer::input::input::{CaretContext, DefaultCaret};
use aimer::style::TextAlign;
use aimer::style::{
    BorderRadius, BorderSlice, BorderStyle, BoxBorder, BoxDecoration, LayoutSpacing, TextOverflow,
    TextStyle, Theme,
};
use aimer::{
    AnyWidget, BoxAlignment, BuildContext, Center, Color, Column, Container, Dimension, Expanded,
    Positioned, Row as Line, SizedBox, Stack, State, StateUpdater, StateView, StatefulWidget, Text,
    Widget,
};
use std::rc::Rc;

/// How tall a row of the menu is.
const ROW_HEIGHT: u32 = 26;
/// How tall the hint under the rows is.
const FOOTER_HEIGHT: u32 = 28;
/// How wide the menu is, and how much of it is the detail at the end of a row.
const WIDTH: u32 = 500;
const DETAIL_WIDTH: u32 = 150;
/// The gap between the caret and the menu below it.
const GAP: u32 = 4;
const FONT_SIZE: u32 = 13;
/// The side of the badge that says what kind of item a row is.
const BADGE: u32 = 16;
/// The space between the frame of the menu and its rows.
const PADDING: u32 = 4;

/// The caret of the code area, with the completion menu hanging under it.
///
/// The text area keeps its caret widget and places it at the insertion point on every frame, so a
/// menu drawn as part of it is always where the word is being typed, with nothing to measure. The
/// widget is not rebuilt with the pane: the cell tells it to draw again when the menu changes.
#[derive(Clone, StatefulWidget)]
pub struct CompletionCaret {
    context: CaretContext,
    menu: Rc<MenuCell>,
    color: Color,
}

impl CompletionCaret {
    pub fn new(context: CaretContext, menu: Rc<MenuCell>, color: Color) -> Self {
        Self {
            context,
            menu,
            color,
        }
    }
}

struct CompletionCaretState {
    context: CaretContext,
    menu: Rc<MenuCell>,
    color: Color,
    /// What makes a build differ from the one before, so the framework builds it again.
    changes: u32,
}

impl State<CompletionCaret> for CompletionCaretState {
    fn create(widget: CompletionCaret) -> Self {
        Self {
            context: widget.context,
            menu: widget.menu,
            color: widget.color,
            changes: 0,
        }
    }

    fn on_mount(&mut self, updater: StateUpdater<Self>) {
        // The caret in this widget blinks, which is the field asking for it to be painted again. It is not painted
        // again by the caret's own callback, nested here as it is, and the widget is built again when it is told.
        let blinking = updater.clone();
        self.context.on_repaint(Rc::new(move || {
            let _ = blinking.try_set_state(|state| state.changes += 1);
        }));
        self.menu.on_change(Rc::new(move || {
            updater.try_set_state(|state| state.changes += 1);
            // A key the menu takes never reaches the field, which is what asks for a frame when it
            // edits: without this the menu would change on screen at the next thing that draws.
            aimer::events::window::request_animation_frame();
        }));
    }

    fn adopt_config_from(&mut self, new: Self) {
        self.context = new.context;
        self.menu = new.menu;
        self.color = new.color;
    }

    fn build(this: StateView<Self>, ctx: &BuildContext) -> impl Widget {
        let _changes = this.changes;
        let theme = *CodeTheme::of(ctx);
        let caret = DefaultCaret::new(this.context.clone(), this.color);
        let mut stack = Stack::new().add_child(caret);
        let geometry = this.context.geometry();
        let extent = this.context.visible_extent();
        if let Some((rows, position, total)) = this.menu.with(|menu| {
            menu.map(|menu| {
                (
                    rows_of(menu, &this.menu, &theme),
                    menu.selected() + 1,
                    menu.len(),
                )
            })
        }) {
            let height = menu_height(rows.len());
            let (left, top) = place(
                geometry.x,
                geometry.y,
                geometry.height.max(0.0),
                extent,
                height as f32,
            );
            // A press on the menu is the menu's, and not the field's: it would put the caret where the click is. The
            // menu is wider than the caret it hangs from, so the field tells it where it was pressed.
            this.context
                .set_popup(Some((left, top, WIDTH as f32, height as f32)));
            let dismissed = Rc::clone(&this.menu);
            this.context
                .on_popup_dismiss(Some(Rc::new(move || dismissed.dismiss())));
            let cell = Rc::clone(&this.menu);
            this.context
                .on_popup_press(Some(Rc::new(move |_, y| {
                    if let Some(row) = row_at(y) {
                        let number = cell.with(|menu| {
                            menu.and_then(|menu| menu.rows().nth(row).map(|row| row.number))
                        });
                        if let Some(number) = number {
                            cell.pick(number);
                        }
                    }
                })));
            stack = stack.add_child(
                Positioned::new()
                    .left(Dimension::Px(left))
                    .top(Dimension::Px(top))
                    .child(menu_box(rows, position, total, theme)),
            );
        } else {
            this.context.set_popup(None);
            this.context.on_popup_press(None);
            this.context.on_popup_dismiss(None);
        }
        stack
    }
}

/// Which row of the window of the menu a press `y` from the top of the menu is on, if it is on one: the padding above
/// the rows, and the hint under them, are not.
fn row_at(y: f32) -> Option<usize> {
    let row = (y - PADDING as f32) / ROW_HEIGHT as f32;
    (row >= 0.0).then(|| row as usize)
}

/// How tall the menu is with `rows` rows: the rows in their padding, and the hint under them.
fn menu_height(rows: usize) -> u32 {
    rows as u32 * ROW_HEIGHT + 4 * PADDING + FOOTER_HEIGHT + 1
}

/// Where the menu is, as how far from the caret's left and top edges, for a caret that is at `(x, y)` in its field and
/// `caret_height` tall, in a field whose text is shown as far as `extent`. It hangs below the caret; where it does not
/// fit there and there is more room above, it goes above; and where it would go past the right of the field it
/// moves left, as far as the caret is from the left.
fn place(
    x: f32,
    y: f32,
    caret_height: f32,
    extent: Option<(f32, f32)>,
    menu_height: f32,
) -> (f32, f32) {
    let below = (0.0, caret_height + GAP as f32);
    let Some((right, bottom)) = extent else {
        return below;
    };
    let (room_below, room_above) = (bottom - (y + caret_height + GAP as f32), y - GAP as f32);
    let top = if menu_height > room_below && room_above > room_below {
        -(menu_height + GAP as f32)
    } else {
        below.1
    };
    let overflow = x + WIDTH as f32 - right;
    (-overflow.max(0.0).min(x.max(0.0)), top)
}

/// The colors the badges of the kinds of items are in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hue {
    Pink,
    Green,
    Purple,
    Orange,
    Blue,
    Amber,
    Teal,
    Gray,
}

impl Hue {
    /// The color of the glyph and, fainter, of the edge and the fill of the badge.
    fn color(self) -> Color {
        match self {
            Self::Pink => Color::Rgb(240, 100, 178),
            Self::Green => Color::Rgb(104, 200, 150),
            Self::Purple => Color::Rgb(150, 124, 244),
            Self::Orange => Color::Rgb(250, 128, 92),
            Self::Blue => Color::Rgb(112, 150, 246),
            Self::Amber => Color::Rgb(238, 168, 72),
            Self::Teal => Color::Rgb(88, 196, 210),
            Self::Gray => Color::Rgb(156, 152, 176),
        }
    }
}

/// What a badge says of an item: the glyph in it, its color, and the kind in words for a row that has no detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Glyph {
    text: &'static str,
    hue: Hue,
    name: &'static str,
}

/// The glyph of an item. A macro is told by the server as a function whose name ends in `!`.
fn glyph_of(item: &Item) -> Glyph {
    let glyph = |text, hue, name| Glyph { text, hue, name };
    match item.kind {
        ItemKind::Function | ItemKind::Method
            if item.label.ends_with('!')
                || item
                    .detail
                    .as_deref()
                    .is_some_and(|detail| detail.starts_with("macro")) =>
        {
            glyph("#", Hue::Orange, "macro")
        }
        ItemKind::Function => glyph("fn", Hue::Green, "function"),
        ItemKind::Method => glyph("fn", Hue::Green, "method"),
        ItemKind::Constructor => glyph("fn", Hue::Green, "constructor"),
        ItemKind::Field | ItemKind::Property => glyph("p", Hue::Teal, "field"),
        ItemKind::Variable | ItemKind::Value => glyph("v", Hue::Teal, "variable"),
        ItemKind::Class | ItemKind::Struct => glyph("S", Hue::Pink, "struct"),
        ItemKind::Interface => glyph("T", Hue::Purple, "trait"),
        ItemKind::Module | ItemKind::Folder | ItemKind::File => glyph("M", Hue::Blue, "module"),
        ItemKind::Enum => glyph("E", Hue::Amber, "enum"),
        ItemKind::EnumMember => glyph("e", Hue::Amber, "variant"),
        ItemKind::Constant => glyph("C", Hue::Blue, "constant"),
        ItemKind::Keyword => glyph("k", Hue::Gray, "keyword"),
        ItemKind::Snippet => glyph("s", Hue::Gray, "snippet"),
        ItemKind::TypeParameter => glyph("T", Hue::Purple, "type parameter"),
        ItemKind::Unit
        | ItemKind::Event
        | ItemKind::Operator
        | ItemKind::Reference
        | ItemKind::Color
        | ItemKind::Text
        | ItemKind::Other => glyph("\u{b7}", Hue::Gray, ""),
    }
}

/// What a row shows, owned so that the button of the row can draw it again when the pointer is over it.
struct RowView {
    number: usize,
    typed: String,
    rest: String,
    glyph: Glyph,
    label_detail: String,
    detail: String,
    selected: bool,
    deprecated: bool,
}

/// The rows of the window of the menu, the selected one marked, each of them a button that picks its item.
fn rows_of(menu: &Menu, cell: &Rc<MenuCell>, theme: &CodeTheme) -> Vec<AnyWidget> {
    menu.rows()
        .map(|row| {
            // The word typed is a whole number of characters of the label, which the menu checked.
            let (typed, rest) = row.item.label.split_at(row.typed);
            let glyph = glyph_of(row.item);
            let view = RowView {
                number: row.number,
                typed: typed.to_owned(),
                rest: rest.to_owned(),
                glyph,
                label_detail: row
                    .item
                    .label_detail
                    .clone()
                    .unwrap_or_default(),
                detail: row
                    .item
                    .detail
                    .as_deref()
                    .filter(|detail| !detail.is_empty())
                    .unwrap_or(glyph.name)
                    .to_owned(),
                selected: row.selected,
                deprecated: row.item.deprecated,
            };
            let (cell, theme) = (Rc::clone(cell), *theme);
            let number = view.number;
            Button::new()
                .no_decor()
                .on_press(move || cell.pick(number))
                .build(move |state| row_widget(&view, state.is_hovered, theme))
                .boxed()
        })
        .collect()
}

/// One row of the menu: the kind, the name with the word typed marked, what is said beside it, and the detail.
fn row_widget(view: &RowView, hovered: bool, theme: CodeTheme) -> impl Widget + use<> {
    let code = || {
        TextStyle::new()
            .font_family(fonts::code_family())
            .font_size(FONT_SIZE)
    };
    // What is said to be deprecated is dimmed, and still there to be chosen.
    let plain = if view.deprecated {
        theme.muted
    } else {
        theme.text
    };
    let background = if view.selected {
        theme.selected
    } else if hovered {
        theme.hover
    } else {
        Color::Transparent
    };
    Container::new()
        .width(Dimension::Percent(100.0))
        .height(ROW_HEIGHT)
        .padding(
            LayoutSpacing::new()
                .left(8)
                .right(12)
                .top(2)
                .bottom(2),
        )
        .margin(LayoutSpacing::new().bottom(4))
        .decoration([BorderRadius::all(5).notype(), background.notype()])
        .child(
            Line::new()
                .vertical_alignment(BoxAlignment::Center)
                .gaps(LayoutSpacing::all(8))
                .children([
                    badge(view.glyph).boxed(),
                    Line::new()
                        .children([
                            Text::new(view.typed.clone())
                                .text_style(code().color(theme.accent))
                                .boxed(),
                            Text::new(view.rest.clone())
                                .text_style(code().color(plain))
                                .boxed(),
                        ])
                        .boxed(),
                    // What is said beside the label, such as where the item is imported from, takes the room
                    // that is left, and is cut with an ellipsis when it does not fit.
                    Expanded::new()
                        .child(
                            Text::new(view.label_detail.clone()).text_style(
                                code()
                                    .font_size(FONT_SIZE - 1)
                                    .color(theme.muted)
                                    .text_overflow(TextOverflow::Ellipsis),
                            ),
                        )
                        .boxed(),
                    SizedBox::new()
                        .width(DETAIL_WIDTH)
                        .box_child(
                            Text::new(view.detail.clone())
                                .text_align(TextAlign::TopRight)
                                .text_style(
                                    fonts::ui()
                                        .font_size(FONT_SIZE - 1)
                                        .color(theme.muted)
                                        .text_overflow(TextOverflow::Ellipsis),
                                ),
                        )
                        .boxed(),
                ]),
        )
}

/// The small rounded square that tells the kind of an item: a glyph in the color of the kind, on a fainter fill of it, in an
/// edge of it that is fainter than the glyph.
fn badge(glyph: Glyph) -> impl Widget {
    let color = glyph.hue.color();

    Container::new()
        .width(BADGE)
        .height(BADGE)
        .decoration(
            BoxDecoration::new()
                .background_color(color.with_alpha(0.08))
                .border_radius(3)
                .border(BoxBorder::all(
                    BorderSlice::new()
                        .color(color.with_alpha(0.75))
                        .style(BorderStyle::Solid)
                        .stroke(1.0),
                )),
        )
        .child(
            Center::new().box_child(
                Text::new(glyph.text)
                    .text_align(TextAlign::MidCenter)
                    .text_style(
                        TextStyle::new()
                            .font_family(fonts::ui_family())
                            .font_size(if glyph.text.len() > 1 { 10 } else { 12 })
                            .font_weight(aimer::FontWeight::Bold)
                            .color(color),
                    ),
            ),
        )
}

/// The frame around the rows, with the hint for the keys under them.
fn menu_box(rows: Vec<AnyWidget>, position: usize, total: usize, theme: CodeTheme) -> impl Widget {
    // The height is given and not left to the rows: the caret this hangs from is a line tall, and
    // the room it offers its child is no more than that.
    let rows_height = rows.len() as u32 * ROW_HEIGHT + 4 * PADDING;
    Container::new()
        .width(WIDTH)
        .height(menu_height(rows.len()))
        .decoration(frame(theme))
        .child(
            Column::new()
                .gaps(LayoutSpacing::new())
                .children([
                    Container::new()
                        .width(Dimension::Percent(100.0))
                        .height(rows_height)
                        .padding(LayoutSpacing::all(PADDING))
                        .child(Column::new().children(rows))
                        .boxed(),
                    footer(position, total, theme).boxed(),
                ]),
        )
}

/// The hint under the rows, set off from them by a line, and which of how many items is selected.
fn footer(position: usize, total: usize, theme: CodeTheme) -> impl Widget {
    Container::new()
        .width(Dimension::Percent(100.0))
        .height(FOOTER_HEIGHT + 1)
        .padding(LayoutSpacing::new().left(12).right(12).top(8))
        .decoration([BoxDecoration::new()
            .border(
                BoxBorder::new().top(
                    BorderSlice::new()
                        .color(theme.divider)
                        .style(BorderStyle::Solid)
                        .stroke(1.0),
                ),
            )
            .notype()])
        .child(
            Line::new().children([
                Text::new("Enter or Tab to insert, Esc to close")
                    .text_style(
                        fonts::ui()
                            .font_size(FONT_SIZE - 2)
                            .color(theme.muted),
                    )
                    .boxed(),
                Expanded::new().child(SizedBox::new()).boxed(),
                Text::new(format!("{position} of {total}"))
                    .text_style(
                        fonts::ui()
                            .font_size(FONT_SIZE - 2)
                            .color(theme.muted),
                    )
                    .boxed(),
            ]),
        )
}

/// The look of the frame: a dark fill and a line around it, so the menu stands out of the code.
fn frame(theme: CodeTheme) -> BoxDecoration {
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

#[cfg(test)]
mod tests {
    use super::*;

    const ROWS_10: f32 =
        10.0 * ROW_HEIGHT as f32 + 2.0 * PADDING as f32 + FOOTER_HEIGHT as f32 + 1.0;

    #[test]
    fn the_menu_hangs_below_the_caret_where_there_is_room_and_where_the_field_is_not_known() {
        // A caret 20 tall at 100, in a field 800 tall and 1000 wide.
        assert_eq!(
            place(50.0, 100.0, 20.0, Some((1000.0, 800.0)), ROWS_10),
            (0.0, 24.0)
        );
        assert_eq!(
            place(50.0, 700.0, 20.0, None, ROWS_10),
            (0.0, 24.0),
            "a field that has not said where it ends"
        );
    }

    #[test]
    fn the_menu_goes_above_the_caret_where_it_does_not_fit_below_and_there_is_more_room_above() {
        let (left, top) = place(50.0, 700.0, 20.0, Some((1000.0, 780.0)), ROWS_10);
        assert_eq!(left, 0.0);
        assert_eq!(top, -(ROWS_10 + 4.0), "its bottom is a gap above the caret");
        // Neither fits, and there is more room below than above: it stays below.
        assert_eq!(
            place(50.0, 40.0, 20.0, Some((1000.0, 200.0)), ROWS_10).1,
            24.0
        );
        // Neither fits, and there is more room above: above.
        assert_eq!(
            place(50.0, 180.0, 20.0, Some((1000.0, 200.0)), ROWS_10).1,
            -(ROWS_10 + 4.0)
        );
        // It just fits below: the line is where it was.
        assert_eq!(
            place(
                50.0,
                100.0,
                20.0,
                Some((1000.0, 100.0 + 20.0 + 4.0 + ROWS_10)),
                ROWS_10
            )
            .1,
            24.0
        );
    }

    #[test]
    fn the_menu_moves_left_where_it_would_go_past_the_right_of_the_field_but_not_past_its_left() {
        let width = WIDTH as f32;
        assert_eq!(
            place(300.0, 100.0, 20.0, Some((300.0 + width, 800.0)), ROWS_10).0,
            0.0,
            "it just fits"
        );
        assert_eq!(
            place(
                300.0,
                100.0,
                20.0,
                Some((300.0 + width - 50.0, 800.0)),
                ROWS_10
            )
            .0,
            -50.0
        );
        assert_eq!(
            place(30.0, 100.0, 20.0, Some((100.0, 800.0)), ROWS_10).0,
            -30.0,
            "no further than the left of the field"
        );
        assert_eq!(
            place(0.0, 100.0, 20.0, Some((100.0, 800.0)), ROWS_10).0,
            0.0
        );
    }

    fn item_of(kind: ItemKind, label: &str, detail: Option<&str>) -> Item {
        use crate::lsp::Edit;
        Item {
            label: label.to_owned(),
            kind,
            detail: detail.map(str::to_owned),
            label_detail: None,
            sort: label.to_owned(),
            filter: label.to_owned(),
            edit: Edit {
                range: None,
                text: label.to_owned(),
                extra: vec![],
            },
            resolve: None,
            deprecated: false,
        }
    }

    #[test]
    fn the_kinds_of_items_have_the_glyphs_and_the_colors_of_the_symbols_of_an_outline() {
        let of = |kind, label: &str, detail| {
            let glyph = glyph_of(&item_of(kind, label, detail));
            (glyph.text, glyph.hue)
        };
        assert_eq!(of(ItemKind::Struct, "Widget", None), ("S", Hue::Pink));
        assert_eq!(
            of(ItemKind::Function, "detect_overflow", Some("fn(&self)")),
            ("fn", Hue::Green)
        );
        assert_eq!(of(ItemKind::Method, "push", None), ("fn", Hue::Green));
        assert_eq!(
            of(ItemKind::Interface, "StatelessWidget", None),
            ("T", Hue::Purple)
        );
        assert_eq!(
            of(ItemKind::Module, "aimer_element::", None),
            ("M", Hue::Blue)
        );
        assert_eq!(of(ItemKind::Enum, "RatioOption", None), ("E", Hue::Amber));
        assert_eq!(of(ItemKind::Keyword, "let", None), ("k", Hue::Gray));
    }

    #[test]
    fn a_macro_is_told_from_a_function_by_its_name_or_its_detail() {
        let glyph = |label: &str, detail| glyph_of(&item_of(ItemKind::Function, label, detail));
        assert_eq!(
            (
                glyph("assert_eq!", None).text,
                glyph("assert_eq!", None).hue
            ),
            ("#", Hue::Orange)
        );
        assert_eq!(glyph("vec", Some("macro_rules! vec")).text, "#");
        assert_eq!(
            glyph("main", Some("fn() -> ()")).text,
            "fn",
            "a name that is not a macro's"
        );
        assert_eq!(glyph("assert_eq!", None).name, "macro");
    }

    #[test]
    fn every_kind_of_item_has_a_glyph_that_is_short_enough_for_its_badge() {
        use ItemKind::*;
        for kind in [
            Text,
            Method,
            Function,
            Constructor,
            Field,
            Variable,
            Class,
            Interface,
            Module,
            Property,
            Unit,
            Value,
            Enum,
            Keyword,
            Snippet,
            Color,
            File,
            Reference,
            Folder,
            EnumMember,
            Constant,
            Struct,
            Event,
            Operator,
            TypeParameter,
            Other,
        ] {
            let glyph = glyph_of(&item_of(kind, "x", None));
            assert!(
                !glyph.text.is_empty() && glyph.text.chars().count() <= 2,
                "{kind:?} has {:?}",
                glyph.text
            );
        }
    }

    #[test]
    fn a_press_is_on_the_row_it_is_in_and_on_none_in_the_padding_above_them() {
        let (padding, row) = (PADDING as f32, ROW_HEIGHT as f32);
        assert_eq!(row_at(-1.0), None);
        assert_eq!(
            row_at(padding - 0.5),
            None,
            "the padding above the first row"
        );
        assert_eq!(row_at(padding), Some(0), "the first pixel of the first row");
        assert_eq!(row_at(padding + row - 0.5), Some(0), "the last of it");
        assert_eq!(row_at(padding + row), Some(1));
        assert_eq!(row_at(padding + 3.0 * row + 2.0), Some(3));
        // Past the last row is a number too, which the menu has no row for: it is not a pick.
        assert_eq!(row_at(padding + 50.0 * row), Some(50));
    }

    #[test]
    fn the_height_of_a_menu_is_its_rows_and_its_hint() {
        assert_eq!(menu_height(0), 2 * PADDING + FOOTER_HEIGHT + 1);
        assert_eq!(menu_height(10) as f32, ROWS_10);
        assert_eq!(menu_height(3) - menu_height(2), ROW_HEIGHT);
    }
}
