use crate::ui::components::icon::icon;
use crate::ui::components::pad::pad;
use crate::ui::fonts;
use crate::ui::theme::CodeTheme;
use aimer::decoration::{BorderSlice, BorderStyle, BoxBorder, BoxDecoration, Decoration};
use aimer::input::button::Button;
use aimer::style::{BorderRadius, LayoutSpacing, Theme};
use aimer::{
    BoxAlignment, BuildContext, Color, Container, Icons, Row, SizedBox, StatelessWidget, Text,
    Widget,
};
use std::ops::RangeInclusive;
use std::rc::Rc;

/// A number with a button on each side that takes one from it and adds one to it, which stops at the ends
/// of its range: the button that cannot go further is dimmed and does nothing.
#[derive(Clone, StatelessWidget)]
pub struct Stepper {
    value: i64,
    range: RangeInclusive<i64>,
    unit: &'static str,
    /// How much a press changes the value by.
    step: i64,
    on_change: Rc<dyn Fn(i64)>,
}

impl Stepper {
    pub fn new(
        value: i64,
        range: RangeInclusive<i64>,
        unit: &'static str,
        on_change: Rc<dyn Fn(i64)>,
    ) -> Self {
        Self {
            value,
            range,
            unit,
            step: 1,
            on_change,
        }
    }

    /// Makes a press change the value by `step`, which is one unless it is said.
    #[must_use]
    pub fn step(mut self, step: i64) -> Self {
        self.step = step.max(1);
        self
    }

    /// The value one `step` from `value`, which is the end of the range where a whole step does not fit in what is left of it, or
    /// `None` where the value is at that end already.
    fn stepped(value: i64, step: i64, range: &RangeInclusive<i64>) -> Option<i64> {
        let next = value
            .checked_add(step)?
            .clamp(*range.start(), *range.end());
        (next != value).then_some(next)
    }
}

/// A square button with a glyph, that is dimmed when it has nothing to do.
fn step_button(
    glyph: &'static aimer::StaticSvg,
    target: Option<i64>,
    on_change: Rc<dyn Fn(i64)>,
    theme: CodeTheme,
) -> impl Widget {
    let mut button = Button::new().no_decor();
    if let Some(target) = target {
        button = button.on_press(move || on_change(target));
    }
    button.build(move |state| {
        let live = target.is_some();
        Container::new()
            .padding(pad(4, 5, 4, 5))
            .decoration([
                BorderRadius::all(4).notype(),
                if live && state.is_hovered {
                    theme.hover
                } else {
                    Color::Transparent
                }
                .notype(),
            ])
            .child(icon(
                glyph,
                13,
                if live {
                    theme.text
                } else {
                    theme.line_number.with_alpha(0.5)
                },
            ))
    })
}

impl StatelessWidget for Stepper {
    fn build(&self, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let less = Self::stepped(self.value, -self.step, &self.range);
        let more = Self::stepped(self.value, self.step, &self.range);
        Container::new()
            .fit_content()
            .padding(pad(2, 3, 2, 3))
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
                    .vertical_alignment(BoxAlignment::Center)
                    .gaps(LayoutSpacing::all(4))
                    .children([
                        step_button(Icons::MINUS, less, Rc::clone(&self.on_change), theme).boxed(),
                        SizedBox::new()
                            .width(60)
                            .box_child(
                                Text::new(format!("{} {}", self.value, self.unit))
                                    .text_align(aimer::style::TextAlign::MidCenter)
                                    .text_style(fonts::ui().font_size(12).color(theme.text)),
                            )
                            .boxed(),
                        step_button(Icons::PLUS, more, Rc::clone(&self.on_change), theme).boxed(),
                    ]),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_can_be_more_than_one_and_the_last_one_goes_to_the_end_when_the_whole_step_does_not_fit()
     {
        let range = 100..=30_000;
        assert_eq!(Stepper::stepped(1000, 100, &range), Some(1100));
        assert_eq!(Stepper::stepped(1000, -100, &range), Some(900));
        assert_eq!(
            Stepper::stepped(100, -100, &range),
            None,
            "no less than the smallest"
        );
        assert_eq!(
            Stepper::stepped(29_950, 100, &range),
            Some(30_000),
            "what is left of the range is a step"
        );
        assert_eq!(Stepper::stepped(30_000, 100, &range), None);
        assert_eq!(Stepper::stepped(150, -100, &range), Some(100));
        let by_hundreds = Stepper::new(1000, range, "ms", Rc::new(|_| {})).step(100);
        assert_eq!(by_hundreds.step, 100);
        assert_eq!(
            Stepper::new(1, 0..=5, "", Rc::new(|_| {})).step,
            1,
            "one unless it is said"
        );
    }

    #[test]
    fn a_step_stops_at_the_ends_of_the_range() {
        let range = 8..=32;
        assert_eq!(Stepper::stepped(12, 1, &range), Some(13));
        assert_eq!(Stepper::stepped(12, -1, &range), Some(11));
        assert_eq!(
            Stepper::stepped(32, 1, &range),
            None,
            "no more than the largest"
        );
        assert_eq!(
            Stepper::stepped(8, -1, &range),
            None,
            "no less than the smallest"
        );
        assert_eq!(
            Stepper::stepped(i64::MAX, 1, &range),
            None,
            "an overflow is not a step"
        );
    }
}
