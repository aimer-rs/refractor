use super::progress_bar::MacProgressBar;
use crate::ui::components::icon::{icon, icon_stroked};
use crate::ui::components::pad::pad;
use crate::ui::components::rule;
use crate::model::document::Document;
use crate::ui::fonts;
use crate::services::lsp::{Status, Summary};
use crate::ui::theme::CodeTheme;
use crate::model::workspace::Workspace;
use aimer::input::ControllerListener;
use aimer::style::{LayoutSpacing, TextOverflow, Theme};
use aimer::{
    BoxAlignment, BuildContext, Color, Column, Container, Dimension, Expanded, Icons, Key,
    ProviderContext, Row, SizedBox, State, StateUpdater, StateView, StatefulWidget,
    StatelessWidget, Text, Venus, Widget, ZeroSizedBox,
};
use std::rc::Rc;
use std::time::Duration;

/// How tall the bar is.
const HEIGHT: u32 = 26;
/// Delay before showing the progress bar for transient ready->working bursts (e.g. instant check on save).
const BUSY_DEBOUNCE: Duration = Duration::from_millis(250);

/// What the status bar shows for the language server.
#[derive(Clone, Debug, PartialEq)]
enum LspIndicator {
    /// No server is active or it has stopped.
    None,
    /// Server is starting up: shown immediately as an indeterminate progress bar.
    Starting,
    /// Server is indexing or checking: debounced briefly when transitioning from `Ready`.
    Working(Option<f32>),
    /// Ready and idle: shown as a light-pink sparkles icon.
    Ready,
    /// Failed to start or exited unexpectedly.
    Failed(String),
}

fn lsp_indicator(summary: Option<&Summary>) -> LspIndicator {
    let Some(summary) = summary else {
        return LspIndicator::None;
    };
    match summary.status.as_ref() {
        None | Some(Status::Stopped) => LspIndicator::None,
        Some(Status::Starting) => LspIndicator::Starting,
        Some(Status::Ready) if summary.progress.is_some() => LspIndicator::Working(
            summary
                .progress_percentage
                .filter(|&pct| pct > 0)
                .map(|pct| (pct as f32 / 100.0).clamp(0.0, 1.0)),
        ),
        Some(Status::Ready) => LspIndicator::Ready,
        Some(Status::Failed(why)) => LspIndicator::Failed(format!("Language server failed: {why}")),
    }
}

/// Returns `true` when transitioning from `displayed` to `next` should wait [`BUSY_DEBOUNCE`]
/// so brief checks (like an instant check on save) do not flicker the status bar.
fn should_debounce_busy(displayed: &LspIndicator, next: &LspIndicator) -> bool {
    matches!(displayed, LspIndicator::Ready) && matches!(next, LspIndicator::Working(_))
}

/// Soft light pink for the ready sparkles icon, tuned for dark and light status bars.
fn light_pink(theme: &CodeTheme) -> Color {
    let (r, _, _, _) = theme.bar.to_rgba();
    if r < 128 {
        Color::Rgb(255, 156, 196)
    } else {
        Color::Rgb(236, 102, 158)
    }
}

/// The bar along the bottom of the window: the branch and what went wrong, if anything, on the
/// left, and what is known of the file being edited on the right.
#[derive(Clone, StatelessWidget)]
pub struct StatusBar;

impl StatelessWidget for StatusBar {
    fn build(&self, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let document =
            ctx.select::<Workspace, Option<Rc<Document>>>(|space| space.active_document().cloned());
        let branch = ctx.select::<Workspace, Option<(String, bool)>>(|space| {
            space
                .branch
                .clone()
                .map(|branch| (branch, space.git_dirty()))
        });
        let message = ctx.select::<Workspace, Option<String>>(|space| space.message.clone());
        let server =
            ctx.select::<Workspace, Option<crate::services::lsp::Summary>>(|space| space.lsp_summary());
        let small = fonts::ui().font_size(12).color(theme.muted);

        let mut left = Vec::new();
        if let Some((name, dirty)) = branch {
            left.push(icon(Icons::GIT_BRANCH, 13, theme.muted).boxed());
            // A `*` after the name says something in the repository is changed.
            left.push(
                Text::new(if dirty { format!("{name}*") } else { name })
                    .text_style(small)
                    .boxed(),
            );
        }
        let indicator = lsp_indicator(server.as_ref());
        if indicator != LspIndicator::None {
            left.push(LspStatusSlot::new(indicator).boxed());
        }
        if let Some(message) = message {
            left.push(
                Text::new(message)
                    .text_style(
                        fonts::ui()
                            .font_size(12)
                            .color(theme.warning)
                            .text_overflow(TextOverflow::Ellipsis),
                    )
                    .boxed(),
            );
        }
        let right: aimer::AnyWidget = match document {
            Some(document) => CaretStatus::new(document).notype(),
            None => SizedBox::new().width(1).notype(),
        };
        Column::new().children([
            rule::horizontal(theme.divider).boxed(),
            Container::new()
                .width(Dimension::Percent(100.0))
                .height(HEIGHT)
                .color(theme.bar)
                .padding(pad(0, 12, 0, 12))
                .box_child(
                    Row::new()
                        .vertical_alignment(BoxAlignment::Center)
                        .gaps(LayoutSpacing::all(18))
                        .children([
                            Expanded::new()
                                .child(
                                    Row::new()
                                        .vertical_alignment(BoxAlignment::Center)
                                        .gaps(LayoutSpacing::all(6))
                                        .children(left),
                                )
                                .boxed(),
                            right.boxed(),
                        ]),
                ),
        ])
    }
}

