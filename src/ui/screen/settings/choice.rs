use crate::ui::components::icon::icon;
use crate::ui::components::pad::pad;
use crate::ui::fonts;
use crate::ui::theme::CodeTheme;
use aimer::decoration::{BorderSlice, BorderStyle, BoxBorder, BoxDecoration, Decoration};
use aimer::input::button::Button;
use aimer::style::{BorderRadius, LayoutSpacing, Theme};
use aimer::{BuildContext, Color, Container, Row, StatelessWidget, StaticSvg, Text, Widget};
use std::rc::Rc;

#[derive(Clone, Copy)]
enum ChoiceOption {
    Text(&'static str),
    Icon(&'static StaticSvg),
}

/// Buttons side by side, one for each choice, of which one is selected. Pressing another selects it; pressing the
/// one that is selected does nothing.
#[derive(Clone, StatelessWidget)]
pub struct Choice {
    options: Vec<ChoiceOption>,
    selected: usize,
    on_select: Rc<dyn Fn(usize)>,
}

impl Choice {
    pub fn new(labels: Vec<&'static str>, selected: usize, on_select: Rc<dyn Fn(usize)>) -> Self {
        Self {
            options: labels.into_iter().map(ChoiceOption::Text).collect(),
            selected,
            on_select,
        }
    }

    /// A segmented choice whose buttons display icons instead of text labels.
    pub fn icons(
        glyphs: Vec<&'static StaticSvg>,
        selected: usize,
        on_select: Rc<dyn Fn(usize)>,
    ) -> Self {
        Self {
            options: glyphs.into_iter().map(ChoiceOption::Icon).collect(),
            selected,
            on_select,
        }
    }
}

impl StatelessWidget for Choice {
    fn build(&self, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let buttons = self
            .options
            .iter()
            .enumerate()
            .map(|(index, &option)| {
                let (chosen, on_select) = (index == self.selected, Rc::clone(&self.on_select));
                Button::new()
                    .no_decor()
                    .on_press(move || {
                        if !chosen {
                            on_select(index);
                        }
                    })
                    .build(move |state| {
                        let background = if chosen {
                            theme.marker.with_alpha(0.28)
                        } else if state.is_hovered {
                            theme.hover
                        } else {
                            Color::Transparent
                        };
                        let foreground = if chosen {
                            theme.text
                        } else {
                            theme.text.with_alpha(0.68)
                        };
                        match option {
                            ChoiceOption::Text(label) => Container::new()
                                .fit_content()
                                .padding(pad(4, 10, 4, 10))
                                .decoration([BorderRadius::all(4).notype(), background.notype()])
                                .child(
                                    Text::new(label)
                                        .text_style(fonts::ui().font_size(12).color(foreground)),
                                )
                                .notype(),
                            ChoiceOption::Icon(glyph) => Container::new()
                                .fit_content()
                                .padding(pad(4, 9, 4, 9))
                                .decoration([BorderRadius::all(4).notype(), background.notype()])
                                .child(icon(glyph, 14, foreground))
                                .notype(),
                        }
                    })
                    .boxed()
            })
            .collect::<Vec<_>>();
        Container::new()
            .fit_content()
            .padding(pad(2, 2, 2, 2))
            .decoration(
                BoxDecoration::new()
                    .background_color(Color::Transparent)
                    .border_radius(6)
                    .border(BoxBorder::all(
                        BorderSlice::new()
                            .color(theme.marker.with_alpha(0.28))
                            .style(BorderStyle::Solid)
                            .stroke(1.0),
                    )),
            )
            .child(
                Row::new()
                    .gaps(LayoutSpacing::all(2))
                    .children(buttons),
            )
    }
}
