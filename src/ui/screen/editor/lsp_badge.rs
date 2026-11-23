//! The mark that floats at the top-right of the editor and says how the language server finds the file:
//! a check when it has nothing to say against it, the count of its errors and the count of its warnings, and a
//! spinner while the server is still starting or working.

use super::spinner::Spinner;
use crate::ui::components::icon::icon_stroked;
use crate::ui::components::pad::pad;
use crate::ui::fonts;
use crate::services::lsp::{Status, Summary};
use crate::ui::theme::CodeTheme;
use crate::model::workspace::{FILE_ACTIVITY_DURATION, FileActivity, FileActivityKind, Workspace};
use aimer::console::cursor::{CursorIcon, set_cursor};
use aimer::console::info;
use aimer::decoration::BoxDecoration;
use aimer::input::button::Button;
use aimer::input::mouse_region::MouseRegion;
use aimer::style::{BorderSlice, BorderStyle, BoxBorder, FontWeight, LayoutSpacing, Theme};
use aimer::{
    AnyWidget, BoxAlignment, BuildContext, Container, Icons, Key, Opacity, Positioned,
    ProviderContext, ProviderHandle, Row, SizedBox, Stack, State as WidgetState, StateUpdater,
    StateView, StatefulWidget, StatelessWidget, StaticSvg, Text, Venus, Widget, ZeroSizedBox,
};
use std::cell::Cell;
use std::f32::consts::TAU;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// What the server is doing about the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// It has nothing against the file, or has said what it has.
    Ready,
    /// The server is starting up (shown immediately).
    Starting,
    /// The server is working after being ready (debounced briefly for instant checks).
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
        Status::Starting => (State::Starting, 0, 0),
        Status::Ready if summary.progress.is_some() => {
            (State::Busy, summary.errors, summary.warnings)
        }
        Status::Ready => (State::Ready, summary.errors, summary.warnings),
    };
    Some(Look {
        state,
        errors,
        warnings,
    })
}

/// Which kind of segment is in the badge, used to detect newly shown items for enter animations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SegmentKind {
    Activity,
    Check,
    Failed,
    Spinner,
    Errors,
    Warnings,
}

/// A part of the mark, from left to right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Segment {
    Activity(FileActivity),
    Check,
    Failed,
    Spinner,
    Errors(usize),
    Warnings(usize),
}

impl Segment {
    fn kind(self) -> SegmentKind {
        match self {
            Self::Activity(_) => SegmentKind::Activity,
            Self::Check => SegmentKind::Check,
            Self::Failed => SegmentKind::Failed,
            Self::Spinner => SegmentKind::Spinner,
            Self::Errors(_) => SegmentKind::Errors,
            Self::Warnings(_) => SegmentKind::Warnings,
        }
    }

    /// Priority rank used to sort stacked cards from most important (front/top) to least important.
    fn importance(self) -> u8 {
        match self {
            Self::Failed => 5,
            Self::Errors(_) => 4,
            Self::Warnings(_) => 3,
            Self::Activity(_) => 2,
            Self::Spinner => 1,
            Self::Check => 0,
        }
    }

    /// How wide the content inside this card is: its icon, and for a count the digits after it.
    fn content_width(self) -> u32 {
        match self {
            Self::Errors(count) | Self::Warnings(count) => {
                ICON + COUNT_GAP + DIGIT * count.to_string().len() as u32
            }
            _ => ICON,
        }
    }

    /// Total width of this item's individual card: square for an icon alone, pill for a counted card.
    fn card_width(self) -> u32 {
        let content = self.content_width();
        if content == ICON {
            SIZE
        } else {
            CARD_PAD_H + content + CARD_PAD_H + CARD_BORDER_EXTRA
        }
    }

    /// Semantic accent color of this card.
    fn color(self, theme: &CodeTheme) -> aimer::Color {
        match self {
            Self::Failed | Self::Errors(_) => theme.warning,
            Self::Warnings(_) => theme.caution,
            Self::Activity(_) => theme.marker,
            Self::Spinner => theme.muted,
            Self::Check => theme.success,
        }
    }
}

/// Active cards sorted from most important (front/top of stack) to least important (back of stack).
fn segments_of(look: Option<&Look>, activity: Option<FileActivity>) -> Vec<Segment> {
    let mut segments = Vec::new();
    if let Some(activity) = activity {
        segments.push(Segment::Activity(activity));
    }
    if let Some(look) = look {
        if look.state == State::Failed {
            segments.push(Segment::Failed);
        } else {
            if matches!(look.state, State::Starting | State::Busy) {
                segments.push(Segment::Spinner);
            }
            if look.errors > 0 {
                segments.push(Segment::Errors(look.errors));
            }
            if look.warnings > 0 {
                segments.push(Segment::Warnings(look.warnings));
            }
            if look.state == State::Ready && look.errors == 0 && look.warnings == 0 {
                segments.push(Segment::Check);
            }
        }
    }
    segments.sort_by(|a, b| b.importance().cmp(&a.importance()));
    segments
}

/// The side of a card, which is a square when it is a single icon.
const SIZE: u32 = 22;
/// The side of the icon in it.
const ICON: u32 = 16;
/// The room around the icon in the square, which is what makes the icon sit in the middle of it.
const INSET: u32 = (SIZE - ICON) / 2;
/// Generous width per bold digit so counts never clip against the card border.
const DIGIT: u32 = 11;
/// The space between an icon and its count.
const COUNT_GAP: u32 = 5;
/// Horizontal padding inside a counted card.
const CARD_PAD_H: u32 = 8;
/// Extra horizontal allowance for the 1px border on both sides and bold font side-bearings.
const CARD_BORDER_EXTRA: u32 = 8;
/// Horizontal gap (logical px) between cards in the row.
const CARD_GAP: u32 = 4;
/// How much of the card's accent color is blended into the editor background for the card surface.
const CARD_TINT: f32 = 0.26;
const CARD_TINT_HOVERED: f32 = 0.48;
/// Border opacity for each card.
const CARD_BORDER: f32 = 0.30;
const CARD_BORDER_HOVERED: f32 = 0.55;
/// Where the card row floats over the pane: from its top and from its right.
pub const TOP: u32 = 16;
pub const RIGHT: u32 = 10;

