use super::usages_card::{FILE_COLUMN, ICON, NUMBER_COLUMN, ROW_HEIGHT, text};
use crate::ui::components::icon::material;
use crate::ui::components::popup::Popup;
use crate::ui::file_icons::file_icon;
use crate::ui::theme::CodeTheme;
use crate::ui::usages::{UsageRow, Usages};
use crate::model::workspace::{Workspace, notifier};
use aimer::decoration::Decoration;
use aimer::input::button::Button;
use aimer::input::mouse_region::MouseRegion;
use aimer::share::ShareRef;
use aimer::style::{BorderRadius, LayoutSpacing, TextAlign, TextOverflow};
use aimer::{
    BoxAlignment, BuildContext, Color, Container, Dimension, Expanded, ProviderHandle, RichText,
    Row as Line, SizedBox, State, StateUpdater, StateView, StatefulWidget, Text, TextSpan, Widget,
};
use std::rc::Rc;

/// One place of the list of usages: the file, the line number and the line of code, as a button that goes there. The row that is chosen
/// is marked.
///
/// It draws itself again when it is chosen, or is not any more, and the card does not: a list may have hundreds of places and the
/// pointer chooses one at each row that goes under it as the list scrolls, which, if each choice made the card draw again, would build
/// every row that is in view again for the two that changed.
#[derive(Clone, StatefulWidget)]
pub struct UsageRowView {
    row: ShareRef<UsageRow>,
    theme: CodeTheme,
    store: ProviderHandle<Workspace>,
    usages: Rc<Usages>,
}

impl UsageRowView {
    pub fn new(
        row: ShareRef<UsageRow>,
        theme: CodeTheme,
        store: ProviderHandle<Workspace>,
        usages: Rc<Usages>,
    ) -> Self {
        Self {
            row,
            theme,
            store,
            usages,
        }
    }
}

struct UsageRowViewState {
    row: ShareRef<UsageRow>,
    theme: CodeTheme,
    store: ProviderHandle<Workspace>,
    usages: Rc<Usages>,
    /// What makes a build differ from the one before, so the framework builds it again.
    changes: u32,
    updater: Option<StateUpdater<Self>>,
}

impl UsageRowViewState {
    /// Has the list tell this row when it is chosen or is not any more. Told again where the row is another place of the list.
    fn watch(&self) {
        let Some(updater) = self.updater.clone() else {
            return;
        };
        self.usages.on_row_change(
            self.row.index,
            Rc::new(move || {
                let alive = updater
                    .try_set_state(|state| state.changes += 1)
                    .is_some();
                if alive {
                    aimer::events::window::request_animation_frame();
                }
                alive
            }),
        );
    }
}

impl State<UsageRowView> for UsageRowViewState {
    fn create(widget: UsageRowView) -> Self {
        Self {
            row: widget.row,
            theme: widget.theme,
            store: widget.store,
            usages: widget.usages,
            changes: 0,
            updater: None,
        }
    }

    fn on_mount(&mut self, updater: StateUpdater<Self>) {
        self.updater = Some(updater);
        self.watch();
    }

    fn adopt_config_from(&mut self, new: Self) {
        let moved = self.row.index != new.row.index;
        self.row = new.row;
        self.theme = new.theme;
        self.store = new.store;
        self.usages = new.usages;
        if moved {
            self.watch();
        }
    }

    fn build(this: StateView<Self>, _ctx: &BuildContext) -> impl Widget {
        // What is chosen is read as the row is made, and not kept: the list says when it is not what it was.
        let selected = this.usages.selected() == Some(this.row.index);
        let _changes = this.changes;
        let (theme, index) = (this.theme, this.row.index);
        let (number, pieces) = (this.row.number, this.row.pieces.clone());
        let (store, usages) = (this.store.clone(), Rc::clone(&this.usages));
        let name = this.row.place.path.file_name().map_or_else(
            || this.row.file.to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        // A click, a deep press of the trackpad and Return are the same: go there. A button that does not take the deep press leaves it
        // to what is behind the card, so a firm click would go to the text under the list and not to the place that was pressed.
        let go: Rc<dyn Fn()> = {
            let (place, store, usages) = (this.row.place.clone(), store, Rc::clone(&usages));
            Rc::new(move || {
                // Shut first: going somewhere else rebuilds what the card is part of.
                usages.close();
                let notify = notifier(&store);
                store.update(|space| {
                    let _ = space.go_to_place(&place, notify);
                });
            })
        };
        let (on_press, on_deep_press) = (Rc::clone(&go), Rc::clone(&go));
        let chosen = usages;
        MouseRegion::new()
            // The pointer on a row chooses it, as the keys do: there is one row that is marked, and Return goes to it.
            .on_hover_enter(move || chosen.select(index))
            .box_child(
                Button::new()
                    .no_decor()
                    .on_press(move || on_press())
                    .on_force_press(move || on_deep_press())
                    .build(move |_state| {
                        // The row that is chosen is marked in the pink of the app, as softly as a list of this many rows is read, and its line
                        // number, which is faint, is as bright as the text on it.
                        let background = if selected {
                            theme.marker.with_alpha(Popup::ROW_HIGHLIGHT)
                        } else {
                            Color::Transparent
                        };
                        let code = RichText::new(TextSpan::new("").children(pieces.iter().map(
                            |(text, style)| TextSpan::new(text.as_str()).style(style.clone()),
                        )))
                        .text_style(
                            text()
                                .color(theme.code)
                                .text_overflow(TextOverflow::Clip),
                        );
                        SizedBox::new()
                            .width(Dimension::Percent(100.0))
                            .height(ROW_HEIGHT)
                            .box_child(
                                Container::new()
                                    .width(Dimension::Percent(100.0))
                                    .padding(LayoutSpacing::new().left(8).right(8))
                                    .decoration([
                                        BorderRadius::all(4).notype(),
                                        background.notype(),
                                    ])
                                    .child(
                                        Line::new()
                                            .vertical_alignment(BoxAlignment::Center)
                                            .gaps(LayoutSpacing::all(8))
                                            .children([
                                                material(file_icon(&name), ICON).boxed(),
                                                SizedBox::new()
                                                    .width(FILE_COLUMN)
                                                    .box_child(
                                                        Text::new(name.clone()).text_style(
                                                            text()
                                                                .color(theme.text)
                                                                .text_overflow(TextOverflow::Clip),
                                                        ),
                                                    )
                                                    .boxed(),
                                                SizedBox::new()
                                                    .width(NUMBER_COLUMN)
                                                    .box_child(
                                                        Text::new(number.to_string())
                                                            .text_align(TextAlign::TopRight)
                                                            .text_style(text().color(
                                                                if selected {
                                                                    theme.text
                                                                } else {
                                                                    theme.line_number
                                                                },
                                                            )),
                                                    )
                                                    .boxed(),
                                                Expanded::new().child(code).boxed(),
                                            ]),
                                    ),
                            )
                    }),
            )
    }
}
