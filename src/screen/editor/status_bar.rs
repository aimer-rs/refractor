use crate::components::icon::icon;
use crate::components::pad::pad;
use crate::components::rule;
use crate::document::Document;
use crate::fonts;
use crate::theme::CodeTheme;
use crate::workspace::Workspace;
use aimer::input::ControllerListener;
use aimer::style::{LayoutSpacing, TextOverflow, Theme};
use aimer::{
    BoxAlignment, BuildContext, Column, Container, Dimension, Expanded, Icons, Key,
    ProviderContext, Row, SizedBox, State, StateUpdater, StateView, StatefulWidget,
    StatelessWidget, Text, Widget,
};
use std::rc::Rc;

/// How tall the bar is.
const HEIGHT: u32 = 26;

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
        let small = fonts::ui()
            .font_size(12)
            .color(theme.muted);

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
            None => SizedBox::new()
                .width(1)
                .notype(),
        };
        Column::new().children([
            rule::horizontal(theme.divider).boxed(),
            SizedBox::new()
                .width(Dimension::Percent(100.0))
                .height(HEIGHT)
                .box_child(
                    Container::new()
                        .color(theme.bar)
                        .padding(pad(0, 12, 0, 12))
                        .child(
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
                )
                .boxed(),
        ])
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
        let position = widget
            .document
            .controller
            .cursor_position();
        Self {
            document: widget.document,
            position,
            listener: None,
        }
    }

    fn on_mount(&mut self, updater: StateUpdater<Self>) {
        let document = Rc::clone(&self.document);
        self.listener = Some(
            self.document
                .controller
                .listen(move |_| {
                    let position = document
                        .controller
                        .cursor_position();
                    // The line is rebuilt when the position changes, not for each change of the text.
                    updater.try_set_state(move |state| {
                        if state.position != position {
                            state.position = position;
                        }
                    });
                }),
        );
    }

    fn adopt_config_from(&mut self, _new: Self) {}

    fn build(this: StateView<Self>, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let small = fonts::ui()
            .font_size(12)
            .color(theme.muted);
        let (line, column) = this.position;
        Row::new()
            .vertical_alignment(BoxAlignment::Center)
            .gaps(LayoutSpacing::all(18))
            .children([
                Text::new(format!("Ln {line}, Col {column}"))
                    .text_style(small)
                    .boxed(),
                Text::new(format!("Spaces: {}", crate::screen::editor::TAB_SPACES))
                    .text_style(small)
                    .boxed(),
                Text::new("UTF-8")
                    .text_style(small)
                    .boxed(),
                Text::new(this.document.line_ending())
                    .text_style(small)
                    .boxed(),
                Text::new(this.document.language.label())
                    .text_style(small)
                    .boxed(),
            ])
    }
}
