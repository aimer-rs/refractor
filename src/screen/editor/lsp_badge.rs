//! The mark that floats at the top-right of the editor and says how the language server finds the file:
//! a check when it has nothing to say against it, the count of its errors and the count of its warnings, and a
//! spinner while the server is still starting or working.

use crate::components::icon::icon_stroked;
use crate::components::pad::pad;
use crate::fonts;
use super::spinner::Spinner;
use crate::lsp::{Status, Summary};
use crate::theme::CodeTheme;
use crate::workspace::Workspace;
use aimer::decoration::Decoration;
use aimer::console::cursor::{CursorIcon, set_cursor};
use aimer::input::button::Button;
use aimer::input::mouse_region::MouseRegion;
use aimer::style::{BorderRadius, FontWeight, LayoutSpacing, Theme};
use aimer::{
    AnyWidget, BoxAlignment, BuildContext, Container, Icons, ProviderContext, ProviderHandle, Row,
    SizedBox, StatelessWidget, Text, Widget, ZeroSizedBox,
};

/// What the server is doing about the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// It has nothing against the file, or has said what it has.
    Ready,
    /// The server is starting, or working.
    Busy,
    /// The server could not be run, or has gone.
    Failed,
}

/// How the mark looks for a summary: what the server is doing, and what it has found, which is not shown while it has
/// failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Look {
    state: State,
    errors: usize,
    warnings: usize,
}

/// The look of the mark for `summary`, or `None` when there is none: a server that was not asked for
/// or has been asked to stop says nothing about the file.
///
/// What is known against the file is shown while the server works: it is still true, and an error that is hidden by a
/// spinner is an error that is not seen.
fn look_of(summary: &Summary) -> Option<Look> {
    let (state, errors, warnings) = match summary.status.as_ref()? {
        Status::Stopped => return None,
        Status::Failed(_) => (State::Failed, 0, 0),
        Status::Starting => (State::Busy, 0, 0),
        Status::Ready if summary.progress.is_some() => (State::Busy, summary.errors, summary.warnings),
        Status::Ready => (State::Ready, summary.errors, summary.warnings),
    };
    Some(Look { state, errors, warnings })
}

/// A part of the mark, from left to right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Segment {
    Check,
    Failed,
    Spinner,
    Errors(usize),
    Warnings(usize),
}

impl Segment {
    /// How wide it is: its icon, and for a count the digits after it.
    fn width(self) -> u32 {
        match self {
            Self::Errors(count) | Self::Warnings(count) => ICON + COUNT_GAP + DIGIT * count.to_string().len() as u32,
            _ => ICON,
        }
    }
}

/// What the mark is made of, in order: the spinner while the server works, then the errors and the warnings that there are,
/// and where there are none and nothing is done, the check; a server that failed is the failure alone.
fn segments_of(look: &Look) -> Vec<Segment> {
    if look.state == State::Failed {
        return vec![Segment::Failed];
    }
    let mut segments = Vec::new();
    if look.state == State::Busy {
        segments.push(Segment::Spinner);
    }
    if look.errors > 0 {
        segments.push(Segment::Errors(look.errors));
    }
    if look.warnings > 0 {
        segments.push(Segment::Warnings(look.warnings));
    }
    if segments.is_empty() {
        segments.push(Segment::Check);
    }
    segments
}

/// The side of the mark, which is a square when it is a single icon.
const SIZE: u32 = 22;
/// The side of the icon in it.
const ICON: u32 = 16;
/// The room around the icon in the square, which is what makes the icon sit in the middle of it.
const INSET: u32 = (SIZE - ICON) / 2;
/// The room each digit of a count gets: a little more than it needs, for a text that has less than it needs is made smaller.
const DIGIT: u32 = 9;
/// The space between an icon and its count, and between one part of the mark and the next.
const COUNT_GAP: u32 = 5;
const PART_GAP: u32 = 9;
/// The space before the first part of a mark that has more than an icon, and after the last.
const PAD_BEFORE: u32 = 7;
const PAD_AFTER: u32 = 12;
/// How much of the color of the mark is its fill: enough to read as that color on the dark of the editor, and not so much
/// as to be a block of it.
const FILL: f32 = 0.1;
/// And while the pointer is over it, which says that it can be pressed.
const FILL_HOVERED: f32 = 0.4;
/// Where the mark floats over the pane: from its top and from its right.
pub const TOP: u32 = 16;
pub const RIGHT: u32 = 10;