/// Total logical width of the horizontal row of `cards` separated by [`CARD_GAP`].
#[cfg(test)]
fn row_width(cards: &[Segment]) -> u32 {
    if cards.is_empty() {
        return 0;
    }
    cards
        .iter()
        .map(|s| s.card_width())
        .sum::<u32>()
        + CARD_GAP * cards.len().saturating_sub(1) as u32
}

thread_local! {
    /// Current `(width, height)` of the card row, if visible. Set on build so the pane under it
    /// keeps the default pointer cursor over the entire row.
    static BOUNDS: Cell<Option<(u32, u32)>> = const { Cell::new(None) };
}

/// The part of the pane that the badge covers, where the pointer is an arrow: `(left, top, width, height)`
/// for a pane `pane_width` wide, or nothing where there is no badge.
pub fn covered(pane_width: f32) -> Vec<(f32, f32, f32, f32)> {
    match BOUNDS.get() {
        Some((width, height)) => vec![(
            pane_width - RIGHT as f32 - width as f32,
            TOP as f32,
            width as f32,
            height as f32,
        )],
        None => Vec::new(),
    }
}

/// Frame interval for badge move and enter/exit transitions (~120 fps while animating).
const TRANSITION_FRAME: Duration = Duration::from_millis(8);
/// Duration of item move, enter, and exit animations (at least 400ms).
const ITEM_ANIM_DURATION: Duration = Duration::from_millis(420);
/// Delay before showing the spinner card on transient Ready -> Busy checks (e.g. instant check on save).
const BUSY_DEBOUNCE: Duration = Duration::from_millis(650);
/// Delay before dropping error/warning counts on the same document so short edits or checks do not flicker.
const COUNT_DEBOUNCE: Duration = Duration::from_millis(350);
/// Horizontal move distance (logical px) for entering and exiting cards.
const MOVE_OFFSET: f32 = -14.0;

fn ease_out_cubic(t: f32) -> f32 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

/// Target right-edge offsets for `segments` (sorted most important / rightmost first).
fn target_offsets(segments: &[Segment]) -> Vec<(Segment, f32)> {
    let mut right = 0.0_f32;
    let mut out = Vec::with_capacity(segments.len());
    for &segment in segments {
        out.push((segment, right));
        right += segment.card_width() as f32 + CARD_GAP as f32;
    }
    out
}

/// One animated card slot tracked by its [`SegmentKind`] so entering, exiting, and shifting
/// cards smoothly move between right-edge offsets and fade in/out.
#[derive(Clone, Debug)]
struct AnimatedSlot {
    segment: Segment,
    /// Current distance from the right edge of the badge row (logical px).
    right: f32,
    from_right: f32,
    target_right: f32,
    /// Current opacity in `0.0..=1.0`.
    alpha: f32,
    from_alpha: f32,
    target_alpha: f32,
    /// When the current move/fade transition began (`None` when settled).
    started: Option<Instant>,
    /// `true` while fading and sliding out after leaving the active segment list.
    exiting: bool,
}

impl AnimatedSlot {
    fn settled(segment: Segment, right: f32) -> Self {
        Self {
            segment,
            right,
            from_right: right,
            target_right: right,
            alpha: 1.0,
            from_alpha: 1.0,
            target_alpha: 1.0,
            started: None,
            exiting: false,
        }
    }

    fn entering(segment: Segment, target_right: f32, now: Instant) -> Self {
        // Cards entering to the left of an existing card slide outward from behind it;
        // a card entering at the right edge slides inward into place.
        let from_right = if target_right > 0.0 {
            (target_right - MOVE_OFFSET).max(0.0)
        } else {
            MOVE_OFFSET
        };
        Self {
            segment,
            right: from_right,
            from_right,
            target_right,
            alpha: 0.0,
            from_alpha: 0.0,
            target_alpha: 1.0,
            started: Some(now),
            exiting: false,
        }
    }
}

/// The card row that floats over the top-right of the editor. A press opens or closes the
/// problems panel. Active items are shown side-by-side in a row sorted by importance.
#[derive(Clone, StatelessWidget)]
pub struct LspBadge;

impl StatelessWidget for LspBadge {
    fn build(&self, ctx: &BuildContext) -> impl Widget {
        let (doc_id, look, activity) = ctx
            .select::<Workspace, (Option<u64>, Option<Look>, Option<FileActivity>)>(|space| {
                (
                    space.active_document().map(|doc| doc.id),
                    space.lsp_summary().as_ref().and_then(look_of),
                    space.file_activity(),
                )
            });
        AnimatedMark::new(doc_id, look, activity)
    }
}

/// Stateful host for the badge row that smoothly animates card move, enter, and exit transitions
/// for at least 400ms and debounces transient busy/count drops during short actions.
#[derive(Clone, StatefulWidget)]
struct AnimatedMark {
    key: Option<Key>,
    doc_id: Option<u64>,
    look: Option<Look>,
    activity: Option<FileActivity>,
}

impl AnimatedMark {
    fn new(doc_id: Option<u64>, look: Option<Look>, activity: Option<FileActivity>) -> Self {
        Self {
            key: Some(Key::from("lsp-badge-mark")),
            doc_id,
            look,
            activity,
        }
    }
}

struct AnimatedMarkState {
    doc_id: Option<u64>,
    look: Option<Look>,
    activity: Option<FileActivity>,
    rendered_look: Option<Look>,
    rendered_activity: Option<FileActivity>,
    busy_debounce_gen: u64,
    busy_debounce_pending: bool,
    count_debounce_gen: u64,
    count_debounce_pending: bool,
    slots: Vec<AnimatedSlot>,
    updater: Option<StateUpdater<Self>>,
    ticking: bool,
}

impl AnimatedMarkState {
    fn effective_look(&self) -> Option<Look> {
        let mut look = self.look?;
        if self.busy_debounce_pending && look.state == State::Busy {
            look.state = State::Ready;
        }
        if self.count_debounce_pending
            && matches!(look.state, State::Ready | State::Busy)
            && let Some(rendered) = self.rendered_look
            && matches!(rendered.state, State::Ready | State::Busy)
        {
            look.errors = rendered.errors.max(look.errors);
            look.warnings = rendered.warnings.max(look.warnings);
        }
        Some(look)
    }

