use crate::ui::components::pad::pad;
use crate::ui::components::rule;
use crate::ui::fonts;
use crate::ui::theme::CodeTheme;
use aimer::style::LayoutSpacing;
use aimer::{BoxAlignment, Column, Container, Expanded, FontWeight, Row, Text, Widget};

/// The heading at the top of a settings category page.
pub fn section_heading(title: &'static str, theme: CodeTheme) -> impl Widget {
    Container::new().padding(pad(0, 0, 8, 0)).child(
        Text::new(title).text_style(
            fonts::ui()
                .font_size(15)
                .font_weight(FontWeight::Bold)
                .color(theme.text),
        ),
    )
}

/// One setting: its name over a line that says what it does, on the left, the `control` that
/// changes it on the right, and a divider line beneath it.
pub fn setting_row(
    name: &'static str,
    hint: &'static str,
    control: impl Widget + 'static,
    theme: CodeTheme,
) -> impl Widget {
    Column::new().children([
        Container::new()
            .padding(pad(10, 0, 12, 0))
            .child(
                Row::new()
                    .vertical_alignment(BoxAlignment::Center)
                    .gaps(LayoutSpacing::all(20))
                    .children([
                        Expanded::new()
                            .child(
                                Column::new()
                                    .gaps(LayoutSpacing::all(2))
                                    .children([
                                        Text::new(name)
                                            .text_style(fonts::ui().font_size(13).color(theme.text))
                                            .boxed(),
                                        Text::new(hint)
                                            .text_style(
                                                fonts::ui()
                                                    .font_size(11)
                                                    .color(theme.text.with_alpha(0.62)),
                                            )
                                            .wrapped()
                                            .boxed(),
                                    ]),
                            )
                            .boxed(),
                        control.boxed(),
                    ]),
            )
            .boxed(),
        rule::horizontal(theme.divider).boxed(),
    ])
}
