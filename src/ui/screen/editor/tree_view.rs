//! Shared collapsible tree-view primitives used by both the sidebar file tree and the Git changes tree:
//! animated vertical expand/collapse clipping, rotating disclosure chevrons, and per-level colored indent guides.

use crate::ui::components::icon::icon;
use crate::ui::components::pad::pad;
use crate::ui::theme::CodeTheme;
use aimer::animation::{AnimInstant, AnimationController, RotationTransition, curves};
use aimer::decoration::Decoration;
use aimer::events::element::ElementEvent;
use aimer::events::window::request_animation_frame;
use aimer::style::BorderRadius;
use aimer::{
    AnyElement, AnyWidget, BoxAlignment, BuildContext, Color, Container, Dimension, Drawable,
    Element, EventElement, EventResult, EventTreeRole, Expanded, Icons, InteractionBounds,
    InteractionDisagreement, InteractionSource, LayoutElement, NumberExt, Rebuildable,
    ResolvedSize, Row as Line, SizedBox, Vec2d, VisitorElement, Widget, mark_paint_damage_full,
};
use std::cell::Cell;
use std::time::Duration;

/// How far each level of a sidebar tree is indented, in logical pixels.
pub(super) const INDENT: u32 = 12;
/// How tall one tree row is, in logical pixels.
pub(super) const ROW_HEIGHT: u32 = 24;
/// Duration of the folder expand/collapse reveal and chevron rotation animations.
pub(super) const COLLAPSE_DURATION: Duration = Duration::from_millis(250);
/// Opacity applied to each colored vertical indent guide bar.
pub(super) const GUIDE_ALPHA: f32 = 0.45;

/// Animation controllers paired for a collapsible tree folder: the rotating chevron and the vertical reveal body.
pub(super) struct FolderAnimation {
    pub(super) chevron: AnimationController,
    pub(super) body: AnimationController,
}

impl FolderAnimation {
    /// Creates controllers settled at `expanded`.
    pub(super) fn new(expanded: bool) -> Self {
        let chevron = AnimationController::new(COLLAPSE_DURATION, curves::EASE_IN_OUT);
        if !expanded {
            chevron.set_value(1.0);
        }
        let body = AnimationController::new(COLLAPSE_DURATION, curves::EASE_IN_OUT);
        if expanded {
            body.set_value(1.0);
        }
        Self { chevron, body }
    }

    /// Animates the folder open.
    pub(super) fn expand(&self) {
        self.body.forward();
        self.chevron.reverse();
    }

    /// Animates the folder shut.
    pub(super) fn collapse(&self) {
        self.body.reverse();
        self.chevron.forward();
    }

    /// Snaps both controllers to the closed state without animating.
    pub(super) fn snap_closed(&self) {
        self.body.set_value(0.0);
        self.chevron.set_value(1.0);
    }
}

/// Colors for the vertical indent guide bars of a row at `depth`.
pub(super) fn guide_colors_for_depth(depth: usize, theme: &CodeTheme) -> Vec<Color> {
    (0..depth)
        .map(|level| theme.indent_guide(level).with_alpha(GUIDE_ALPHA))
        .collect()
}

/// One 12px indent column with a 1px vertical colored guide bar centered under the parent folder's chevron.
pub(super) fn indent_guide(color: Color) -> AnyWidget {
    Container::new()
        .width(INDENT)
        .height(ROW_HEIGHT)
        .padding(pad(0, 5, 0, 6))
        .child(
            Container::new()
                .width(1)
                .height(ROW_HEIGHT)
                .color(color)
                .child(SizedBox::new()),
        )
        .notype()
}

/// The rotating disclosure chevron for a tree folder row.
pub(super) fn chevron(controller: AnimationController, theme: &CodeTheme) -> AnyWidget {
    Container::new()
        .width(14)
        .height(14)
        .box_child(
            RotationTransition::new(controller, icon(Icons::CHEVRON_DOWN, 14, theme.muted))
                .turn_range(0.0, -0.25),
        )
}