/// Debounces short ready -> working bursts so quick LSP checks (e.g. instant check on save)
/// do not flicker the sparkles icon into a progress bar and back.
#[derive(Clone, StatefulWidget)]
struct LspStatusSlot {
    key: Option<Key>,
    indicator: LspIndicator,
}

impl LspStatusSlot {
    fn new(indicator: LspIndicator) -> Self {
        Self {
            key: Some(Key::from("status-bar-lsp-slot")),
            indicator,
        }
    }
}

struct LspStatusSlotState {
    actual: LspIndicator,
    displayed: LspIndicator,
    debounce_gen: u64,
    updater: Option<StateUpdater<Self>>,
}

impl LspStatusSlotState {
    fn schedule_debounce(&mut self) {
        self.debounce_gen += 1;
        let ticket = self.debounce_gen;
        let Some(updater) = self.updater.clone() else {
            return;
        };
        let Some(venus) = Venus::current() else {
            return;
        };
        let sleeper = venus.clone();
        venus.spawn(async move {
            let _ = sleeper.sleep(BUSY_DEBOUNCE).await;
            updater.try_set_state(move |state| {
                if state.debounce_gen == ticket && matches!(state.actual, LspIndicator::Working(_))
                {
                    state.displayed = state.actual.clone();
                }
            });
            aimer::events::window::request_animation_frame();
        });
    }
}

impl State<LspStatusSlot> for LspStatusSlotState {
    fn create(widget: LspStatusSlot) -> Self {
        Self {
            actual: widget.indicator.clone(),
            displayed: widget.indicator,
            debounce_gen: 0,
            updater: None,
        }
    }

    fn on_mount(&mut self, updater: StateUpdater<Self>) {
        self.updater = Some(updater);
    }

    fn adopt_config_from(&mut self, new: Self) {
        if self.actual == new.actual {
            return;
        }
        self.actual = new.actual.clone();
        if should_debounce_busy(&self.displayed, &new.actual) {
            self.schedule_debounce();
        } else {
            self.debounce_gen += 1;
            self.displayed = new.actual;
        }
    }

    fn build(this: StateView<Self>, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        match &this.displayed {
            LspIndicator::None => ZeroSizedBox.notype(),
            LspIndicator::Starting => MacProgressBar::new(None).notype(),
            LspIndicator::Working(fraction) => MacProgressBar::new(*fraction).notype(),
            LspIndicator::Ready => {
                icon_stroked(Icons::PACKAGE_CHECK, 12, light_pink(&theme), 1.75).notype()
            }
            LspIndicator::Failed(why) => Text::new(why.clone())
                .text_style(
                    fonts::ui()
                        .font_size(12)
                        .color(theme.warning)
                        .text_overflow(TextOverflow::Ellipsis),
                )
                .notype(),
        }
    }
}

/// What the status bar says of the file being edited: the caret's line and column, the indent, the
/// encoding, the line endings and the language. It follows the document's controller itself, so a
/// keystroke rebuilds this line and not the bar. Keyed by the document.
#[derive(Clone, StatefulWidget)]
struct CaretStatus {
    key: Option<Key>,
    document: Rc<Document>,
}

impl CaretStatus {
    fn new(document: Rc<Document>) -> Self {
        Self {
            key: Some(Key::from(("caret-status", document.id))),
            document,
        }
    }
}

