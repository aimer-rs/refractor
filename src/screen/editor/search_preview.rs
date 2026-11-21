use crate::fonts;
use crate::search::Preview;
use crate::theme::CodeTheme;
use aimer::style::{LayoutSpacing, TextAlign, TextOverflow, TextStyle, Theme};
use aimer::{AnyWidget, BoxAlignment, BuildContext, Color, Column, Container, Dimension, Row, SizedBox, StatelessWidget, Text, Widget};
use std::rc::Rc;

/// How tall a line of the preview is, and the part the line numbers take.
pub const LINE_HEIGHT: u32 = 15;
const NUMBER_WIDTH: u32 = 36;
/// The padding above and below the lines.
const PAD: u32 = 4;
/// The most lines a preview has, which is what the area is as tall as.
pub const LINES: u32 = (Preview::BEFORE + 1 + Preview::AFTER) as u32;
/// How tall the area is, in logical pixels: the same for every preview, so that what is under it does not move.
pub const HEIGHT: u32 = LINES * LINE_HEIGHT + 2 * PAD;

/// The lines of a file around what the chosen hit goes to, in the font of code, with the line that is gone to marked. It is
/// shown while the preview key is held.
#[derive(Clone, StatelessWidget)]
pub struct SearchPreview {
    preview: Option<Rc<Preview>>,
}

impl SearchPreview {
    pub fn new(preview: Option<Rc<Preview>>) -> Self {
        Self { preview }
    }
}

impl StatelessWidget for SearchPreview {
    fn build(&self, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let code = || TextStyle::new().font_family(fonts::code_family()).font_size(11);
        let lines: Vec<AnyWidget> = match &self.preview {
            Some(preview) => preview
                .lines
                .iter()
                .enumerate()
                .map(|(at, line)| {
                    let number = preview.first + at;
                    let focus = preview.focus == Some(number);
                    SizedBox::new()
                        .width(Dimension::Percent(100.0))
                        .height(LINE_HEIGHT)
                        .box_child(
                            Container::new()
                                .width(Dimension::Percent(100.0))
                                .decoration(if focus { theme.marker.with_alpha(0.16) } else { Color::Transparent })
                                .child(
                                    Row::new().vertical_alignment(BoxAlignment::Center).gaps(LayoutSpacing::all(10)).children([
                                        SizedBox::new()
                                            .width(NUMBER_WIDTH)
                                            .box_child(
                                                Text::new(number.to_string())
                                                    .text_align(TextAlign::TopRight)
                                                    .text_style(code().color(if focus { theme.text } else { theme.line_number })),
                                            )
                                            .boxed(),
                                        Text::new(line.clone())
                                            .text_style(code().color(theme.code).text_overflow(TextOverflow::Clip))
                                            .boxed(),
                                    ]),
                                ),
                        )
                        .boxed()
                })
                .collect(),
            None => Vec::new(),
        };
        SizedBox::new().width(Dimension::Percent(100.0)).height(HEIGHT).box_child(
            Container::new()
                .width(Dimension::Percent(100.0))
                .decoration(theme.editor)
                .padding(LayoutSpacing::new().top(PAD).bottom(PAD))
                .child(Column::new().children(lines)),
        )
    }
}