/// Horizontal spacer matching [`chevron`]'s width for file rows.
pub(super) fn chevron_spacer() -> AnyWidget {
    SizedBox::new().width(14).notype()
}

/// Standard rounded tree-row container with per-level colored vertical indent guides at `depth`.
pub(super) fn row_shell<T: Widget + 'static>(
    depth: usize,
    background: Color,
    theme: &CodeTheme,
    content: T,
) -> impl Widget + 'static {
    let row_line: AnyWidget = if depth == 0 {
        content.notype()
    } else {
        let guide_colors = guide_colors_for_depth(depth, theme);
        let mut parts: Vec<AnyWidget> = Vec::with_capacity(guide_colors.len() + 1);
        for color in guide_colors {
            parts.push(indent_guide(color));
        }
        parts.push(Expanded::new().child(content).notype());
        Line::new()
            .vertical_alignment(BoxAlignment::Center)
            .children(parts)
            .notype()
    };
    Container::new()
        .height(ROW_HEIGHT)
        .width(Dimension::Percent(100.0))
        .padding(pad(0, 14, 0, 8))
        .decoration([BorderRadius::all(4.px()).notype(), background.notype()])
        .child(row_line)
}

#[inline]
fn animation_progress(value: f32) -> f32 {
    value
        .is_finite()
        .then_some(value.clamp(0.0, 1.0))
        .unwrap_or(0.0)
}

#[inline]
fn nonnegative_extent(value: f32) -> f32 {
    value.is_finite().then_some(value.max(0.0)).unwrap_or(0.0)
}

#[inline]
fn collapsed_height(natural_height: f32, progress: f32) -> f32 {
    nonnegative_extent(natural_height) * animation_progress(progress)
}

/// Retained layout wrapper (mirroring `aimer_collapsible`'s `AnimatedCollapse`) that animates a folder's
/// child subtree vertical extent while clipping it to that extent.
#[derive(Clone)]
pub(super) struct AnimatedCollapse<T: Widget + 'static> {
    controller: AnimationController,
    child: T,
}

impl<T: Widget + 'static> AnimatedCollapse<T> {
    #[inline]
    pub(super) fn new(controller: AnimationController, child: T) -> Self {
        Self { controller, child }
    }
}

impl<T: Widget + 'static> Widget for AnimatedCollapse<T> {
    fn to_element(self, ctx: &BuildContext) -> AnyElement {
        AnimatedCollapseElement {
            child: self.child.to_element(ctx),
            controller: self.controller,
            last_progress: Cell::new(None),
            bounds: InteractionBounds::new(),
        }
        .boxed()
    }

    fn name(&self) -> &'static str {
        "AnimatedCollapse"
    }
}

struct AnimatedCollapseElement {
    child: AnyElement,
    controller: AnimationController,
    last_progress: Cell<Option<u32>>,
    bounds: InteractionBounds,
}

// SAFETY: Aimer renders and mutates retained elements on one UI thread.
unsafe impl Send for AnimatedCollapseElement {}
unsafe impl Sync for AnimatedCollapseElement {}

impl AnimatedCollapseElement {
    #[inline]
    fn progress(&self) -> f32 {
        animation_progress(self.controller.tick(AnimInstant::now()))
    }

    #[inline]
    fn is_event_visible(&self) -> bool {
        animation_progress(self.controller.value()) > 0.0
    }

    fn natural_size(&self, ctx: &BuildContext) -> ResolvedSize {
        let mut natural_ctx = ctx.clone();
        natural_ctx.box_constraint.min_height = 0.0;
        natural_ctx.box_constraint.max_height = f32::MAX;
        natural_ctx.parent_size.height = f32::MAX;
        natural_ctx.set_visible_rect(None);
        self.child.computed_size(&natural_ctx)
    }