/// How wide the mark is for `segments`: an icon alone is a square, and anything more is a pill that has them all.
fn width_of(segments: &[Segment]) -> u32 {
    match segments {
        [single] if single.width() == ICON => SIZE,
        _ => PAD_BEFORE + segments.iter().map(|segment| segment.width()).sum::<u32>() + PART_GAP * segments.len().saturating_sub(1) as u32 + PAD_AFTER,
    }
}

thread_local! {
    /// How wide the mark is now, if there is one. The mark sets it when it is built, so that the pane under it need not
    /// ask the workspace on every move of the pointer.
    static WIDTH: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
}

/// The part of the pane that the mark covers, where the pointer is an arrow: `(left, top, width, height)` for a pane
/// `pane_width` wide, or nothing where there is no mark.
pub fn covered(pane_width: f32) -> Vec<(f32, f32, f32, f32)> {
    match WIDTH.get() {
        Some(width) => vec![(pane_width - RIGHT as f32 - width as f32, TOP as f32, width as f32, SIZE as f32)],
        None => Vec::new(),
    }
}

/// The mark that floats over the top-right of the editor. A press on it opens the panel of problems under the
/// editor, or shuts it. It watches the workspace for what the language server says of the file being edited,
/// so a keystroke does not rebuild it.
#[derive(Clone, StatelessWidget)]
pub struct LspBadge;

impl StatelessWidget for LspBadge {
    fn build(&self, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let store = ProviderHandle::<Workspace>::of(ctx);
        let look = ctx.select::<Workspace, Option<Look>>(|space| space.lsp_summary().as_ref().and_then(look_of));
        let Some(look) = look else {
            WIDTH.set(None);
            return ZeroSizedBox.notype();
        };
        WIDTH.set(Some(width_of(&segments_of(&look))));
        // The badge floats over the text, whose field asks for the text cursor on every move of the pointer in it, unless
        // it is told that this part of it is covered: the pane is, and the badge is an arrow.
        MouseRegion::new()
            .cursor(CursorIcon::Default)
            .on_pointer_activity(|_| {
                set_cursor(CursorIcon::Default);
            })
            .box_child(
                Button::new()
                    .no_decor()
                    .on_press(move || store.update(|space| space.toggle_problems()))
                    .build(move |state| mark(look, &theme, state.is_hovered)),
            )
    }
}

/// The color of the mark: that of the worst of what it says.
fn color_of(look: &Look, theme: &CodeTheme) -> aimer::Color {
    match look {
        Look { state: State::Failed, .. } => theme.warning,
        Look { errors, .. } if *errors > 0 => theme.warning,
        Look { warnings, .. } if *warnings > 0 => theme.caution,
        Look { state: State::Busy, .. } => theme.muted,
        Look { .. } => theme.success,
    }
}

/// The mark itself, a little brighter when the pointer is over it.
fn mark(look: Look, theme: &CodeTheme, hovered: bool) -> AnyWidget {
    let color = color_of(&look, theme);
    let segments = segments_of(&look);
    let width = width_of(&segments);
    let fill = [BorderRadius::all(7).notype(), color.with_alpha(if hovered { FILL_HOVERED } else { FILL }).notype()];
    let parts: Vec<AnyWidget> = segments.iter().map(|segment| part(*segment, theme)).collect();
    let inside: AnyWidget = if parts.len() == 1 && width == SIZE {
        Container::new().padding(pad(INSET, INSET, INSET, INSET)).decoration(fill).child(parts.into_iter().next().expect("one part")).notype()
    } else {
        Container::new()
            .padding(pad(INSET, PAD_AFTER - 3, INSET, PAD_BEFORE))
            .decoration(fill)
            .child(Row::new().vertical_alignment(BoxAlignment::Center).gaps(LayoutSpacing::all(PART_GAP)).children(parts))
            .notype()
    };
    // The box is what keeps the mark its size: a container fills what it is given.
    SizedBox::new().width(width).height(SIZE).box_child(inside).notype()
}

