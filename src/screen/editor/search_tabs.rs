use crate::fonts;
use crate::search::Tab;
use crate::theme::CodeTheme;
use aimer::input::button::Button;
use aimer::style::{FontWeight, LayoutSpacing, Theme};
use aimer::{BoxAlignment, BuildContext, Color, Column, Container, Row, SizedBox, StatelessWidget, Text, Widget};
use std::rc::Rc;

/// How tall the strip of tabs is, and the line under the one that is shown.
pub const HEIGHT: u32 = 32;
const LINE: u32 = 2;

/// The tabs of the search: All, Types, Files, Symbols, Actions and Text. The one that is shown has the pink of the app under it.
#[derive(Clone, StatelessWidget)]
pub struct SearchTabs {
    active: Tab,
    /// Shows another tab.
    pick: Rc<dyn Fn(Tab)>,
}

impl SearchTabs {
    pub fn new(active: Tab, pick: Rc<dyn Fn(Tab)>) -> Self {
        Self { active, pick }
    }
}

impl StatelessWidget for SearchTabs {
    fn build(&self, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let tabs = Tab::ALL.map(|tab| {
            let (active, pick) = (self.active == tab, Rc::clone(&self.pick));
            Button::new().no_decor().on_press(move || pick(tab)).build(move |state| {
                let color = if active || state.is_hovered { theme.text } else { theme.muted };
                let weight = if active { FontWeight::Bold } else { FontWeight::Normal };
                SizedBox::new().height(HEIGHT).box_child(
                    Column::new().children([
                        Container::new()
                            .height(HEIGHT - LINE)
                            .padding(LayoutSpacing::new().left(10).right(10).top(7))
                            .child(Text::new(tab.title()).text_style(fonts::ui().font_size(13).font_weight(weight).color(color)))
                            .boxed(),
                        Container::new()
                            .height(LINE)
                            .color(if active { theme.marker } else { Color::Transparent })
                            .child(SizedBox::new().height(LINE))
                            .boxed(),
                    ]),
                )
            })
        });
        Container::new()
            .padding(LayoutSpacing::new().left(6).right(6))
            .child(Row::new().vertical_alignment(BoxAlignment::Center).children(tabs))
    }
}