    #[inline]
    fn child_context<'a>(&self, ctx: &BuildContext<'a>, natural: ResolvedSize) -> BuildContext<'a> {
        let mut child_ctx = ctx.clone();
        child_ctx.parent_size = natural;
        child_ctx.box_constraint.min_width = 0.0;
        child_ctx.box_constraint.min_height = 0.0;
        child_ctx.box_constraint.max_width = natural.width;
        child_ctx.box_constraint.max_height = natural.height;
        child_ctx
    }

    fn current_size(&self, ctx: &BuildContext) -> ResolvedSize {
        let natural = self.natural_size(ctx);
        ResolvedSize {
            width: nonnegative_extent(natural.width),
            height: collapsed_height(natural.height, self.progress()),
        }
    }

    fn advance_layout(&self, progress: f32) -> bool {
        let bits = progress.to_bits();
        let moved = matches!(self.last_progress.replace(Some(bits)), Some(last) if last != bits);
        let active = self.controller.is_animating();
        if moved || active {
            self.child.invalidate_layout();
            request_animation_frame();
        }
        moved || active
    }

    fn update_bounds(&self, ctx: &BuildContext, size: ResolvedSize) {
        let scale = ctx.scale;
        if !scale.is_finite() || scale <= 0.0 {
            return;
        }
        let width = size.width.max(0.0);
        let height = size.height.max(0.0);
        if !width.is_finite() || !height.is_finite() {
            return;
        }
        let (start_x, start_y) = ctx.canvas.get_transform_translation();
        self.bounds.save(scale, start_x, start_y, width, height);
    }
}

impl Drawable for AnimatedCollapseElement {
    #[inline]
    fn reusable_walk(&self) -> bool {
        !self.controller.is_animating()
    }

    fn interaction_size(&self, ctx: &BuildContext) -> Option<ResolvedSize> {
        Some(self.current_size(ctx))
    }

    #[inline]
    fn adopt_interaction_source(&self, source: InteractionSource) {
        self.bounds.adopt(source);
    }

    #[inline]
    fn interaction_disagreement(&self) -> Option<InteractionDisagreement> {
        self.bounds.disagreement("AnimatedCollapse")
    }

    fn update(&self, ctx: &BuildContext) {
        let progress = self.progress();
        let natural = self.natural_size(ctx);
        let size = ResolvedSize {
            width: nonnegative_extent(natural.width),
            height: collapsed_height(natural.height, progress),
        };
        self.update_bounds(ctx, size);

        if self.advance_layout(progress) {
            mark_paint_damage_full();
        }

        if size.width > 0.0 && size.height > 0.0 {
            self.child.update(&self.child_context(ctx, natural));
        }
    }

    #[inline]
    fn can_paint(&self, _ctx: &BuildContext) -> bool {
        true
    }

    #[inline]
    fn paint(&self, _ctx: &BuildContext) {}

    fn sync_paint_state(&self, ctx: &BuildContext) -> bool {
        let progress = self.progress();
        let natural = self.natural_size(ctx);
        let size = ResolvedSize {
            width: nonnegative_extent(natural.width),
            height: collapsed_height(natural.height, progress),
        };
        self.update_bounds(ctx, size);
        self.child
            .sync_paint_geometry(&self.child_context(ctx, natural));
        true
    }

    fn update_retained(&self, ctx: &BuildContext) {
        let progress = self.progress();
        let natural = self.natural_size(ctx);
        let size = ResolvedSize {
            width: nonnegative_extent(natural.width),
            height: collapsed_height(natural.height, progress),
        };
        self.update_bounds(ctx, size);
        self.advance_layout(progress);
        if size.width > 0.0 && size.height > 0.0 {
            self.child.update(&self.child_context(ctx, natural));
        }
    }

    fn retained_clip(&self, ctx: &BuildContext) -> Option<aimer::canvas::Rect> {
        let natural = self.natural_size(ctx);
        let size = ResolvedSize {
            width: nonnegative_extent(natural.width),
            height: collapsed_height(natural.height, self.progress()),
        };
        let scale = if ctx.scale.is_finite() && ctx.scale > 0.0 {
            ctx.scale
        } else {
            1.0
        };
        Some(aimer::canvas::Rect::new(
            0.0,
            0.0,
            size.width / scale,
            size.height / scale,
        ))
    }