    fn schedule_busy_debounce(&mut self) {
        self.busy_debounce_gen += 1;
        self.busy_debounce_pending = true;
        let ticket = self.busy_debounce_gen;
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
                state.finish_busy_debounce(ticket, Instant::now());
            });
            aimer::events::window::request_animation_frame();
        });
    }

    fn finish_busy_debounce(&mut self, ticket: u64, now: Instant) {
        if self.busy_debounce_gen == ticket && self.busy_debounce_pending {
            self.busy_debounce_pending = false;
            let effective = self.effective_look();
            self.apply_effective(effective, self.activity, now);
        }
    }

    fn schedule_count_debounce(&mut self) {
        if self.count_debounce_pending {
            return;
        }
        self.count_debounce_gen += 1;
        self.count_debounce_pending = true;
        let ticket = self.count_debounce_gen;
        let Some(updater) = self.updater.clone() else {
            return;
        };
        let Some(venus) = Venus::current() else {
            return;
        };
        let sleeper = venus.clone();
        venus.spawn(async move {
            let _ = sleeper.sleep(COUNT_DEBOUNCE).await;
            updater.try_set_state(move |state| {
                state.finish_count_debounce(ticket, Instant::now());
            });
            aimer::events::window::request_animation_frame();
        });
    }

    fn finish_count_debounce(&mut self, ticket: u64, now: Instant) {
        if self.count_debounce_gen == ticket && self.count_debounce_pending {
            self.count_debounce_pending = false;
            let effective = self.effective_look();
            self.apply_effective(effective, self.activity, now);
        }
    }

    fn apply_effective(
        &mut self,
        effective_look: Option<Look>,
        activity: Option<FileActivity>,
        now: Instant,
    ) {
        if self.rendered_look == effective_look && self.rendered_activity == activity {
            return;
        }
        self.rendered_look = effective_look;
        self.rendered_activity = activity;
        let next_segments = segments_of(effective_look.as_ref(), activity);
        self.sync_slots(&next_segments, now);
        self.ensure_ticking();
    }

    fn sync_slots(&mut self, next_segments: &[Segment], now: Instant) {
        self.advance_slots(now);
        let targets = target_offsets(next_segments);
        let mut active_kinds = Vec::with_capacity(targets.len());

        for &(segment, target_right) in &targets {
            let kind = segment.kind();
            active_kinds.push(kind);
            if let Some(slot) = self
                .slots
                .iter_mut()
                .find(|s| s.segment.kind() == kind)
            {
                slot.segment = segment;
                let moved = (slot.target_right - target_right).abs() > 0.05;
                let faded = slot.exiting || (slot.target_alpha - 1.0).abs() > 0.005;
                if moved || faded {
                    slot.exiting = false;
                    slot.from_right = slot.right;
                    slot.target_right = target_right;
                    slot.from_alpha = slot.alpha;
                    slot.target_alpha = 1.0;
                    slot.started = Some(now);
                }
            } else {
                self.slots
                    .push(AnimatedSlot::entering(segment, target_right, now));
            }
        }

        let all_exiting = targets.is_empty();
        for slot in &mut self.slots {
            if !active_kinds.contains(&slot.segment.kind()) && !slot.exiting {
                slot.exiting = true;
                slot.from_right = slot.right;
                slot.from_alpha = slot.alpha;
                slot.target_alpha = 0.0;
                // Cards to the left tuck back toward the right edge; a rightmost card stays under
                // incoming cards, or drifts gently left when the whole badge is hiding.
                slot.target_right = if slot.right > 0.0 {
                    (slot.right - MOVE_OFFSET).max(0.0)
                } else if all_exiting {
                    MOVE_OFFSET
                } else {
                    0.0
                };
                slot.started = Some(now);
            }
        }
    }

    fn advance_slots(&mut self, now: Instant) {
        let duration = ITEM_ANIM_DURATION.as_secs_f32();
        for slot in &mut self.slots {
            if let Some(started) = slot.started {
                let t = (now
                    .saturating_duration_since(started)
                    .as_secs_f32()
                    / duration)
                    .clamp(0.0, 1.0);
                let eased = ease_out_cubic(t);
                slot.right = slot.from_right + (slot.target_right - slot.from_right) * eased;
                slot.alpha = slot.from_alpha + (slot.target_alpha - slot.from_alpha) * eased;
                if t >= 1.0 {
                    slot.right = slot.target_right;
                    slot.alpha = slot.target_alpha;
                    slot.started = None;
                }
            }
        }
        self.slots
            .retain(|slot| !(slot.exiting && slot.started.is_none() && slot.alpha <= 0.005));
    }

    fn needs_animation(&self) -> bool {
        self.slots
            .iter()
            .any(|slot| slot.started.is_some())
    }

    fn ensure_ticking(&mut self) {
        if self.ticking || !self.needs_animation() {
            return;
        }
        let Some(updater) = self.updater.clone() else {
            return;
        };
        let Some(venus) = Venus::current() else {
            return;
        };
        self.ticking = true;
        let sleeper = venus.clone();
        venus.spawn(async move {
            let keep_going = Rc::new(Cell::new(true));
            loop {
                let _ = sleeper.sleep(TRANSITION_FRAME).await;
                if !keep_going.get() {
                    break;
                }
                let flag = Rc::clone(&keep_going);
                let alive = updater.try_set_state(move |state| {
                    state.advance_slots(Instant::now());
                    if !state.needs_animation() {
                        flag.set(false);
                        state.ticking = false;
                    }
                });
                if alive.is_none() {
                    break;
                }
                aimer::events::window::request_animation_frame();
            }
        });
    }
}

