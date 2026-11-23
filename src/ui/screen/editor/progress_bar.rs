//! A macOS-styled pill progress bar for the status bar: determinate when the language server
//! reports a percentage, and a smooth ease-in-out sweeping capsule while indeterminate.

use std::cell::Cell;
use std::f32::consts::PI;
use std::rc::Rc;
use std::time::Duration;

use crate::ui::theme::CodeTheme;
use aimer::decoration::BoxDecoration;
use aimer::style::{BorderSlice, BorderStyle, BoxBorder, Theme};
use aimer::{
    BuildContext, Container, Key, Row, SizedBox, Stack, State, StateUpdater, StateView,
    StatefulWidget, Venus, Widget, ZeroSizedBox,
};

/// Default track width in logical pixels, matching macOS compact status-bar progress indicators.
const DEFAULT_WIDTH: f32 = 175.0;
/// Track height in logical pixels (pill radius is half of this).
const DEFAULT_HEIGHT: f32 = 5.0;
/// Base width of the indeterminate capsule as a fraction of the track width (scaled 80%–150%–80%).
const INDETERMINATE_RATIO: f32 = 0.15;
/// Capsule scale at the track edges (80% of base width).
const INDETERMINATE_EDGE_SCALE: f32 = 0.40;
/// Capsule scale at the middle of the sweep (150% of base width).
const INDETERMINATE_PEAK_SCALE: f32 = 2.0;
/// Linear weight of the normal-fast-normal speed curve (blended with cubic ease-in-out).
const NORMAL_SPEED_WEIGHT: f32 = 0.45;
/// Time between animation ticks (~30 fps, light on debug builds while still smooth).
const FRAME: Duration = Duration::from_millis(8);
/// Full back-and-forth sweep period for the indeterminate capsule, in seconds.
const PERIOD_SECS: f32 = 1.35;
/// Exponential smoothing factor per tick for determinate value transitions.
const SMOOTHING: f32 = 0.58;
/// Stop animating determinate progress once within this distance of the target.
const CONVERGE_EPSILON: f32 = 0.003;

/// Computes `(left, fill_width)` in logical pixels for the active pill inside a track of
/// `width × height`.
///
/// For determinate progress (`Some(fraction)`), `left` is `0.0` and `fill_width` grows from
/// `0.0` (at `0.0`) to `width` (at `1.0`), keeping a minimum width of `height` whenever
/// `fraction > 0.0` so the pill stays cleanly rounded.
///
/// For indeterminate progress (`None`), a capsule glides smoothly back and forth between the
/// two ends of the track using a cosine ease-in-out curve for `phase` in `[0.0, 1.0)`.
pub(crate) fn bar_geometry(
    width: f32,
    height: f32,
    fraction: Option<f32>,
    phase: f32,
) -> (f32, f32) {
    let width = width.max(0.0);
    let height = height.max(0.0).min(width);
    match fraction {
        Some(value) => {
            let clamped = if value.is_finite() {
                value.clamp(0.0, 1.0)
            } else {
                0.0
            };
            if clamped <= 0.0 {
                (0.0, 0.0)
            } else {
                (0.0, (width * clamped).max(height).min(width))
            }
        }
        None => {
            let phase = phase.rem_euclid(1.0);
            let (u, forward) = if phase < 0.5 {
                (phase * 2.0, true)
            } else {
                ((phase - 0.5) * 2.0, false)
            };
            // Normal-fast-normal speed curve: steady normal speed at the edges, accelerating
            // smoothly through the middle of the track and returning to normal at the opposite edge.
            let ease = if u < 0.5 {
                4.0 * u * u * u
            } else {
                1.0 - (-2.0 * u + 2.0).powi(3) / 2.0
            };
            let curve = NORMAL_SPEED_WEIGHT * u + (1.0 - NORMAL_SPEED_WEIGHT) * ease;
            // Dynamic capsule size: 80% at edges -> 150% in the middle -> 80% at edges.
            let bell = (PI * u).sin();
            let scale =
                INDETERMINATE_EDGE_SCALE + (INDETERMINATE_PEAK_SCALE - INDETERMINATE_EDGE_SCALE) * bell;
            let capsule = (width * INDETERMINATE_RATIO * scale)
                .max(height)
                .min(width);
            let edge_capsule = (width * INDETERMINATE_RATIO * INDETERMINATE_EDGE_SCALE)
                .max(height)
                .min(width);
            let min_center = edge_capsule * 0.5;
            let max_center = (width - edge_capsule * 0.5).max(min_center);
            let pos = if forward { curve } else { 1.0 - curve };
            let center = min_center + (max_center - min_center) * pos;
            let left = (center - capsule * 0.5).clamp(0.0, (width - capsule).max(0.0));
            (left, capsule)
        }
    }
}

