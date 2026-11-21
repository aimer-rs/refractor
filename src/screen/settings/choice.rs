use crate::components::pad::pad;
use crate::fonts;
use crate::theme::CodeTheme;
use aimer::decoration::Decoration;
use aimer::input::button::Button;
use aimer::style::{BorderRadius, LayoutSpacing, Theme};
use aimer::{BuildContext, Color, Container, Row, StatelessWidget, Text, Widget};
use std::rc::Rc;

/// Buttons side by side, one for each choice, of which one is selected. Pressing another selects it; pressing the
/// one that is selected does nothing.
#[derive(Clone, StatelessWidget)]
pub struct Choice {
    labels: Vec<&'static str>,
    selected: usize,
    on_select: Rc<dyn Fn(usize)>,
}

impl Choice {
    pub fn new(labels: Vec<&'static str>, selected: usize, on_select: Rc<dyn Fn(usize)>) -> Self {
        Self {
            labels,
            selected,
            on_select,
        }
    }
}

impl StatelessWidget for Choice {
    fn build(&self, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let buttons = self
            .labels
            .iter()
            .enumerate()
            .map(|(index, label)| {
                let (label, chosen, on_select) =
                    (*label, index == self.selected, Rc::clone(&self.on_select));
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
                        Container::new()
                            .padding(pad(5, 14, 5, 14))
                            .decoration([BorderRadius::all(6).notype(), background.notype()])
                            .child(Text::new(label).text_style(
                                fonts::ui().font_size(13).color(if chosen {
                                    theme.text
                                } else {
                                    theme.muted
                                }),
                            ))
                    })
                    .boxed()
            })
            .collect::<Vec<_>>();
        Container::new()
            .padding(pad(2, 2, 2, 2))
            .decoration([
                BorderRadius::all(8).notype(),
                theme.selected.with_alpha(0.5).notype(),
            ])
            .child(
                Row::new()
                    .gaps(LayoutSpacing::all(2))
                    .children(buttons),
            )
    }
}