    fn paint_bounds(&self, ctx: &BuildContext) -> Option<ResolvedSize> {
        let natural = self.natural_size(ctx);
        Some(ResolvedSize {
            width: nonnegative_extent(natural.width),
            height: collapsed_height(natural.height, self.progress()),
        })
    }

    fn child_context<'a>(
        &self,
        ctx: &BuildContext<'a>,
        child: &dyn Element,
    ) -> Option<BuildContext<'a>> {
        std::ptr::eq(child, self.child.as_ref())
            .then(|| self.child_context(ctx, self.natural_size(ctx)))
    }

    fn sync_paint_geometry(&self, ctx: &BuildContext) {
        let progress = animation_progress(self.controller.value());
        let natural = self.natural_size(ctx);
        let size = ResolvedSize {
            width: nonnegative_extent(natural.width),
            height: collapsed_height(natural.height, progress),
        };
        self.update_bounds(ctx, size);
        self.child
            .sync_paint_geometry(&self.child_context(ctx, natural));
    }
}

impl VisitorElement for AnimatedCollapseElement {
    fn visit_children<'a>(&'a self, visitor: &mut dyn FnMut(&'a dyn Element)) {
        visitor(self.child.as_ref());
    }

    fn structural_children<'a>(&'a self, visitor: &mut dyn FnMut(&'a dyn Element)) {
        visitor(self.child.as_ref());
    }

    fn event_children<'a>(&'a self, visitor: &mut dyn FnMut(&'a dyn Element)) {
        if self.is_event_visible() {
            visitor(self.child.as_ref());
        }
    }

    fn focus_children<'a>(&'a self, visitor: &mut dyn FnMut(&'a dyn Element)) {
        if self.is_event_visible() {
            visitor(self.child.as_ref());
        }
    }
}

impl Element for AnimatedCollapseElement {
    fn name(&self) -> &'static str {
        "AnimatedCollapseElement"
    }
}

impl EventElement for AnimatedCollapseElement {
    fn event_tree_role(&self) -> EventTreeRole {
        EventTreeRole::IndexedTarget
    }

    fn on_event(&self, event: &ElementEvent) -> EventResult {
        if self.is_event_visible() {
            self.child.on_event(event)
        } else {
            EventResult::ignored()
        }
    }
}

impl Rebuildable for AnimatedCollapseElement {
    #[inline]
    fn option_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn adopt_runtime_state_from(&self, old: &dyn Element) {
        if let Some(old) = old
            .option_any()
            .and_then(|any| any.downcast_ref::<Self>())
        {
            self.last_progress.set(old.last_progress.get());
        }
    }

    fn rebuild_if_dirty(&self, ctx: &BuildContext) {
        self.child.rebuild_if_dirty(ctx);
    }
}

impl LayoutElement for AnimatedCollapseElement {
    fn event_tree_bounds(&self) -> Option<(Vec2d, Vec2d)> {
        None
    }

    fn computed_size(&self, ctx: &BuildContext) -> ResolvedSize {
        let natural = self.natural_size(ctx);
        ResolvedSize {
            width: nonnegative_extent(natural.width),
            height: collapsed_height(natural.height, self.progress()),
        }
    }

    fn content_size(&self, ctx: &BuildContext) -> ResolvedSize {
        self.computed_size(ctx)
    }

    fn get_size_from_child(&self) -> Option<aimer::Size> {
        self.child.get_size_from_child()
    }

    fn is_layout_stable(&self) -> bool {
        false
    }

    fn invalidate_layout(&self) {
        self.child.invalidate_layout();
    }

    fn pos_start_end(&self) -> Option<(Vec2d, Vec2d)> {
        self.bounds.pos_start_end()
    }
}
