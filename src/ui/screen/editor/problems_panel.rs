//! The panel under the editor that lists the problems the language server found in the file being edited,
//! the worst first. A click on one takes the caret to it. It is opened and shut by the mark that floats at
//! the top-right of the editor.

use crate::ui::components::icon::icon;
use crate::ui::components::pad::pad;
use crate::ui::components::rule;
use crate::ui::fonts;
use crate::services::lsp::{Problem, Status, Summary, Weight};
use crate::ui::theme::CodeTheme;
use crate::model::workspace::Workspace;
use aimer::decoration::Decoration;
use aimer::input::button::Button;
use aimer::style::{BorderRadius, FontWeight, LayoutSpacing, TextOverflow, Theme};
use aimer::{
    AnyWidget, BoxAlignment, BuildContext, Center, Color, Column, Container, Dimension, Expanded,
    Icons, ProviderContext, ProviderHandle, Row, Scrollable, SizedBox, StatelessWidget, Text,
    Widget, ZeroSizedBox,
};

/// How tall the header of the panel is, and each row of the list.
const HEADER_HEIGHT: u32 = 30;
const ROW_HEIGHT: u32 = 26;
/// How tall the panel starts, and the limits of dragging its top edge, in logical pixels.
pub const PANEL_HEIGHT: f32 = 220.0;
pub const PANEL_MIN: f32 = 90.0;
pub const PANEL_MAX: f32 = 640.0;

/// The icon and the color a weight is drawn in.
fn mark_of(weight: Weight, theme: &CodeTheme) -> (&'static aimer::StaticSvg, Color) {
    match weight {
        Weight::Error => (Icons::CIRCLE_X, theme.warning),
        Weight::Warning => (Icons::TRIANGLE_ALERT, theme.caution),
        Weight::Weak => (Icons::INFO, theme.muted),
    }
}

/// How many problems of each weight a list has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
struct Tally {
    errors: usize,
    warnings: usize,
    weak: usize,
}

fn tally(problems: &[Problem]) -> Tally {
    let count = |weight| {
        problems
            .iter()
            .filter(|problem| problem.weight == weight)
            .count()
    };
    Tally {
        errors: count(Weight::Error),
        warnings: count(Weight::Warning),
        weak: count(Weight::Weak),
    }
}

/// What the panel says when it has no problems to list: that there are none when the server has looked, and
/// otherwise why it has nothing to say yet.
fn empty_text(summary: Option<&Summary>) -> String {
    let Some(summary) = summary else {
        return "The language server does not look at this file".to_owned();
    };
    match (&summary.status, &summary.progress) {
        (Some(Status::Ready), None) => "No problems found".to_owned(),
        _ => summary
            .label()
            .unwrap_or_else(|| "The language server does not look at this file".to_owned()),
    }
}

/// The panel: a header with how many problems there are of each weight and a button that shuts it, and under
/// that the list, which scrolls. It watches the workspace for the problems of the file being edited, which
/// are found while it is open and not while it is shut.
#[derive(Clone, StatelessWidget)]
pub struct ProblemsPanel;

impl StatelessWidget for ProblemsPanel {
    fn build(&self, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let store = ProviderHandle::<Workspace>::of(ctx);
        let problems = ctx.select::<Workspace, Vec<Problem>>(|space| space.problems());
        let summary = ctx.select::<Workspace, Option<Summary>>(|space| space.lsp_summary());
        let counts = tally(&problems);

        let small = fonts::ui().font_size(12).color(theme.muted);
        let mut header: Vec<AnyWidget> = vec![
            Text::new("Problems")
                .text_style(
                    fonts::ui()
                        .font_size(12)
                        .font_weight(FontWeight::Bold)
                        .color(theme.text),
                )
                .notype(),
        ];
        for (weight, count) in [
            (Weight::Error, counts.errors),
            (Weight::Warning, counts.warnings),
            (Weight::Weak, counts.weak),
        ] {
            if count > 0 {
                let (glyph, color) = mark_of(weight, &theme);
                header.push(
                    Row::new()
                        .vertical_alignment(BoxAlignment::Center)
                        .gaps(LayoutSpacing::all(4))
                        .children([
                            icon(glyph, 13, color).boxed(),
                            Text::new(count.to_string())
                                .text_style(small.clone())
                                .boxed(),
                        ])
                        .notype(),
                );
            }
        }
        header.push(Expanded::new().child(ZeroSizedBox).notype());
        header.push(close_button(store, theme).notype());

        let list: AnyWidget = if problems.is_empty() {
            Container::new()
                .padding(pad(12, 14, 0, 14))
                .child(
                    Text::new(empty_text(summary.as_ref()))
                        .text_style(fonts::ui().font_size(13).color(theme.muted)),
                )
                .notype()
        } else {
            let rows: Vec<AnyWidget> = problems
                .into_iter()
                .map(|problem| ProblemRow { problem }.notype())
                .collect();
            Scrollable::new()
                .vertical_scroll_bar(None)
                .child(
                    Container::new()
                        .padding(pad(2, 0, 8, 0))
                        .child(Column::new().children(rows)),
                )
                .notype()
        };

        Container::new().color(theme.editor).child(
            Column::new().children([
                rule::horizontal(theme.divider).boxed(),
                SizedBox::new()
                    .width(Dimension::Percent(100.0))
                    .height(HEADER_HEIGHT)
                    .box_child(
                        Container::new()
                            .padding(pad(0, 8, 0, 14))
                            .child(
                                Row::new()
                                    .vertical_alignment(BoxAlignment::Center)
                                    .gaps(LayoutSpacing::all(12))
                                    .children(header),
                            ),
                    )
                    .boxed(),
                rule::horizontal(theme.divider).boxed(),
                Expanded::new().child(list).boxed(),
            ]),
        )
    }
}