/// One part of the mark.
fn part(segment: Segment, theme: &CodeTheme) -> AnyWidget {
    let counted = |glyph, color, count: usize| {
        Row::new()
            .vertical_alignment(BoxAlignment::Center)
            .gaps(LayoutSpacing::all(COUNT_GAP))
            .children([
                icon_stroked(glyph, ICON, color, 2.0).boxed(),
                Text::new(count.to_string())
                    .text_style(fonts::ui().font_size(13).font_weight(FontWeight::Bold).color(color))
                    .boxed(),
            ])
            .notype()
    };
    match segment {
        Segment::Check => icon_stroked(Icons::CHECK, ICON, theme.success, 2.0).notype(),
        Segment::Failed => icon_stroked(Icons::CIRCLE_ALERT, ICON, theme.warning, 2.0).notype(),
        Segment::Spinner => Spinner::new(ICON).notype(),
        Segment::Errors(count) => counted(Icons::CIRCLE_X, theme.warning, count),
        Segment::Warnings(count) => counted(Icons::TRIANGLE_ALERT, theme.caution, count),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(status: Option<Status>, progress: Option<&str>, errors: usize, warnings: usize) -> Summary {
        Summary { status, progress: progress.map(str::to_owned), errors, warnings }
    }

    fn look(status: Option<Status>, progress: Option<&str>, errors: usize, warnings: usize) -> Option<Look> {
        look_of(&summary(status, progress, errors, warnings))
    }

    fn of(state: State, errors: usize, warnings: usize) -> Option<Look> {
        Some(Look { state, errors, warnings })
    }

    #[test]
    fn a_file_the_server_has_nothing_against_is_ready_and_has_no_counts() {
        assert_eq!(look(Some(Status::Ready), None, 0, 0), of(State::Ready, 0, 0));
    }

    #[test]
    fn the_errors_and_the_warnings_are_both_counted() {
        assert_eq!(look(Some(Status::Ready), None, 3, 0), of(State::Ready, 3, 0));
        assert_eq!(look(Some(Status::Ready), None, 0, 2), of(State::Ready, 0, 2));
        assert_eq!(look(Some(Status::Ready), None, 1, 9), of(State::Ready, 1, 9), "the warnings do not wait for the errors to be mended");
    }

    #[test]
    fn a_server_that_is_starting_or_working_is_busy_and_still_says_what_it_has_found() {
        assert_eq!(look(Some(Status::Starting), None, 0, 0), of(State::Busy, 0, 0));
        assert_eq!(look(Some(Status::Ready), Some("Indexing 40%"), 0, 0), of(State::Busy, 0, 0));
        assert_eq!(look(Some(Status::Ready), Some("Working"), 2, 1), of(State::Busy, 2, 1), "an error is not hidden by a spinner");
    }

    #[test]
    fn a_server_that_failed_is_a_failure_whatever_it_had_found() {
        assert_eq!(look(Some(Status::Failed("it exited".into())), None, 0, 0), of(State::Failed, 0, 0));
        assert_eq!(look(Some(Status::Failed("it exited".into())), Some("Working"), 4, 4), of(State::Failed, 0, 0));
    }

    #[test]
    fn a_server_that_was_not_asked_for_or_has_stopped_has_no_mark() {
        assert_eq!(look(None, None, 0, 0), None);
        assert_eq!(look(Some(Status::Stopped), None, 5, 5), None);
    }

    fn segments(state: State, errors: usize, warnings: usize) -> Vec<Segment> {
        segments_of(&Look { state, errors, warnings })
    }

    #[test]
    fn the_mark_is_made_of_the_spinner_while_busy_then_the_errors_then_the_warnings_and_a_check_where_there_is_nothing() {
        assert_eq!(segments(State::Ready, 0, 0), [Segment::Check]);
        assert_eq!(segments(State::Busy, 0, 0), [Segment::Spinner], "and not a check while it works");
        assert_eq!(segments(State::Ready, 2, 0), [Segment::Errors(2)]);
        assert_eq!(segments(State::Ready, 0, 5), [Segment::Warnings(5)]);
        assert_eq!(segments(State::Ready, 2, 5), [Segment::Errors(2), Segment::Warnings(5)], "errors come first");
        assert_eq!(segments(State::Busy, 1, 1), [Segment::Spinner, Segment::Errors(1), Segment::Warnings(1)]);
        assert_eq!(segments(State::Failed, 3, 3), [Segment::Failed], "a server that failed has found nothing to count");
    }

    #[test]
    fn an_icon_alone_is_a_square_and_anything_more_is_a_pill_as_wide_as_what_is_in_it() {
        assert_eq!(width_of(&[Segment::Check]), SIZE);
        assert_eq!(width_of(&[Segment::Spinner]), SIZE);
        let one = width_of(&[Segment::Errors(3)]);
        assert_eq!(one, PAD_BEFORE + ICON + COUNT_GAP + DIGIT + PAD_AFTER);
        let both = width_of(&[Segment::Errors(3), Segment::Warnings(12)]);
        assert_eq!(both, PAD_BEFORE + (ICON + COUNT_GAP + DIGIT) + PART_GAP + (ICON + COUNT_GAP + 2 * DIGIT) + PAD_AFTER);
        assert!(width_of(&[Segment::Errors(100)]) > one, "more digits are wider");
        assert!(width_of(&[Segment::Spinner, Segment::Errors(1)]) > width_of(&[Segment::Errors(1)]));
    }

    #[test]
    fn the_icon_has_the_same_room_on_every_side_of_the_square_it_is_in() {
        // The square is the icon and its room: whatever the two are made, nothing of the icon is outside the square, and it
        // is in the middle of it.
        assert!(SIZE >= ICON, "a {ICON} icon does not fit in a {SIZE} square");
        assert_eq!(SIZE - ICON, 2 * INSET, "{SIZE} is {ICON} and two rooms of {INSET}: one side would have more than the other");
        assert_eq!(INSET, (SIZE - ICON) / 2);
        // The room is the same above and below the icons of a pill, which is as tall as the square.
        assert_eq!(SIZE, INSET + ICON + INSET);
    }

    #[test]
    fn the_part_of_the_pane_the_mark_covers_is_at_its_top_right_corner_and_as_wide_as_the_mark() {
        let width = width_of(&[Segment::Errors(2), Segment::Warnings(3)]);
        WIDTH.set(Some(width));
        assert_eq!(covered(800.0), vec![(800.0 - RIGHT as f32 - width as f32, TOP as f32, width as f32, SIZE as f32)]);
        // The same mark is further left in a narrower pane, and a square when it is a check.
        WIDTH.set(Some(SIZE));
        assert_eq!(covered(300.0), vec![(300.0 - RIGHT as f32 - SIZE as f32, TOP as f32, SIZE as f32, SIZE as f32)]);
        // No mark, no part.
        WIDTH.set(None);
        assert!(covered(800.0).is_empty());
    }

    #[test]
    fn the_color_of_the_mark_is_that_of_the_worst_of_what_it_says() {
        let theme = CodeTheme::dark();
        let color = |state, errors, warnings| color_of(&Look { state, errors, warnings }, &theme);
        assert_eq!(color(State::Ready, 1, 1), theme.warning, "errors over warnings");
        assert_eq!(color(State::Ready, 0, 1), theme.caution);
        assert_eq!(color(State::Busy, 0, 0), theme.muted);
        assert_eq!(color(State::Ready, 0, 0), theme.success);
        assert_eq!(color(State::Failed, 0, 0), theme.warning);
        assert_eq!(color(State::Busy, 0, 2), theme.caution, "what is found is the color while the server works");
    }
}
