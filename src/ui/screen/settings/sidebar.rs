use super::SettingsSection;
use crate::ui::components::icon::icon;
use crate::ui::components::pad::pad;
use crate::ui::fonts;
use crate::ui::theme::CodeTheme;
use aimer::decoration::Decoration;
use aimer::input::button::Button;
use aimer::style::{BorderRadius, LayoutSpacing, Theme};
use aimer::{
    BoxAlignment, BuildContext, Color, Column, Container, Dimension, Row, SizedBox,
    StatelessWidget, Text, Widget,
};
use std::rc::Rc;

/// The category navigation rail on the left side of the settings screen.
#[derive(Clone, StatelessWidget)]
pub struct SettingsSidebar {
    selected: SettingsSection,
    on_select: Rc<dyn Fn(SettingsSection)>,
}

impl SettingsSidebar {
    /// Width of the category navigation rail, in logical pixels.
    pub const WIDTH: u32 = 176;

    pub fn new(selected: SettingsSection, on_select: Rc<dyn Fn(SettingsSection)>) -> Self {
        Self {
            selected,
            on_select,
        }
    }
}

impl StatelessWidget for SettingsSidebar {
    fn build(&self, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let items = SettingsSection::ALL
            .iter()
            .map(|&section| {
                nav_item(
                    section,
                    section == self.selected,
                    Rc::clone(&self.on_select),
                    theme,
                )
                .boxed()
            })
            .collect::<Vec<_>>();

        SizedBox::new()
            .width(Self::WIDTH)
            .height(Dimension::Percent(100.0))
            .box_child(
                Container::new()
                    .color(theme.sidebar)
                    .padding(pad(10, 10, 10, 10))
                    .child(
                        Column::new()
                            .gaps(LayoutSpacing::all(2))
                            .children(items),
                    ),
            )
    }
}

fn nav_item(
    section: SettingsSection,
    selected: bool,
    on_select: Rc<dyn Fn(SettingsSection)>,
    theme: CodeTheme,
) -> impl Widget {
    Button::new()
        .no_decor()
        .on_press(move || {
            if !selected {
                on_select(section);
            }
        })
        .build(move |state| {
            let background = if selected {
                theme.marker.with_alpha(0.28)
            } else if state.is_hovered {
                theme.hover
            } else {
                Color::Transparent
            };
            let foreground = if selected {
                theme.text
            } else {
                theme.text.with_alpha(0.78)
            };
            SizedBox::new()
                .width(Dimension::Percent(100.0))
                .box_child(
                    Container::new()
                        .padding(pad(7, 10, 7, 10))
                        .decoration([BorderRadius::all(6).notype(), background.notype()])
                        .child(
                            Row::new()
                                .vertical_alignment(BoxAlignment::Center)
                                .gaps(LayoutSpacing::all(9))
                                .children([
                                    icon(section.icon(), 14, foreground).boxed(),
                                    Text::new(section.label())
                                        .text_style(fonts::ui().font_size(12).color(foreground))
                                        .boxed(),
                                ]),
                        ),
                )
        })
}