impl WidgetState<AnimatedMark> for AnimatedMarkState {
    fn create(widget: AnimatedMark) -> Self {
        let segments = segments_of(widget.look.as_ref(), widget.activity);
        let slots = target_offsets(&segments)
            .into_iter()
            .map(|(segment, right)| AnimatedSlot::settled(segment, right))
            .collect();
        Self {
            doc_id: widget.doc_id,
            look: widget.look,
            activity: widget.activity,
            rendered_look: widget.look,
            rendered_activity: widget.activity,
            busy_debounce_gen: 0,
            busy_debounce_pending: false,
            count_debounce_gen: 0,
            count_debounce_pending: false,
            slots,
            updater: None,
            ticking: false,
        }
    }

    fn on_mount(&mut self, updater: StateUpdater<Self>) {
        self.updater = Some(updater);
        self.ensure_ticking();
    }

    fn adopt_config_from(&mut self, new: Self) {
        if self.doc_id == new.doc_id && self.look == new.look && self.activity == new.activity {
            return;
        }
        let same_doc = self.doc_id == new.doc_id;
        let debounce_busy = same_doc
            && self
                .rendered_look
                .is_some_and(|cur| cur.state == State::Ready)
            && new
                .look
                .is_some_and(|next| next.state == State::Busy);
        let debounce_counts = same_doc
            && self
                .rendered_look
                .is_some_and(|cur| matches!(cur.state, State::Ready | State::Busy))
            && new
                .look
                .is_some_and(|next| matches!(next.state, State::Ready | State::Busy))
            && self
                .rendered_look
                .zip(new.look)
                .is_some_and(|(cur, next)| {
                    next.errors < cur.errors || next.warnings < cur.warnings
                });

        self.doc_id = new.doc_id;
        self.look = new.look;
        self.activity = new.activity;

        if debounce_busy {
            if !self.busy_debounce_pending {
                self.schedule_busy_debounce();
            }
        } else {
            self.busy_debounce_gen += 1;
            self.busy_debounce_pending = false;
        }

        if debounce_counts {
            self.schedule_count_debounce();
        } else {
            self.count_debounce_gen += 1;
            self.count_debounce_pending = false;
        }

        let effective = self.effective_look();
        self.apply_effective(effective, new.activity, Instant::now());
    }

    fn build(this: StateView<Self>, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let store = ProviderHandle::<Workspace>::of(ctx);
        if this.slots.is_empty() || this.slots.iter().all(|slot| slot.alpha <= 0.01) {
            BOUNDS.set(None);
            return ZeroSizedBox.notype();
        }
        let container_width = this
            .slots
            .iter()
            .map(|slot| slot.right + slot.segment.card_width() as f32)
            .fold(0.0_f32, f32::max);
        BOUNDS.set(Some((container_width.ceil() as u32, SIZE)));

        let mut stack = Stack::new();
        for slot in this.slots.iter() {
            let segment = slot.segment;
            let width = segment.card_width() as f32;
            let left = (container_width - slot.right - width).max(0.0);
            let alpha = slot.alpha.clamp(0.0, 1.0);
            let child: AnyWidget = if slot.exiting {
                card(segment, width, alpha, &theme, false)
            } else {
                let click_store = store.clone();
                Button::new()
                    .no_decor()
                    .on_press(move || click_store.update(|space| space.toggle_problems()))
                    .build(move |state| card(segment, width, alpha, &theme, state.is_hovered))
                    .notype()
            };
            stack = stack.add_child(
                Positioned::new()
                    .left(left)
                    .top(0.0)
                    .child(child),
            );
        }

        // The badge floats over the text, whose field asks for the text cursor on every move of the pointer in it, unless
        // it is told that this part of it is covered: the pane is, and the badge is an arrow.
        MouseRegion::new()
            .cursor(CursorIcon::Default)
            .on_pointer_activity(|_| {
                set_cursor(CursorIcon::Default);
            })
            .box_child(
                SizedBox::new()
                    .width(container_width)
                    .height(SIZE)
                    .box_child(stack),
            )
    }
}

/// One individual rounded card in the row.
fn card(segment: Segment, width: f32, alpha: f32, theme: &CodeTheme, hovered: bool) -> AnyWidget {
    let accent = segment.color(theme);
    let surface = theme
        .editor
        .lerp(
            accent,
            if hovered {
                CARD_TINT_HOVERED
            } else {
                CARD_TINT
            },
        )
        .with_alpha(alpha);
    let border = accent.with_alpha(
        (if hovered {
            CARD_BORDER_HOVERED
        } else {
            CARD_BORDER
        }) * alpha,
    );
    let decoration = BoxDecoration::new()
        .background_color(surface)
        .border_radius(7.0)
        .border(BoxBorder::all(
            BorderSlice::new()
                .color(border)
                .style(BorderStyle::Solid)
                .stroke(1.0),
        ));
    let content = part(segment, theme, alpha);
    let padding = if segment.content_width() == ICON {
        pad(INSET, INSET, INSET, INSET)
    } else {
        pad(INSET, CARD_PAD_H - 2, INSET, CARD_PAD_H - 2)
    };
    Container::new()
        .width(width)
        .height(SIZE)
        .padding(padding)
        .decoration(decoration)
        .child(content)
        .notype()
}

/// Content inside one card.
fn part(segment: Segment, theme: &CodeTheme, alpha: f32) -> AnyWidget {
    let counted = |glyph, color: aimer::Color, count: usize| {
        let color = color.with_alpha(alpha);
        Row::new()
            .vertical_alignment(BoxAlignment::Center)
            .gaps(LayoutSpacing::all(COUNT_GAP))
            .children([
                icon_stroked(glyph, ICON, color, 2.0).boxed(),
                Text::new(count.to_string())
                    .text_style(
                        fonts::ui()
                            .font_size(13)
                            .font_weight(FontWeight::Bold)
                            .color(color),
                    )
                    .boxed(),
            ])
            .notype()
    };
    Opacity::new()
        .opacity(alpha)
        .box_child(match segment {
            Segment::Activity(activity) => FileActivityIcon::new(activity).notype(),
            Segment::Check => {
                icon_stroked(Icons::CHECK, ICON, theme.success.with_alpha(alpha), 2.0).notype()
            }
            Segment::Failed => icon_stroked(
                Icons::CIRCLE_ALERT,
                ICON,
                theme.warning.with_alpha(alpha),
                2.0,
            )
            .notype(),
            Segment::Spinner => Spinner::new(ICON).notype(),
            Segment::Errors(count) => counted(Icons::CIRCLE_X, theme.warning, count),
            Segment::Warnings(count) => counted(Icons::TRIANGLE_ALERT, theme.caution, count),
        })
}