/// Advances the indeterminate phase by one animation tick, wrapping in `[0.0, 1.0)`.
fn advance_phase(phase: f32) -> f32 {
    let next = phase + FRAME.as_secs_f32() / PERIOD_SECS;
    next - next.floor()
}

/// Steps a determinate displayed fraction smoothly toward `target`, snapping once close enough.
fn step_toward(current: f32, target: f32) -> f32 {
    let target = if target.is_finite() {
        target.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let delta = target - current;
    if delta.abs() <= CONVERGE_EPSILON {
        target
    } else {
        current + delta * SMOOTHING
    }
}

/// A macOS-design pill progress bar.
///
/// Pass `Some(0.0..=1.0)` for determinate progress or `None` for an indeterminate sweeping bar.
#[derive(Clone, StatefulWidget)]
pub struct MacProgressBar {
    key: Option<Key>,
    fraction: Option<f32>,
    width: f32,
    height: f32,
}

impl MacProgressBar {
    /// Creates a progress bar with optional determinate fraction (`0.0..=1.0`).
    pub fn new(fraction: Option<f32>) -> Self {
        Self {
            key: Some(Key::from("lsp-progress-bar")),
            fraction: fraction.map(|f| if f.is_finite() { f.clamp(0.0, 1.0) } else { 0.0 }),
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
        }
    }
}

struct MacProgressBarState {
    fraction: Option<f32>,
    displayed: f32,
    phase: f32,
    width: f32,
    height: f32,
    updater: Option<StateUpdater<Self>>,
    ticking: bool,
}

impl MacProgressBarState {
    fn needs_animation(&self) -> bool {
        match self.fraction {
            None => true,
            Some(target) => (target - self.displayed).abs() > CONVERGE_EPSILON,
        }
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
            // `try_set_state` queues the mutation for the next frame, so keep_going starts `true`
            // and is only flipped to `false` by the mutation once the animation finishes.
            let keep_going = Rc::new(Cell::new(true));
            loop {
                let _ = sleeper.sleep(FRAME).await;
                if !keep_going.get() {
                    break;
                }
                let flag = Rc::clone(&keep_going);
                let alive = updater.try_set_state(move |state| {
                    let more = match state.fraction {
                        None => {
                            state.phase = advance_phase(state.phase);
                            true
                        }
                        Some(target) => {
                            state.displayed = step_toward(state.displayed, target);
                            state.needs_animation()
                        }
                    };
                    if !more {
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

impl State<MacProgressBar> for MacProgressBarState {
    fn create(widget: MacProgressBar) -> Self {
        Self {
            fraction: widget.fraction,
            displayed: widget.fraction.unwrap_or(0.0),
            phase: 0.0,
            width: widget.width,
            height: widget.height,
            updater: None,
            ticking: false,
        }
    }

    fn on_mount(&mut self, updater: StateUpdater<Self>) {
        self.updater = Some(updater);
        self.ensure_ticking();
    }

    fn adopt_config_from(&mut self, new: Self) {
        self.width = new.width;
        self.height = new.height;
        if self.fraction != new.fraction {
            let was_indeterminate = self.fraction.is_none();
            self.fraction = new.fraction;
            if let Some(target) = self.fraction {
                if was_indeterminate {
                    // Start smoothly from the beginning when transitioning from indeterminate.
                    self.displayed = target;
                }
            }
            self.ensure_ticking();
        }
    }

    fn build(this: StateView<Self>, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let radius = this.height / 2.0;
        let active = this.fraction.map(|_| this.displayed);
        let (left, fill_width) = bar_geometry(this.width, this.height, active, this.phase);

        let track_fill = theme.text.with_alpha(0.11);
        let track_border = theme.text.with_alpha(0.10);
        let bar_color = theme.marker;

        let mut stack = Stack::new().add_child(
            SizedBox::new()
                .width(this.width)
                .height(this.height)
                .box_child(
                    Container::new()
                        .decoration(
                            BoxDecoration::new()
                                .background_color(track_fill)
                                .border_radius(radius)
                                .border(BoxBorder::all(
                                    BorderSlice::new()
                                        .color(track_border)
                                        .style(BorderStyle::Solid)
                                        .stroke(0.5),
                                )),
                        )
                        .child(ZeroSizedBox),
                ),
        );

        if fill_width > 0.0 {
            let fill_pill = SizedBox::new()
                .width(fill_width)
                .height(this.height)
                .box_child(
                    Container::new()
                        .decoration(
                            BoxDecoration::new()
                                .background_color(bar_color)
                                .border_radius(radius),
                        )
                        .child(ZeroSizedBox),
                );
            let bar_row = if left > 0.0 {
                Row::new()
                    .children([
                        SizedBox::new().width(left).height(this.height).boxed(),
                        fill_pill.boxed(),
                    ])
                    .notype()
            } else {
                fill_pill
            };
            stack = stack.add_child(bar_row);
        }

        SizedBox::new()
            .width(this.width)
            .height(this.height)
            .box_child(stack)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn determinate_bar_maps_zero_middle_and_full_cleanly_within_track() {
        assert_eq!(bar_geometry(100.0, 6.0, Some(0.0), 0.0), (0.0, 0.0));
        assert_eq!(
            bar_geometry(100.0, 6.0, Some(0.01), 0.0),
            (0.0, 6.0),
            "tiny positive progress keeps a full pill diameter"
        );
        assert_eq!(bar_geometry(100.0, 6.0, Some(0.5), 0.0), (0.0, 50.0));
        assert_eq!(bar_geometry(100.0, 6.0, Some(1.0), 0.0), (0.0, 100.0));
        assert_eq!(
            bar_geometry(100.0, 6.0, Some(1.5), 0.0),
            (0.0, 100.0),
            "values above 1 clamp to the track width"
        );
        assert_eq!(
            bar_geometry(100.0, 6.0, Some(-0.2), 0.0),
            (0.0, 0.0),
            "negative values clamp to zero"
        );
        assert_eq!(
            bar_geometry(100.0, 6.0, Some(f32::NAN), 0.0),
            (0.0, 0.0),
            "non-finite values fall back to zero"
        );
    }

    #[test]
    fn indeterminate_capsule_uses_normal_fast_normal_curve_and_80_150_80_size() {
        let width = 112.0;
        let height = 6.0;
        let base = width * INDETERMINATE_RATIO;

        let (left_start, cap_start) = bar_geometry(width, height, None, 0.0);
        let (left_mid_fwd, cap_mid_fwd) = bar_geometry(width, height, None, 0.25);
        let (left_end, cap_end) = bar_geometry(width, height, None, 0.5);
        let (_left_mid_back, cap_mid_back) = bar_geometry(width, height, None, 0.75);

        assert!((left_start - 0.0).abs() < 1e-4, "starts at left edge");
        assert!(
            ((left_end + cap_end) - width).abs() < 1e-4,
            "reaches right edge at half-cycle"
        );
        // 80% at edges, 150% at middle on both forward and return sweeps.
        assert!((cap_start - base * 0.80).abs() < 1e-3);
        assert!((cap_mid_fwd - base * 1.50).abs() < 1e-3);
        assert!((cap_end - base * 0.80).abs() < 1e-3);
        assert!((cap_mid_back - base * 1.50).abs() < 1e-3);
        assert!(
            (left_mid_fwd - (width - cap_mid_fwd) * 0.5).abs() < 1e-3,
            "centered at midpoint of sweep"
        );

        // Normal-fast-normal speed: center step moves faster than edge steps, and edges are symmetric.
        let center_of = |p: f32| {
            let (l, w) = bar_geometry(width, height, None, p);
            l + w * 0.5
        };
        let speed_start = center_of(0.02) - center_of(0.0);
        let speed_mid = center_of(0.26) - center_of(0.24);
        let speed_end = center_of(0.50) - center_of(0.48);
        assert!(
            speed_mid > speed_start * 2.0,
            "middle ({speed_mid}) is significantly faster than start ({speed_start})"
        );
        assert!(
            (speed_start - speed_end).abs() < 1e-2,
            "start ({speed_start}) and end ({speed_end}) speeds are symmetric"
        );

        for step in 0..100 {
            let phase = step as f32 / 100.0;
            let (left, cap) = bar_geometry(width, height, None, phase);
            assert!(left >= -1e-4, "phase {phase}: left {left} < 0");
            assert!(
                left + cap <= width + 1e-4,
                "phase {phase}: right {} > {width}",
                left + cap
            );
            assert!(cap >= height, "capsule stays at least pill-sized");
        }
    }

    #[test]
    fn step_toward_converges_to_target_without_overshooting() {
        let mut current = 0.0;
        for _ in 0..40 {
            current = step_toward(current, 0.75);
            assert!(current <= 0.75);
        }
        assert_eq!(current, 0.75);
    }
}