/// The button at the end of the header that shuts the panel.
fn close_button(store: ProviderHandle<Workspace>, theme: CodeTheme) -> impl Widget {
    Button::new()
        .no_decor()
        .on_press(move || store.update(|space| space.close_problems()))
        .build(move |state| {
            Container::new()
                .margin(LayoutSpacing::vertical(2))
                .padding(pad(4, 5, 4, 5))
                .decoration([
                    BorderRadius::all(5).notype(),
                    if state.is_hovered {
                        theme.hover
                    } else {
                        Color::Transparent
                    }
                    .notype(),
                ])
                .child(Center::new().child(icon(
                    Icons::X,
                    18,
                    if state.is_hovered {
                        theme.text
                    } else {
                        theme.muted
                    },
                )))
        })
}

/// One problem of the list: its weight, what it says, its code and where it is. A click takes the caret to it.
#[derive(Clone, StatelessWidget)]
struct ProblemRow {
    problem: Problem,
}

impl StatelessWidget for ProblemRow {
    fn build(&self, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let store = ProviderHandle::<Workspace>::of(ctx);
        let problem = self.problem.clone();
        Button::new()
            .no_decor()
            .on_press({
                let at = problem.at;
                move || {
                    // The document is read out of the workspace and gone to outside any update of it: going
                    // there is not a change of the workspace.
                    let document = store.read().active_document().cloned();
                    if let Some(document) = document {
                        document.go_to(at);
                    }
                }
            })
            .build(move |state| {
                let (glyph, color) = mark_of(problem.weight, &theme);
                let background = if state.is_hovered {
                    theme.hover
                } else {
                    Color::Transparent
                };
                let small = fonts::ui().font_size(12).color(theme.muted);
                let mut cells: Vec<AnyWidget> = vec![
                    icon(glyph, 14, color).notype(),
                    Expanded::new()
                        .child(
                            Text::new(problem.headline().to_owned()).text_style(
                                fonts::ui()
                                    .font_size(13)
                                    .color(theme.text)
                                    .text_overflow(TextOverflow::Ellipsis),
                            ),
                        )
                        .notype(),
                ];
                if let Some(code) = &problem.code {
                    cells.push(
                        Text::new(code.clone())
                            .text_style(small.clone())
                            .notype(),
                    );
                }
                cells.push(
                    Text::new(format!("{}:{}", problem.line, problem.column))
                        .text_style(small)
                        .notype(),
                );
                SizedBox::new()
                    .width(Dimension::Percent(100.0))
                    .height(ROW_HEIGHT)
                    .box_child(
                        Container::new()
                            .width(Dimension::Percent(100.0))
                            .padding(pad(0, 14, 0, 14))
                            .decoration([background.notype()])
                            .child(
                                Row::new()
                                    .vertical_alignment(BoxAlignment::Center)
                                    .gaps(LayoutSpacing::all(10))
                                    .children(cells),
                            ),
                    )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problem(weight: Weight) -> Problem {
        Problem {
            weight,
            message: "m".into(),
            code: None,
            line: 1,
            column: 1,
            at: 0,
        }
    }

    fn summary(status: Option<Status>, progress: Option<&str>) -> Summary {
        Summary {
            status,
            progress: progress.map(str::to_owned),
            progress_percentage: None,
            errors: 0,
            warnings: 0,
        }
    }

    #[test]
    fn the_header_counts_the_problems_of_each_weight() {
        assert_eq!(tally(&[]), Tally::default());
        let list = [
            Weight::Error,
            Weight::Weak,
            Weight::Warning,
            Weight::Error,
            Weight::Weak,
            Weight::Weak,
        ]
        .map(problem);
        assert_eq!(
            tally(&list),
            Tally {
                errors: 2,
                warnings: 1,
                weak: 3
            }
        );
    }

    #[test]
    fn an_empty_panel_says_there_are_no_problems_only_when_the_server_has_looked() {
        assert_eq!(
            empty_text(Some(&summary(Some(Status::Ready), None))),
            "No problems found"
        );
        assert_eq!(
            empty_text(Some(&summary(Some(Status::Starting), None))),
            "Language server starting\u{2026}"
        );
        assert_eq!(
            empty_text(Some(&summary(Some(Status::Ready), Some("Indexing 40%")))),
            "Language server: Indexing 40%\u{2026}",
            "a server that is still working has not said there are none"
        );
        assert_eq!(
            empty_text(Some(&summary(
                Some(Status::Failed("it exited".into())),
                None
            ))),
            "Language server failed: it exited"
        );
        assert_eq!(
            empty_text(Some(&summary(Some(Status::Stopped), None))),
            "Language server stopped"
        );
        assert_eq!(
            empty_text(Some(&summary(None, None))),
            "The language server does not look at this file"
        );
        assert_eq!(
            empty_text(None),
            "The language server does not look at this file"
        );
    }
}