/// Static hard-drive body shared by Lucide's `hard-drive-upload` and `hard-drive-download` icons.
static HARD_DRIVE_BODY: StaticSvg = StaticSvg::new(
    "hard-drive-body",
    r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 14h16a2 2 0 0 1 2 2v4a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2v-4a2 2 0 0 1 2-2Z"/><path d="M6 18h.01"/><path d="M10 18h.01"/></svg>"#,
);
/// Arrow part of Lucide's `hard-drive-upload` icon (animated upward on external file sync).
static HARD_DRIVE_UPLOAD_ARROW: StaticSvg = StaticSvg::new(
    "hard-drive-upload-arrow",
    r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m16 6-4-4-4 4"/><path d="M12 2v8"/></svg>"#,
);
/// Arrow part of Lucide's `hard-drive-download` icon (animated downward on file save).
static HARD_DRIVE_DOWNLOAD_ARROW: StaticSvg = StaticSvg::new(
    "hard-drive-download-arrow",
    r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2v8"/><path d="m16 6-4 4-4-4"/></svg>"#,
);

/// Frame interval for the file save/sync arrow animation (~30 fps).
const ACTIVITY_FRAME: Duration = Duration::from_millis(33);
/// Maximum vertical travel (logical px) for the moving arrow above the static hard-drive body.
const ACTIVITY_TRAVEL: f32 = 3.2;

/// Computes `(arrow_alpha, arrow_y_offset)` for a file activity icon at normalized progress `t` in `0.0..=1.0`.
///
/// The hard-drive body remains stationary while only the arrow pulses twice across the animation
/// duration (>= 1s): gliding downward into the drive for Save (`hard-drive-download`) and upward
/// out of the drive for Sync (`hard-drive-upload`).
fn activity_frame(kind: FileActivityKind, progress: f32) -> (f32, f32) {
    let t = progress.clamp(0.0, 1.0);
    let wave = 0.5 * (1.0 - (2.0 * TAU * t).cos());
    let alpha = 0.65 + 0.35 * wave;
    let direction = match kind {
        FileActivityKind::Save => 1.0,
        FileActivityKind::Sync => -1.0,
    };
    (alpha.clamp(0.5, 1.0), direction * ACTIVITY_TRAVEL * wave)
}

/// Animated `hard-drive-upload` (sync) or `hard-drive-download` (save) icon inside the LSP badge.
#[derive(Clone, StatefulWidget)]
struct FileActivityIcon {
    key: Option<Key>,
    activity: FileActivity,
}

impl FileActivityIcon {
    fn new(activity: FileActivity) -> Self {
        Self {
            key: Some(Key::from(("file-activity", activity.seq))),
            activity,
        }
    }
}

struct FileActivityIconState {
    activity: FileActivity,
    started: Instant,
    progress: f32,
}

impl WidgetState<FileActivityIcon> for FileActivityIconState {
    fn create(widget: FileActivityIcon) -> Self {
        Self {
            activity: widget.activity,
            started: Instant::now(),
            progress: 0.0,
        }
    }

    fn on_mount(&mut self, updater: StateUpdater<Self>) {
        let Some(venus) = Venus::current() else {
            return;
        };
        let started = self.started;
        let duration = FILE_ACTIVITY_DURATION.as_secs_f32();
        let sleeper = venus.clone();
        venus.spawn(async move {
            loop {
                let _ = sleeper.sleep(ACTIVITY_FRAME).await;
                let progress = (started.elapsed().as_secs_f32() / duration).clamp(0.0, 1.0);
                let alive = updater.try_set_state(move |state| {
                    state.progress = progress;
                });
                if alive.is_none() || progress >= 1.0 {
                    break;
                }
                aimer::events::window::request_animation_frame();
            }
        });
    }

    fn adopt_config_from(&mut self, new: Self) {
        self.activity = new.activity;
    }

