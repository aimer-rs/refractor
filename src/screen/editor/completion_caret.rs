use crate::completion::{Call, Label, Menu, MenuCell};
use crate::components::badge::{Badge, Glyph, Hue};
use crate::components::popup::{Anchor, Popup};
use crate::fonts;
use crate::lsp::{Item, ItemKind};
use crate::theme::CodeTheme;
use aimer::decoration::Decoration;
use aimer::input::input::{CaretContext, DefaultCaret};
use aimer::style::TextAlign;
use aimer::style::{
    BorderRadius, BorderSlice, BorderStyle, BoxBorder, BoxDecoration, FontWeight, LayoutSpacing,
    TextOverflow, TextStyle, Theme,
};
use aimer::{
    AnyWidget, BoxAlignment, BuildContext, Color, Column, Container, Dimension, Expanded,
    Positioned, Row as Line, SizedBox, Stack, State, StateUpdater, StateView, StatefulWidget, Text,
    Widget,
};
use std::cell::Cell;
use std::rc::Rc;

/// How tall a row of the menu is.
const ROW_HEIGHT: u32 = 23;
/// The space under a row, between it and the next: none, as in the list of usages, where the row that is chosen marks itself.
const ROW_GAP: u32 = 0;
/// How far apart the tops of two rows are: what the list is measured in, by the height of the menu, by where a pointer is
/// and by how far the wheel has turned it.
const ROW_PITCH: u32 = ROW_HEIGHT + ROW_GAP;
/// How tall the hint under the rows is.
const FOOTER_HEIGHT: u32 = 26;
/// How wide the menu is, and how much of it is what the item gives back, at the end of a row.
const WIDTH: u32 = 560;
const DETAIL_WIDTH: u32 = 190;
const FONT_SIZE: u32 = 13;
/// The space between the parts of a row: the badge, the name and what goes with it.
const ROW_GAPS: u32 = 5;
/// The space between the frame of the menu and its rows.
const PADDING: u32 = 6;

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
    /// How far the wheel has turned the list, in logical pixels, that was not enough for a row yet: a trackpad turns it
    /// in steps smaller than a row, and the list moves when they add up to one.
    wheel: Rc<Cell<f32>>,
    /// What makes a build differ from the one before, so the framework builds it again.
    changes: u32,
}