struct CaretStatusState {
    document: Rc<Document>,
    /// The caret's line and column, counted from one.
    position: (usize, usize),
    /// Keeps the status following the controller for as long as the bar is on screen.
    listener: Option<ControllerListener>,
}

impl State<CaretStatus> for CaretStatusState {
    fn create(widget: CaretStatus) -> Self {
        let position = widget.document.controller.cursor_position();
        Self {
            document: widget.document,
            position,
            listener: None,
        }
    }

    fn on_mount(&mut self, updater: StateUpdater<Self>) {
        let document = Rc::clone(&self.document);
        self.listener = Some(self.document.controller.listen(move |_| {
            let position = document.controller.cursor_position();
            // The line is rebuilt when the position changes, not for each change of the text.
            updater.try_set_state(move |state| {
                if state.position != position {
                    state.position = position;
                }
            });
        }));
    }

    fn adopt_config_from(&mut self, _new: Self) {}

    fn build(this: StateView<Self>, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let small = fonts::ui().font_size(12).color(theme.muted);
        let (line, column) = this.position;
        Row::new()
            .vertical_alignment(BoxAlignment::Center)
            .gaps(LayoutSpacing::all(8))
            .children([
                Text::new(format!("{line}:{column}"))
                    .text_style(small)
                    .boxed(),
                Text::new("UTF-8").text_style(small).boxed(),
                Text::new(this.document.line_ending())
                    .text_style(small)
                    .boxed(),
                Text::new(this.document.language.label())
                    .text_style(small)
                    .boxed(),
            ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(
        status: Option<Status>,
        progress: Option<&str>,
        progress_percentage: Option<u32>,
    ) -> Summary {
        Summary {
            status,
            progress: progress.map(str::to_owned),
            progress_percentage,
            errors: 0,
            warnings: 0,
        }
    }

    #[test]
    fn status_bar_shows_progress_bar_while_starting_or_working_and_sparkles_when_ready() {
        assert_eq!(lsp_indicator(None), LspIndicator::None);
        assert_eq!(
            lsp_indicator(Some(&summary(None, None, None))),
            LspIndicator::None
        );
        assert_eq!(
            lsp_indicator(Some(&summary(Some(Status::Stopped), None, None))),
            LspIndicator::None
        );
        assert_eq!(
            lsp_indicator(Some(&summary(Some(Status::Starting), None, None))),
            LspIndicator::Starting
        );
        assert_eq!(
            lsp_indicator(Some(&summary(Some(Status::Ready), Some("Working"), None))),
            LspIndicator::Working(None)
        );
        assert_eq!(
            lsp_indicator(Some(&summary(
                Some(Status::Ready),
                Some("Indexing 42%"),
                Some(42)
            ))),
            LspIndicator::Working(Some(0.42))
        );
        assert_eq!(
            lsp_indicator(Some(&summary(Some(Status::Ready), None, None))),
            LspIndicator::Ready
        );
        assert_eq!(
            lsp_indicator(Some(&summary(
                Some(Status::Failed("crashed".into())),
                None,
                None
            ))),
            LspIndicator::Failed("Language server failed: crashed".into())
        );
    }

    #[test]
    fn brief_ready_to_working_bursts_are_debounced_while_startup_and_recovery_are_immediate() {
        let mut slot = LspStatusSlotState::create(LspStatusSlot::new(LspIndicator::Ready));
        slot.adopt_config_from(LspStatusSlotState::create(LspStatusSlot::new(
            LspIndicator::Working(None),
        )));
        assert_eq!(
            slot.displayed,
            LspIndicator::Ready,
            "stays on sparkles during the debounce window for short checks"
        );
        // If LSP finishes before the debounce expires, sparkles never flickers.
        slot.adopt_config_from(LspStatusSlotState::create(LspStatusSlot::new(
            LspIndicator::Ready,
        )));
        assert_eq!(slot.displayed, LspIndicator::Ready);

        // Initial startup shows the progress bar immediately without debounce.
        let mut starting = LspStatusSlotState::create(LspStatusSlot::new(LspIndicator::None));
        starting.adopt_config_from(LspStatusSlotState::create(LspStatusSlot::new(
            LspIndicator::Starting,
        )));
        assert_eq!(starting.displayed, LspIndicator::Starting);
    }

    #[test]
    fn ready_sparkles_color_is_light_pink_in_both_themes() {
        for theme in [CodeTheme::dark(), CodeTheme::light()] {
            let (r, g, b, _) = light_pink(&theme).to_rgba();
            assert!(
                r > b && b > g,
                "light pink has red > blue > green: ({r}, {g}, {b})"
            );
        }
    }
}