    fn build(this: StateView<Self>, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let (arrow_alpha, arrow_y) = activity_frame(this.activity.kind, this.progress);
        let arrow_glyph = match this.activity.kind {
            FileActivityKind::Sync => &HARD_DRIVE_UPLOAD_ARROW,
            FileActivityKind::Save => &HARD_DRIVE_DOWNLOAD_ARROW,
        };
        let body_color = theme.marker;
        let arrow_color = theme.marker.with_alpha(arrow_alpha);
        SizedBox::new()
            .width(ICON - 2)
            .height(ICON)
            .box_child(
                Stack::new()
                    .add_child(icon_stroked(&HARD_DRIVE_BODY, ICON, body_color, 2.0))
                    .add_child(
                        Positioned::new()
                            .top(arrow_y)
                            .left(0.0)
                            .child(icon_stroked(arrow_glyph, ICON, arrow_color, 2.0)),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(
        status: Option<Status>,
        progress: Option<&str>,
        errors: usize,
        warnings: usize,
    ) -> Summary {
        Summary {
            status,
            progress: progress.map(str::to_owned),
            progress_percentage: None,
            errors,
            warnings,
        }
    }

    fn look(
        status: Option<Status>,
        progress: Option<&str>,
        errors: usize,
        warnings: usize,
    ) -> Option<Look> {
        look_of(&summary(status, progress, errors, warnings))
    }

    fn of(state: State, errors: usize, warnings: usize) -> Option<Look> {
        Some(Look {
            state,
            errors,
            warnings,
        })
    }

    #[test]
    fn a_file_the_server_has_nothing_against_is_ready_and_has_no_counts() {
        assert_eq!(
            look(Some(Status::Ready), None, 0, 0),
            of(State::Ready, 0, 0)
        );
    }

    #[test]
    fn the_errors_and_the_warnings_are_both_counted() {
        assert_eq!(
            look(Some(Status::Ready), None, 3, 0),
            of(State::Ready, 3, 0)
        );
        assert_eq!(
            look(Some(Status::Ready), None, 0, 2),
            of(State::Ready, 0, 2)
        );
        assert_eq!(
            look(Some(Status::Ready), None, 1, 9),
            of(State::Ready, 1, 9),
            "the warnings do not wait for the errors to be mended"
        );
    }

    #[test]
    fn a_server_that_is_starting_or_working_is_busy_and_still_says_what_it_has_found() {
        assert_eq!(
            look(Some(Status::Starting), None, 0, 0),
            of(State::Starting, 0, 0)
        );
        assert_eq!(
            look(Some(Status::Ready), Some("Indexing 40%"), 0, 0),
            of(State::Busy, 0, 0)
        );
        assert_eq!(
            look(Some(Status::Ready), Some("Working"), 2, 1),
            of(State::Busy, 2, 1),
            "an error is not hidden by a spinner"
        );
    }

    #[test]
    fn a_server_that_failed_is_a_failure_whatever_it_had_found() {
        assert_eq!(
            look(Some(Status::Failed("it exited".into())), None, 0, 0),
            of(State::Failed, 0, 0)
        );
        assert_eq!(
            look(
                Some(Status::Failed("it exited".into())),
                Some("Working"),
                4,
                4
            ),
            of(State::Failed, 0, 0)
        );
    }

    #[test]
    fn a_server_that_was_not_asked_for_or_has_stopped_has_no_mark() {
        assert_eq!(look(None, None, 0, 0), None);
        assert_eq!(look(Some(Status::Stopped), None, 5, 5), None);
    }

    fn segments(state: State, errors: usize, warnings: usize) -> Vec<Segment> {
        segments_of(
            Some(&Look {
                state,
                errors,
                warnings,
            }),
            None,
        )
    }

    #[test]
    fn items_are_sorted_by_importance_from_most_critical_to_least() {
        assert_eq!(segments(State::Ready, 0, 0), [Segment::Check]);
        assert_eq!(
            segments(State::Busy, 0, 0),
            [Segment::Spinner],
            "and not a check while it works"
        );
        assert_eq!(segments(State::Ready, 2, 0), [Segment::Errors(2)]);
        assert_eq!(segments(State::Ready, 0, 5), [Segment::Warnings(5)]);
        assert_eq!(
            segments(State::Ready, 2, 5),
            [Segment::Errors(2), Segment::Warnings(5)],
            "errors come before warnings"
        );
        assert_eq!(
            segments(State::Busy, 1, 1),
            [Segment::Errors(1), Segment::Warnings(1), Segment::Spinner],
            "errors and warnings sit above background spinner"
        );
        assert_eq!(
            segments(State::Failed, 3, 3),
            [Segment::Failed],
            "a server that failed has found nothing to count"
        );
    }

    #[test]
    fn card_widths_are_square_for_icons_and_grow_with_digits_without_clipping() {
        assert_eq!(Segment::Check.card_width(), SIZE);
        assert_eq!(Segment::Spinner.card_width(), SIZE);
        let one = Segment::Errors(3).card_width();
        assert_eq!(
            one,
            CARD_PAD_H + ICON + COUNT_GAP + DIGIT + CARD_PAD_H + CARD_BORDER_EXTRA
        );
        assert!(
            Segment::Errors(100).card_width() > one,
            "more digits make a wider card"
        );
    }

    #[test]
    fn the_icon_has_the_same_room_on_every_side_of_the_square_it_is_in() {
        assert!(
            SIZE >= ICON,
            "a {ICON} icon does not fit in a {SIZE} square"
        );
        assert_eq!(SIZE - ICON, 2 * INSET);
        assert_eq!(INSET, (SIZE - ICON) / 2);
        assert_eq!(SIZE, INSET + ICON + INSET);
    }

    #[test]
    fn row_width_sums_all_cards_with_card_gap_between_them() {
        assert_eq!(row_width(&[]), 0);
        assert_eq!(row_width(&[Segment::Check]), SIZE);
        let cards = [Segment::Errors(2), Segment::Warnings(1), Segment::Spinner];
        assert_eq!(
            row_width(&cards),
            Segment::Errors(2).card_width()
                + CARD_GAP
                + Segment::Warnings(1).card_width()
                + CARD_GAP
                + Segment::Spinner.card_width()
        );
    }

    #[test]
    fn the_part_of_the_pane_the_mark_covers_matches_the_stacked_or_spread_bounds() {
        BOUNDS.set(Some((46, 50)));
        assert_eq!(
            covered(800.0),
            vec![(800.0 - RIGHT as f32 - 46.0, TOP as f32, 46.0, 50.0)]
        );
        BOUNDS.set(Some((SIZE, SIZE)));
        assert_eq!(
            covered(300.0),
            vec![(
                300.0 - RIGHT as f32 - SIZE as f32,
                TOP as f32,
                SIZE as f32,
                SIZE as f32
            )]
        );
        BOUNDS.set(None);
        assert!(covered(800.0).is_empty());
    }

    #[test]
    fn each_card_uses_its_own_semantic_color() {
        let theme = CodeTheme::dark();
        let save = FileActivity {
            kind: FileActivityKind::Save,
            seq: 1,
        };
        assert_eq!(Segment::Failed.color(&theme), theme.warning);
        assert_eq!(Segment::Errors(1).color(&theme), theme.warning);
        assert_eq!(Segment::Warnings(2).color(&theme), theme.caution);
        assert_eq!(Segment::Activity(save).color(&theme), theme.marker);
        assert_eq!(Segment::Spinner.color(&theme), theme.muted);
        assert_eq!(Segment::Check.color(&theme), theme.success);
    }

    #[test]
    fn save_and_sync_activity_stack_with_lsp_cards_by_importance() {
        let save = FileActivity {
            kind: FileActivityKind::Save,
            seq: 1,
        };
        let sync = FileActivity {
            kind: FileActivityKind::Sync,
            seq: 2,
        };
        assert_eq!(segments_of(None, Some(save)), [Segment::Activity(save)]);
        assert_eq!(segments_of(None, Some(sync)), [Segment::Activity(sync)]);
        assert_eq!(
            segments_of(
                Some(&Look {
                    state: State::Ready,
                    errors: 0,
                    warnings: 0,
                }),
                Some(save)
            ),
            [Segment::Activity(save), Segment::Check],
            "save stacks above the idle check card"
        );
        assert_eq!(
            segments_of(
                Some(&Look {
                    state: State::Ready,
                    errors: 2,
                    warnings: 0,
                }),
                Some(sync)
            ),
            [Segment::Errors(2), Segment::Activity(sync)],
            "errors outrank sync and sit on top of the stack"
        );
        assert_eq!(
            segments_of(
                Some(&Look {
                    state: State::Failed,
                    errors: 0,
                    warnings: 0,
                }),
                Some(save)
            ),
            [Segment::Failed, Segment::Activity(save)],
            "failed server sits above save"
        );
    }

    #[test]
    fn activity_animation_nudges_down_for_save_and_up_for_sync_with_visible_alpha() {
        let (alpha_save, y_save) = activity_frame(FileActivityKind::Save, 0.25);
        let (alpha_sync, y_sync) = activity_frame(FileActivityKind::Sync, 0.25);
        assert!(y_save > 0.0, "save (download) nudges downward: {y_save}");
        assert!(y_sync < 0.0, "sync (upload) nudges upward: {y_sync}");
        assert!((0.5..=1.0).contains(&alpha_save));
        assert!((0.5..=1.0).contains(&alpha_sync));
    }

    #[test]
    fn badge_transitions_animate_show_hide_and_newly_added_items_for_at_least_400ms() {
        assert!(
            ITEM_ANIM_DURATION >= Duration::from_millis(400),
            "item animation must play for at least 400ms"
        );
        // Hidden -> shown triggers fade-in and move animation for the shown item.
        let mut state = AnimatedMarkState::create(AnimatedMark::new(Some(1), None, None));
        assert!(state.slots.is_empty());
        assert!(!state.needs_animation());

        let ready = Some(Look {
            state: State::Ready,
            errors: 0,
            warnings: 0,
        });
        state.adopt_config_from(AnimatedMarkState::create(AnimatedMark::new(
            Some(1),
            ready,
            None,
        )));
        assert_eq!(state.slots.len(), 1);
        assert_eq!(state.slots[0].segment.kind(), SegmentKind::Check);
        assert_eq!(state.slots[0].target_alpha, 1.0);
        assert!(state.slots[0].from_right > state.slots[0].target_right);
        assert!(state.needs_animation());

        // Settle initial show.
        let settled_at = Instant::now() + ITEM_ANIM_DURATION + Duration::from_millis(10);
        state.advance_slots(settled_at);
        assert!(!state.needs_animation());
        assert_eq!(state.slots[0].right, 0.0);

        // Replacing Check with Errors and Warnings animates both entering and Check exiting.
        let multi = Some(Look {
            state: State::Ready,
            errors: 2,
            warnings: 1,
        });
        state.adopt_config_from(AnimatedMarkState::create(AnimatedMark::new(
            Some(1),
            multi,
            None,
        )));
        assert!(state.needs_animation());
        let errors_slot = state
            .slots
            .iter()
            .find(|s| s.segment.kind() == SegmentKind::Errors)
            .expect("errors slot");
        let warnings_slot = state
            .slots
            .iter()
            .find(|s| s.segment.kind() == SegmentKind::Warnings)
            .expect("warnings slot");
        let check_slot = state
            .slots
            .iter()
            .find(|s| s.segment.kind() == SegmentKind::Check)
            .expect("exiting check slot");
        assert_eq!(errors_slot.target_right, 0.0);
        assert_eq!(
            warnings_slot.target_right,
            Segment::Errors(2).card_width() as f32 + CARD_GAP as f32
        );
        assert!(
            warnings_slot.from_right < warnings_slot.target_right,
            "card to the left slides outward from behind its neighbor"
        );
        assert!(check_slot.exiting);
        assert_eq!(check_slot.target_alpha, 0.0);

        // Once settled, exiting Check is pruned.
        state.advance_slots(Instant::now() + ITEM_ANIM_DURATION + Duration::from_millis(10));
        assert_eq!(state.slots.len(), 2);
        assert!(!state.needs_animation());

        // Hiding keeps slots alive while sliding and fading out toward alpha 0.
        state.adopt_config_from(AnimatedMarkState::create(AnimatedMark::new(
            Some(1),
            None,
            None,
        )));
        assert!(
            state
                .slots
                .iter()
                .all(|s| s.exiting && s.target_alpha == 0.0)
        );
        assert!(state.needs_animation());
        state.advance_slots(Instant::now() + ITEM_ANIM_DURATION + Duration::from_millis(10));
        assert!(state.slots.is_empty());
    }

    #[test]
    fn existing_items_move_smoothly_when_siblings_enter_or_exit() {
        let warnings_only = Some(Look {
            state: State::Ready,
            errors: 0,
            warnings: 1,
        });
        let both = Some(Look {
            state: State::Ready,
            errors: 2,
            warnings: 1,
        });
        let mut state = AnimatedMarkState::create(AnimatedMark::new(Some(1), warnings_only, None));
        assert_eq!(state.slots.len(), 1);
        assert_eq!(state.slots[0].right, 0.0);

        // Errors enters at the right edge -> Warnings moves smoothly to the left.
        state.adopt_config_from(AnimatedMarkState::create(AnimatedMark::new(
            Some(1),
            both,
            None,
        )));
        let warnings_target = Segment::Errors(2).card_width() as f32 + CARD_GAP as f32;
        let warnings = state
            .slots
            .iter()
            .find(|s| s.segment.kind() == SegmentKind::Warnings)
            .expect("warnings slot");
        assert_eq!(warnings.from_right, 0.0);
        assert_eq!(warnings.target_right, warnings_target);
        assert!(warnings.started.is_some());

        // Mid-animation, Warnings is between 0.0 and warnings_target.
        let started = warnings.started.expect("started");
        state.advance_slots(started + ITEM_ANIM_DURATION / 2);
        let mid_right = state
            .slots
            .iter()
            .find(|s| s.segment.kind() == SegmentKind::Warnings)
            .expect("warnings slot")
            .right;
        assert!(
            mid_right > 0.0 && mid_right < warnings_target,
            "warnings should be mid-move: {mid_right} vs {warnings_target}"
        );

        // Finish enter animation.
        state.advance_slots(started + ITEM_ANIM_DURATION + Duration::from_millis(10));
        assert!(!state.needs_animation());

        // Now Errors exits (after debounce expires) -> Warnings moves smoothly back to 0.0 while
        // Errors fades out underneath.
        state.adopt_config_from(AnimatedMarkState::create(AnimatedMark::new(
            Some(1),
            warnings_only,
            None,
        )));
        let ticket = state.count_debounce_gen;
        let exit_start = Instant::now();
        state.finish_count_debounce(ticket, exit_start);
        let warnings = state
            .slots
            .iter()
            .find(|s| s.segment.kind() == SegmentKind::Warnings)
            .expect("warnings slot");
        let errors = state
            .slots
            .iter()
            .find(|s| s.segment.kind() == SegmentKind::Errors)
            .expect("exiting errors slot");
        assert_eq!(warnings.from_right, warnings_target);
        assert_eq!(warnings.target_right, 0.0);
        assert!(!warnings.exiting);
        assert!(errors.exiting);
        assert_eq!(errors.target_alpha, 0.0);

        // Re-entering Errors mid-exit reuses the slot smoothly without duplicating it.
        state.advance_slots(exit_start + ITEM_ANIM_DURATION / 2);
        state.adopt_config_from(AnimatedMarkState::create(AnimatedMark::new(
            Some(1),
            both,
            None,
        )));
        let errors_count = state
            .slots
            .iter()
            .filter(|s| s.segment.kind() == SegmentKind::Errors)
            .count();
        assert_eq!(errors_count, 1, "re-entering item reuses its exiting slot");
        let errors = state
            .slots
            .iter()
            .find(|s| s.segment.kind() == SegmentKind::Errors)
            .expect("re-entered errors slot");
        assert!(!errors.exiting);
        assert_eq!(errors.target_alpha, 1.0);
    }

    #[test]
    fn short_ready_to_busy_check_is_debounced_while_startup_spinner_is_immediate() {
        let ready = Some(Look {
            state: State::Ready,
            errors: 0,
            warnings: 0,
        });
        let busy = Some(Look {
            state: State::Busy,
            errors: 0,
            warnings: 0,
        });
        let mut state = AnimatedMarkState::create(AnimatedMark::new(Some(1), ready, None));
        state.adopt_config_from(AnimatedMarkState::create(AnimatedMark::new(
            Some(1),
            busy,
            None,
        )));
        assert_eq!(
            state.rendered_look.map(|l| l.state),
            Some(State::Ready),
            "spinner does not flash during a brief ready -> busy check"
        );
        // Returning to Ready cancels the pending debounce cleanly.
        state.adopt_config_from(AnimatedMarkState::create(AnimatedMark::new(
            Some(1),
            ready,
            None,
        )));
        assert_eq!(state.rendered_look.map(|l| l.state), Some(State::Ready));

        // Initial startup shows the spinner immediately.
        let starting = Some(Look {
            state: State::Starting,
            errors: 0,
            warnings: 0,
        });
        let mut boot = AnimatedMarkState::create(AnimatedMark::new(Some(1), None, None));
        boot.adopt_config_from(AnimatedMarkState::create(AnimatedMark::new(
            Some(1),
            starting,
            None,
        )));
        assert_eq!(boot.rendered_look.map(|l| l.state), Some(State::Starting));
    }

    #[test]
    fn short_count_drop_does_not_flicker_error_and_warning_counts() {
        let with_counts = Some(Look {
            state: State::Ready,
            errors: 2,
            warnings: 1,
        });
        let cleared = Some(Look {
            state: State::Ready,
            errors: 0,
            warnings: 0,
        });
        let busy_cleared = Some(Look {
            state: State::Busy,
            errors: 0,
            warnings: 0,
        });
        let mut state = AnimatedMarkState::create(AnimatedMark::new(Some(1), with_counts, None));

        // Brief drop to (0, 0) on edit holds the previous counts without triggering exit animations.
        state.adopt_config_from(AnimatedMarkState::create(AnimatedMark::new(
            Some(1),
            cleared,
            None,
        )));
        assert_eq!(
            state.rendered_look, with_counts,
            "brief drop to zero during a short action must not flicker counts away"
        );
        assert!(!state.needs_animation());

        // Brief Ready -> Busy with (0, 0) also holds both Ready state and counts.
        state.adopt_config_from(AnimatedMarkState::create(AnimatedMark::new(
            Some(1),
            busy_cleared,
            None,
        )));
        assert_eq!(state.rendered_look, with_counts);

        // Returning to (2, 1) before the debounce expires cancels the drop with zero flicker.
        state.adopt_config_from(AnimatedMarkState::create(AnimatedMark::new(
            Some(1),
            with_counts,
            None,
        )));
        assert_eq!(state.rendered_look, with_counts);
        assert!(!state.count_debounce_pending);

        // An increase in counts is reflected immediately without waiting for debounce.
        let more_errors = Some(Look {
            state: State::Ready,
            errors: 3,
            warnings: 1,
        });
        state.adopt_config_from(AnimatedMarkState::create(AnimatedMark::new(
            Some(1),
            more_errors,
            None,
        )));
        assert_eq!(state.rendered_look, more_errors);

        // A genuine fix clears counts once the debounce window expires.
        state.adopt_config_from(AnimatedMarkState::create(AnimatedMark::new(
            Some(1),
            cleared,
            None,
        )));
        assert_eq!(state.rendered_look, more_errors);
        let ticket = state.count_debounce_gen;
        state.finish_count_debounce(ticket, Instant::now());
        assert_eq!(state.rendered_look, cleared);

        // Switching to a different document applies its counts immediately without holding stale ones.
        let mut switched = AnimatedMarkState::create(AnimatedMark::new(Some(1), with_counts, None));
        switched.adopt_config_from(AnimatedMarkState::create(AnimatedMark::new(
            Some(2),
            cleared,
            None,
        )));
        assert_eq!(
            switched.rendered_look, cleared,
            "switching documents must not carry over the previous document's counts"
        );
    }
}