impl State<CompletionCaret> for CompletionCaretState {
    fn create(widget: CompletionCaret) -> Self {
        Self {
            context: widget.context,
            menu: widget.menu,
            color: widget.color,
            wheel: Rc::default(),
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
        self.menu
            .on_change(Popup::redraw_watcher(updater, |state| state.changes += 1));
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
        if let Some((rows, position, total)) = this
            .menu
            .with(|menu| menu.map(|menu| (rows_of(menu, &theme), menu.selected() + 1, menu.len())))
        {
            let height = menu_height(rows.len());
            // Where the menu goes in the field, and from the caret, which is what the field places it by.
            let anchor = Anchor {
                x: geometry.x,
                y: geometry.y,
                height: geometry.height.max(0.0),
            };
            let (field_left, field_top) =
                Popup::place(anchor, (WIDTH as f32, height as f32), extent);
            let (left, top) = (field_left - geometry.x, field_top - geometry.y);
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
            // The pointer and the keys move one selection, so the row under the pointer is the selected one, and a move that
            // is not on a row leaves it where it was. A list that scrolls under a pointer that is still does not select.
            let cell = Rc::clone(&this.menu);
            this.context
                .on_popup_hover(Some(Rc::new(move |_, y| {
                    let number = row_at(y).and_then(|row| {
                        cell.with(|menu| {
                            menu.and_then(|menu| menu.rows().nth(row).map(|row| row.number))
                        })
                    });
                    if let Some(number) = number {
                        cell.hover(number);
                    }
                })));
            // The wheel scrolls the list, and the code under it stays where it is, even when the list is at its end.
            let cell = Rc::clone(&this.menu);
            let pending = Rc::clone(&this.wheel);
            this.context
                .on_popup_scroll(Some(Rc::new(move |delta_y| {
                    let (rows, left) = wheel_rows(pending.get(), delta_y);
                    pending.set(left);
                    cell.scroll(rows);
                    true
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
            this.context.on_popup_hover(None);
            this.context.on_popup_scroll(None);
            this.context.on_popup_dismiss(None);
            this.wheel.set(0.0);
        }
        stack
    }
}

/// Which row of the window of the menu a press `y` from the top of the menu is on, if it is on one: the padding above
/// the rows, and the hint under them, are not.
fn row_at(y: f32) -> Option<usize> {
    let row = (y - PADDING as f32) / ROW_PITCH as f32;
    (row >= 0.0).then(|| row as usize)
}

/// How tall the rows of the menu are, with `rows` of them, in their padding.
fn rows_height(rows: usize) -> u32 {
    rows as u32 * ROW_PITCH + 2 * PADDING
}

/// How tall the menu is with `rows` rows: the rows in their padding, and the hint under them.
fn menu_height(rows: usize) -> u32 {
    rows_height(rows) + FOOTER_HEIGHT + 1
}

/// How many whole rows a turn of the wheel of `delta_y` logical pixels moves the list, down where positive, when `pending`
/// pixels of the turns before it were not enough for a row, and how much is pending after it. A scroll event tells how far
/// the content moves, so a delta that is negative is the list going down.
fn wheel_rows(pending: f32, delta_y: f32) -> (isize, f32) {
    let total = pending - delta_y;
    let rows = (total / ROW_PITCH as f32).trunc();
    (rows as isize, total - rows * ROW_PITCH as f32)
}

/// The glyph of an item. A macro is told by the server as a function whose name ends in `!`.
fn glyph_of(item: &Item) -> Glyph {
    match item.kind {
        ItemKind::Function | ItemKind::Method
            if item.label.ends_with('!')
                || item
                    .detail
                    .as_deref()
                    .is_some_and(|detail| detail.starts_with("macro")) =>
        {
            Glyph::new("#", Hue::Orange, "macro")
        }
        kind => Glyph::of(kind),
    }
}

/// What a row shows, owned by the widget of the row.
struct RowView {
    typed: String,
    rest: String,
    glyph: Glyph,
    label_detail: String,
    /// Whether the row has a badge for its kind: a keyword has none.
    badge: bool,
    /// The parameters of a function, in their brackets, which are set after its name.
    params: String,
    /// What is set at the end of the row: what a function gives back, or what the server says of any other item.
    detail: String,
    selected: bool,
    deprecated: bool,
}

/// Whether an item of `kind` has a badge: a keyword is not a symbol of the code, and has none, though its row keeps the room of one.
fn has_badge(kind: ItemKind) -> bool {
    !matches!(kind, ItemKind::Keyword)
}

/// The rows of the window of the menu, the selected one marked. A press on one is the field's to tell, as it is on the
/// rest of the menu, and so a row is not a button.
fn rows_of(menu: &Menu, theme: &CodeTheme) -> Vec<AnyWidget> {
    menu.rows()
        .map(|row| {
            // The word typed is a whole number of characters of the label, which the menu checked, and is not longer than the name that
            // is shown of it, which may be the label without the colons of a module.
            let name = Label::name(&row.item.label, row.item.kind);
            let (typed, rest) = name.split_at(row.typed.min(name.len()));
            let glyph = glyph_of(row.item);
            let detail = row
                .item
                .detail
                .as_deref()
                .filter(|detail| !detail.is_empty());
            let signature = detail.and_then(Call::signature);
            let view = RowView {
                typed: typed.to_owned(),
                rest: rest.to_owned(),
                glyph,
                label_detail: row
                    .item
                    .label_detail
                    .as_deref()
                    .map(Label::detail)
                    .unwrap_or_default(),
                badge: has_badge(row.item.kind),
                params: signature
                    .map(|signature| signature.params.to_owned())
                    .unwrap_or_default(),
                detail: match signature {
                    // A function says what it takes beside its name, and what it gives back at the end of the row.
                    Some(signature) => signature.returns.unwrap_or_default().to_owned(),
                    None => detail.unwrap_or(glyph.name).to_owned(),
                },
                selected: row.selected,
                deprecated: row.item.deprecated,
            };
            row_widget(&view, *theme).boxed()
        })
        .collect()
}

/// One row of the menu: the kind, the name with the word typed marked and what it takes, and at the end what it gives back.
fn row_widget(view: &RowView, theme: CodeTheme) -> impl Widget + use<> {
    // Names, parameters and what they give back are code, and are set as it is: in the font of the code.
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
    // What goes with the name is dimmer than the name, and is as plain as it on the row that is chosen.
    let dim = if view.selected {
        plain.with_alpha(0.8)
    } else {
        plain.with_alpha(0.55)
    };
    // One look for what is chosen, whether the keys or the pointer chose it: the pink of the app, as softly as the list of usages
    // marks its row.
    let background = if view.selected {
        theme.marker.with_alpha(Popup::ROW_HIGHLIGHT)
    } else {
        Color::Transparent
    };
    Container::new()
        .width(Dimension::Percent(100.0))
        .height(ROW_HEIGHT)
        .padding(LayoutSpacing::new().left(8).right(10))
        .margin(LayoutSpacing::new().bottom(ROW_GAP))
        .decoration([BorderRadius::all(4).notype(), background.notype()])
        .child(
            Line::new()
                .vertical_alignment(BoxAlignment::Center)
                .gaps(LayoutSpacing::all(ROW_GAPS))
                .children([
                    // A keyword has no badge, and keeps the room of one, so that the names of all the rows start at one line.
                    if view.badge {
                        Badge::new(view.glyph).boxed()
                    } else {
                        SizedBox::new()
                            .width(Badge::SIZE)
                            .height(Badge::SIZE)
                            .boxed()
                    },
                    Line::new()
                        .children([
                            // What was typed is the pink of the app, and the whole name is bold.
                            Text::new(view.typed.clone())
                                .text_style(
                                    code()
                                        .font_weight(FontWeight::Bold)
                                        .color(theme.marker),
                                )
                                .boxed(),
                            Text::new(view.rest.clone())
                                .text_style(
                                    code()
                                        .font_weight(FontWeight::Bold)
                                        .color(plain),
                                )
                                .boxed(),
                        ])
                        .boxed(),
                    // What it takes, and what is said beside the name, such as where it is imported from, take the room that is
                    // left, and are cut when it does not fit.
                    Expanded::new()
                        .child(
                            Text::new(format!(
                                "{}{}",
                                view.params,
                                if view.label_detail.is_empty() {
                                    String::new()
                                } else {
                                    format!(" {}", view.label_detail)
                                }
                            ))
                            .text_style(
                                code()
                                    .color(dim)
                                    .text_overflow(TextOverflow::Clip),
                            ),
                        )
                        .boxed(),
                    SizedBox::new()
                        .width(DETAIL_WIDTH)
                        .box_child(
                            Text::new(view.detail.clone())
                                .text_align(TextAlign::TopRight)
                                .text_style(
                                    code()
                                        .color(dim)
                                        .text_overflow(TextOverflow::Clip),
                                ),
                        )
                        .boxed(),
                ]),
        )
}

/// The frame around the rows, with the hint for the keys under them.
fn menu_box(rows: Vec<AnyWidget>, position: usize, total: usize, theme: CodeTheme) -> impl Widget {
    // The height is given and not left to the rows: the caret this hangs from is a line tall, and
    // the room it offers its child is no more than that.
    let rows_height = rows_height(rows.len());
    Container::new()
        .width(WIDTH)
        .height(menu_height(rows.len()))
        .decoration(Popup::frame(theme))
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

#[cfg(test)]
mod tests {
    use super::*;

    const ROWS_10: f32 =
        10.0 * ROW_PITCH as f32 + 2.0 * PADDING as f32 + FOOTER_HEIGHT as f32 + 1.0;

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
    fn a_keyword_has_no_badge_and_every_other_kind_has() {
        assert!(!has_badge(ItemKind::Keyword));
        for kind in [
            ItemKind::Function,
            ItemKind::Method,
            ItemKind::Struct,
            ItemKind::Variable,
            ItemKind::Field,
            ItemKind::Module,
            ItemKind::Constant,
        ] {
            assert!(has_badge(kind), "{kind:?}");
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
        let (padding, row) = (PADDING as f32, ROW_PITCH as f32);
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
    fn the_rows_are_as_far_apart_as_they_are_drawn() {
        // The row is its height and the gap under it: a pointer on the last row of a full window is on the last row, and
        // the window is as tall as all of them.
        assert_eq!(ROW_PITCH, ROW_HEIGHT + ROW_GAP);
        let last = PADDING as f32 + (Menu::WINDOW - 1) as f32 * ROW_PITCH as f32;
        assert_eq!(row_at(last + 1.0), Some(Menu::WINDOW - 1));
        assert!(last + ROW_HEIGHT as f32 <= (menu_height(Menu::WINDOW) - FOOTER_HEIGHT) as f32);
    }

    #[test]
    fn the_wheel_moves_the_rows_a_whole_row_at_a_time_and_keeps_what_is_left_for_the_next_turn() {
        let pitch = ROW_PITCH as f32;
        // Content moves up, which is the list going down, when the delta is negative.
        assert_eq!(wheel_rows(0.0, -pitch), (1, 0.0));
        assert_eq!(wheel_rows(0.0, pitch * 2.0), (-2, 0.0));
        let (rows, left) = wheel_rows(0.0, -pitch * 0.4);
        assert_eq!(rows, 0);
        assert!((left - pitch * 0.4).abs() < 1e-4, "{left}");
        let (rows, left) = wheel_rows(left, -pitch * 0.7);
        assert_eq!(rows, 1, "two small turns are a row");
        assert!((left - pitch * 0.1).abs() < 1e-4, "{left}");
        let (rows, _) = wheel_rows(pitch * 0.9, pitch * 0.2);
        assert_eq!(rows, 0, "turning back first takes what was kept");
    }

    #[test]
    fn the_height_of_a_menu_is_its_rows_and_its_hint() {
        assert_eq!(menu_height(0), 2 * PADDING + FOOTER_HEIGHT + 1);
        assert_eq!(menu_height(10) as f32, ROWS_10);
        assert_eq!(menu_height(3) - menu_height(2), ROW_PITCH);
    }
}
