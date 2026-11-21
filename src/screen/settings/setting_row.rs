use crate::components::pad::pad;
use crate::components::rule;
use crate::fonts;
use crate::theme::CodeTheme;
use aimer::style::LayoutSpacing;
use aimer::{BoxAlignment, Column, Container, Expanded, Row, Text, Widget};

/// The space between what the page is made of, in logical pixels.
pub const SECTION_GAP: u32 = 14;

/// The name of a group of settings, with a line under it.
pub fn section_title(title: &'static str, theme: CodeTheme) -> impl Widget {
    Column::new()
        .gaps(LayoutSpacing::all(6))
        .children([
            Container::new()
                .padding(pad(10, 0, 0, 0))
                .child(Text::new(title).text_style(fonts::ui().font_size(13).color(theme.muted)))
                .boxed(),
            rule::horizontal(theme.divider).boxed(),
        ])
}

/// One setting: its name over a line that says what it does, on the left, and the `control` that changes it
/// on the right.
pub fn setting_row(
    name: &'static str,
    hint: &'static str,
    control: impl Widget + 'static,
    theme: CodeTheme,
) -> impl Widget {
    Container::new().padding(pad(2, 0, 2, 0)).child(
        Row::new()
            .vertical_alignment(BoxAlignment::Center)
            .gaps(LayoutSpacing::all(24))
            .children([
                Expanded::new()
                    .child(
                        Column::new()
                            .gaps(LayoutSpacing::all(3))
                            .children([
                                Text::new(name)
                                    .text_style(fonts::ui().font_size(14).color(theme.text))
                                    .boxed(),
                                Text::new(hint)
                                    .text_style(
                                        fonts::ui()
                                            .font_size(12)
                                            .color(theme.line_number),
                                    )
                                    .boxed(),
                            ]),
                    )
                    .boxed(),
                control.boxed(),
            ]),
    )
}
