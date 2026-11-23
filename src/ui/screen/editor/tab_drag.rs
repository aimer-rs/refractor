//! Horizontal drag-and-drop for open file tabs in the tab bar, unifying primary click (activate),
//! secondary click (tab context menu), and primary drag (reorder) in one pointer-capturing element.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use aimer::animation::AnimInstant;
use aimer::dnd::{
    AutoScrollPolicy, AutoScroller, DragAxis, DragOverlay, DragPayload, DragSession, ScrollTarget,
    ScrollViewport,
};
use aimer::events::element::ElementEvent;
use aimer::events::pointer::{PointerButton, PointerSource};
use aimer::input::gesture::{LONG_PRESS_DURATION, MOUSE_TAP_TOLERANCE, TAP_SLOP};
use aimer::modal::ModalScope;
use aimer::style::{LayoutSpacing, TextOverflow};
use aimer::{
    AnyElement, AnyWidget, BoxAlignment, BuildContext, Container, Drawable, Element, EventElement,
    EventResult, EventTreeRole, FollowUp, InteractionBounds, InteractionDisagreement,
    InteractionSource, LayoutElement, PointerKey, Rebuildable, ResolvedSize, Row as Line,
    ScrollController, Size, Text, Vec2d, Venus, VisitorElement, Widget, claim_pointer, list,
};

use crate::ui::components::icon::material;
use crate::ui::components::popup::Popup;
use crate::ui::file_icons::file_icon;
use crate::ui::fonts;
use crate::ui::theme::CodeTheme;

/// Payload carried while dragging an open file tab along the tab bar to reorder it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraggedTab {
    pub id: u64,
    pub index: usize,
    pub pinned: bool,
    pub name: String,
}

/// How far a mouse press must travel before turning into a drag instead of a click, in logical pixels.
const MOUSE_DRAG_THRESHOLD: f32 = 4.0;

/// Edge band width (in logical pixels) near the start/end of the tab bar that triggers auto-scrolling while dragging a tab.
const AUTO_SCROLL_EDGE: f32 = 48.0;
/// Maximum horizontal auto-scroll speed (in logical pixels per second) at the very edge of the tab bar.
const AUTO_SCROLL_MAX_VELOCITY: f32 = 540.0;
/// Polling interval for the tab bar's auto-scroll loop while a tab drag is active.
const AUTO_SCROLL_TICK: Duration = Duration::from_millis(16);

/// Drives bounded horizontal edge auto-scrolling of the tab bar while a tab drag is active.
pub struct TabAutoScroll {
    scroll: ScrollController,
    bounds: Rc<Cell<(Vec2d, Vec2d)>>,
    running: Rc<Cell<bool>>,
}

impl TabAutoScroll {
    pub fn new(scroll: ScrollController) -> Self {
        Self {
            scroll,
            bounds: Rc::new(Cell::new((Vec2d::ZERO, Vec2d::ZERO))),
            running: Rc::new(Cell::new(false)),
        }
    }

    /// Shared viewport bounds `(origin, size)` updated by [`TabBarViewport`].
    pub fn bounds(&self) -> Rc<Cell<(Vec2d, Vec2d)>> {
        Rc::clone(&self.bounds)
    }

    /// Policy used for horizontal tab-bar edge auto-scrolling.
    pub fn policy() -> AutoScrollPolicy {
        AutoScrollPolicy::new(AUTO_SCROLL_EDGE, AUTO_SCROLL_MAX_VELOCITY)
            .expect("auto-scroll edge and velocity are positive finite constants")
    }

    /// Starts the edge auto-scroll loop for the current tab drag if it is not already running.
    pub fn start(&self) {
        if self.running.replace(true) {
            return;
        }
        let Some(venus) = Venus::current() else {
            self.running.set(false);
            return;
        };
        let target = TabScrollTarget {
            scroll: self.scroll.clone(),
            bounds: Rc::clone(&self.bounds),
        };
        let running = Rc::clone(&self.running);
        venus.spawn(async move {
            let mut scroller = AutoScroller::new(Self::policy());
            let started = AnimInstant::now();
            while DragSession::with_payload(|_: &DraggedTab| ()).is_some() {
                if let Some(pos) = DragSession::position() {
                    scroller.tick_at(started.elapsed(), pos, &target);
                }
                let Some(venus) = Venus::current() else {
                    break;
                };
                let _ = venus.sleep(AUTO_SCROLL_TICK).await;
            }
            scroller.cancel();
            running.set(false);
        });
    }
}

struct TabScrollTarget {
    scroll: ScrollController,
    bounds: Rc<Cell<(Vec2d, Vec2d)>>,
}

impl ScrollTarget for TabScrollTarget {
    fn viewport(&self) -> ScrollViewport {
        let (origin, size) = self.bounds.get();
        let offset = self.scroll.offset();
        let max = self.scroll.max_extent();
        ScrollViewport::new(
            origin,
            size,
            Vec2d {
                x: offset.x,
                y: 0.0,
            },
            Vec2d { x: max.x, y: 0.0 },
        )
    }

    fn request_scroll(&self, delta: Vec2d) {
        if delta.x != 0.0 {
            let current = self.scroll.offset();
            let max = self.scroll.max_extent();
            let next_x = (current.x + delta.x).clamp(0.0, max.x);
            if (next_x - current.x).abs() > f32::EPSILON {
                self.scroll.jump_to(Vec2d {
                    x: next_x,
                    y: current.y,
                });
            }
        }
    }
}

/// Records the tab bar's window-space viewport `(origin, size)` so [`TabAutoScroll`] knows where its left and right edges are.
pub struct TabBarViewport {
    bounds: Rc<Cell<(Vec2d, Vec2d)>>,
    child: AnyWidget,
}

impl TabBarViewport {
    pub fn new(bounds: Rc<Cell<(Vec2d, Vec2d)>>, child: impl Widget + 'static) -> Self {
        Self {
            bounds,
            child: child.notype(),
        }
    }
}

impl Widget for TabBarViewport {
    fn to_element(self, ctx: &BuildContext) -> AnyElement {
        RawTabBarViewport {
            bounds: self.bounds,
            child: self.child.to_element(ctx),
        }
        .boxed()
    }

    fn name(&self) -> &'static str {
        "TabBarViewport"
    }
}

struct RawTabBarViewport {
    bounds: Rc<Cell<(Vec2d, Vec2d)>>,
    child: AnyElement,
}

impl RawTabBarViewport {
    fn record(&self, ctx: &BuildContext, size: ResolvedSize) {
        let scale = if ctx.scale > 0.0 { ctx.scale } else { 1.0 };
        let (abs_x, abs_y) = ctx.canvas.get_transform_translation();
        self.bounds.set((
            Vec2d {
                x: abs_x / scale,
                y: abs_y / scale,
            },
            Vec2d {
                x: size.width / scale,
                y: size.height / scale,
            },
        ));
    }
}

impl VisitorElement for RawTabBarViewport {
    fn visit_children<'a>(&'a self, visitor: &mut dyn FnMut(&'a dyn Element)) {
        visitor(self.child.as_ref());
    }

    fn visit_retained_children<'a>(&'a self, visitor: &mut dyn FnMut(usize, &'a dyn Element)) {
        visitor(0, self.child.as_ref());
    }

    fn event_children<'a>(&'a self, visitor: &mut dyn FnMut(&'a dyn Element)) {
        visitor(self.child.as_ref());
    }
}

impl Element for RawTabBarViewport {
    fn name(&self) -> &'static str {
        "TabBarViewport"
    }
}

impl EventElement for RawTabBarViewport {}

impl LayoutElement for RawTabBarViewport {
    #[inline]
    fn size(&self) -> Option<Size> {
        None
    }

    fn layout(&self, ctx: &BuildContext) -> ResolvedSize {
        let size = self.child.layout(ctx);
        self.record(ctx, size);
        size
    }

    fn computed_size(&self, ctx: &BuildContext) -> ResolvedSize {
        self.child.computed_size(ctx)
    }
}

impl Drawable for RawTabBarViewport {
    fn update(&self, ctx: &BuildContext) {
        let size = self.child.computed_size(ctx);
        self.record(ctx, size);
        self.child.update(ctx);
    }

    #[inline]
    fn can_paint(&self, _ctx: &BuildContext) -> bool {
        true
    }

    #[inline]
    fn paint(&self, _ctx: &BuildContext) {}

    fn sync_paint_state(&self, ctx: &BuildContext) -> bool {
        let size = self.child.computed_size(ctx);
        self.record(ctx, size);
        self.child.sync_paint_geometry(ctx);
        true
    }

    fn child_context<'a>(
        &self,
        ctx: &BuildContext<'a>,
        child: &dyn Element,
    ) -> Option<BuildContext<'a>> {
        std::ptr::eq(child, self.child.as_ref()).then(|| ctx.clone())
    }
}

impl Rebuildable for RawTabBarViewport {}

/// Floating pill drawn under the cursor while dragging a tab.
pub fn drag_feedback(tab: &DraggedTab, theme: CodeTheme) -> AnyWidget {
    Container::new()
        .fit_content()
        .decoration(Popup::frame(theme))
        .padding(
            LayoutSpacing::new()
                .left(10)
                .right(12)
                .top(6)
                .bottom(6),
        )
        .child(
            Line::new()
                .vertical_alignment(BoxAlignment::Center)
                .gaps(LayoutSpacing::all(7))
                .children(list![
                    material(file_icon(&tab.name), 16),
                    Text::new(tab.name.clone()).text_style(
                        fonts::ui()
                            .font_size(13)
                            .color(theme.text)
                            .text_overflow(TextOverflow::Clip),
                    ),
                ]),
        )
        .notype()
}

/// Handles primary tap (activate), secondary tap (context menu), and primary horizontal drag (reorder)
/// for a single tab's label area without an inner `GestureDetector` stealing pointer capture.
pub struct TabDragArea {
    data: Option<DraggedTab>,
    auto_scroll: Option<Rc<TabAutoScroll>>,
    theme: CodeTheme,
    on_tap: Rc<dyn Fn()>,
    on_right_tap: Rc<dyn Fn(Vec2d)>,
    child: AnyWidget,
}

impl TabDragArea {
    pub fn new(
        data: Option<DraggedTab>,
        auto_scroll: Option<Rc<TabAutoScroll>>,
        theme: CodeTheme,
        on_tap: impl Fn() + 'static,
        on_right_tap: impl Fn(Vec2d) + 'static,
        child: impl Widget + 'static,
    ) -> Self {
        Self {
            data,
            auto_scroll,
            theme,
            on_tap: Rc::new(on_tap),
            on_right_tap: Rc::new(on_right_tap),
            child: child.notype(),
        }
    }
}

impl Widget for TabDragArea {
    fn to_element(self, ctx: &BuildContext) -> AnyElement {
        RawTabDragArea {
            data: self.data,
            auto_scroll: self.auto_scroll,
            theme: self.theme,
            on_tap: self.on_tap,
            on_right_tap: self.on_right_tap,
            child: self.child.to_element(ctx),
            scope: ModalScope::capture(ctx),
            bounds: InteractionBounds::new(),
            press: RefCell::new(None),
            dragging: Cell::new(false),
        }
        .boxed()
    }

    fn name(&self) -> &'static str {
        "TabDragArea"
    }
}

#[derive(Clone, Copy)]
struct Press {
    pointer: PointerKey,
    button: PointerButton,
    at: Vec2d,
    when: AnimInstant,
}

pub struct RawTabDragArea {
    data: Option<DraggedTab>,
    auto_scroll: Option<Rc<TabAutoScroll>>,
    theme: CodeTheme,
    on_tap: Rc<dyn Fn()>,
    on_right_tap: Rc<dyn Fn(Vec2d)>,
    child: AnyElement,
    scope: ModalScope,
    bounds: InteractionBounds,
    press: RefCell<Option<Press>>,
    dragging: Cell<bool>,
}

impl RawTabDragArea {
    fn should_start_drag(&self, press: &Press, now_pos: Vec2d) -> bool {
        if self.data.is_none() || press.button != PointerButton::Primary {
            return false;
        }
        let dx = now_pos.x - press.at.x;
        let dy = now_pos.y - press.at.y;
        let dist_sq = dx * dx + dy * dy;
        match press.pointer.source {
            PointerSource::Mouse => dist_sq >= MOUSE_DRAG_THRESHOLD * MOUSE_DRAG_THRESHOLD,
            _ => dist_sq >= TAP_SLOP * TAP_SLOP && press.when.elapsed() >= LONG_PRESS_DURATION,
        }
    }

    fn within_tap_tolerance(press: &Press, up_pos: Vec2d) -> bool {
        let dx = up_pos.x - press.at.x;
        let dy = up_pos.y - press.at.y;
        let tol = match press.pointer.source {
            PointerSource::Mouse => MOUSE_TAP_TOLERANCE,
            _ => TAP_SLOP,
        };
        dx * dx + dy * dy <= tol * tol
    }

    fn begin_drag(&self, press: &Press) -> bool {
        let Some(tab) = self.data.clone() else {
            return false;
        };
        if !DragSession::begin(press.pointer, DragPayload::new(tab.clone()), press.at) {
            return false;
        }
        claim_pointer(press.pointer);
        let theme = self.theme;
        let grab_offset = Vec2d { x: 16.0, y: 14.0 };
        DragOverlay::show_in(
            self.scope.clone(),
            Some(Rc::new(move || drag_feedback(&tab, theme))),
            grab_offset,
            press.at,
            DragAxis::Horizontal,
        );
        self.dragging.set(true);
        if let Some(auto_scroll) = &self.auto_scroll {
            auto_scroll.start();
        }
        true
    }

    fn abandon(&self, pointer: Option<PointerKey>) {
        let owns = self
            .press
            .borrow()
            .as_ref()
            .is_some_and(|press| pointer.is_none_or(|p| press.pointer == p));
        if !owns {
            return;
        }
        if let Some(press) = self.press.borrow_mut().take()
            && self.dragging.replace(false)
        {
            DragSession::cancel(press.pointer);
            DragOverlay::spring_back();
        }
    }
}

impl VisitorElement for RawTabDragArea {
    fn visit_children<'a>(&'a self, visitor: &mut dyn FnMut(&'a dyn Element)) {
        visitor(self.child.as_ref());
    }

    fn visit_retained_children<'a>(&'a self, visitor: &mut dyn FnMut(usize, &'a dyn Element)) {
        visitor(0, self.child.as_ref());
    }

    fn event_children<'a>(&'a self, visitor: &mut dyn FnMut(&'a dyn Element)) {
        visitor(self.child.as_ref());
    }
}

impl Element for RawTabDragArea {
    fn name(&self) -> &'static str {
        "TabDragArea"
    }
}

impl EventElement for RawTabDragArea {
    fn event_tree_role(&self) -> EventTreeRole {
        EventTreeRole::IndexedHitTestBoundary
    }

    fn on_event(&self, event: &ElementEvent) -> EventResult {
        match event {
            ElementEvent::PointerDown(info) => {
                if !self.bounds.is_inside(info.x(), info.y()) {
                    return EventResult::ignored();
                }
                if !matches!(
                    info.button,
                    PointerButton::Primary | PointerButton::Secondary
                ) {
                    return EventResult::ignored();
                }
                if DragSession::is_active() || self.press.borrow().is_some() {
                    return EventResult::ignored();
                }
                let pointer = PointerKey::new(info.source, info.id);
                *self.press.borrow_mut() = Some(Press {
                    pointer,
                    button: info.button,
                    at: info.pos,
                    when: AnimInstant::now(),
                });
                EventResult::consumed().with_pointer_capture(pointer)
            }

            ElementEvent::PointerMove(info) => {
                let pointer = PointerKey::new(info.source, info.id);
                let mut press = self.press.borrow_mut();
                let Some(press) = press.as_mut().filter(|p| p.pointer == pointer) else {
                    return EventResult::ignored();
                };

                if !self.dragging.get() {
                    if !self.should_start_drag(press, info.pos) {
                        return EventResult::ignored();
                    }
                    if !self.begin_drag(press) {
                        return EventResult::ignored();
                    }
                }

                DragSession::update(pointer, info.pos);
                EventResult::consumed()
                    .with_redraw()
                    .with_follow_up(FollowUp::DragOver)
            }

            ElementEvent::PointerUp(info) => {
                let pointer = PointerKey::new(info.source, info.id);
                let press = self
                    .press
                    .borrow_mut()
                    .take_if(|p| p.pointer == pointer);
                let Some(press) = press else {
                    return EventResult::ignored();
                };
                if self.dragging.replace(false) {
                    DragOverlay::resolve_on_next_frame(None);
                    return EventResult::consumed()
                        .with_redraw()
                        .with_pointer_release(pointer)
                        .with_follow_up(FollowUp::DragDrop);
                }
                if Self::within_tap_tolerance(&press, info.pos) {
                    match press.button {
                        PointerButton::Primary => (self.on_tap)(),
                        PointerButton::Secondary => (self.on_right_tap)(info.pos),
                        _ => {}
                    }
                }
                EventResult::consumed()
                    .with_redraw()
                    .with_pointer_release(pointer)
            }

            ElementEvent::PointerExited(source, id) => {
                let pointer = PointerKey::new(*source, *id);
                self.abandon(Some(pointer));
                EventResult::ignored().with_pointer_release(pointer)
            }

            ElementEvent::Cancel => {
                self.abandon(None);
                EventResult::ignored()
            }

            _ => EventResult::ignored(),
        }
    }
}

impl LayoutElement for RawTabDragArea {
    #[inline]
    fn size(&self) -> Option<Size> {
        None
    }

    fn layout(&self, ctx: &BuildContext) -> ResolvedSize {
        let size = self.child.layout(ctx);
        let (abs_x, abs_y) = ctx.canvas.get_transform_translation();
        self.bounds
            .save(ctx.scale, abs_x, abs_y, size.width, size.height);
        size
    }

    fn computed_size(&self, ctx: &BuildContext) -> ResolvedSize {
        self.child.computed_size(ctx)
    }

    fn pos_start_end(&self) -> Option<(Vec2d, Vec2d)> {
        self.bounds.pos_start_end()
    }
}

impl Drawable for RawTabDragArea {
    #[inline]
    fn interaction_size(&self, ctx: &BuildContext) -> Option<ResolvedSize> {
        Some(self.child.computed_size(ctx))
    }

    #[inline]
    fn adopt_interaction_source(&self, source: InteractionSource) {
        self.bounds.adopt(source);
    }

    #[inline]
    fn interaction_disagreement(&self) -> Option<InteractionDisagreement> {
        self.bounds.disagreement("TabDragArea")
    }

    fn update(&self, ctx: &BuildContext) {
        let (abs_x, abs_y) = ctx.canvas.get_transform_translation();
        let size = self.child.computed_size(ctx);
        self.bounds
            .save(ctx.scale, abs_x, abs_y, size.width, size.height);
        self.child.update(ctx);
    }

    #[inline]
    fn can_paint(&self, _ctx: &BuildContext) -> bool {
        true
    }

    #[inline]
    fn paint(&self, _ctx: &BuildContext) {}

    fn sync_paint_state(&self, ctx: &BuildContext) -> bool {
        let (abs_x, abs_y) = ctx.canvas.get_transform_translation();
        let size = self.child.computed_size(ctx);
        self.bounds
            .save(ctx.scale, abs_x, abs_y, size.width, size.height);
        self.child.sync_paint_geometry(ctx);
        true
    }

    fn child_context<'a>(
        &self,
        ctx: &BuildContext<'a>,
        child: &dyn Element,
    ) -> Option<BuildContext<'a>> {
        std::ptr::eq(child, self.child.as_ref()).then(|| ctx.clone())
    }
}

impl Rebuildable for RawTabDragArea {
    #[inline]
    fn option_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn adopt_runtime_state_from(&self, old: &dyn Element) {
        let Some(old) = old
            .option_any()
            .and_then(|value| value.downcast_ref::<Self>())
        else {
            return;
        };
        *self.press.borrow_mut() = old.press.borrow_mut().take();
        self.dragging.set(old.dragging.replace(false));
    }
}

impl Drop for RawTabDragArea {
    fn drop(&mut self) {
        self.abandon(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aimer::dnd::AutoScrollEdges;

    struct FakeHorizontalTarget {
        origin_x: f32,
        width: f32,
        offset_x: Cell<f32>,
        max_x: f32,
    }

    impl ScrollTarget for FakeHorizontalTarget {
        fn viewport(&self) -> ScrollViewport {
            ScrollViewport::new(
                Vec2d {
                    x: self.origin_x,
                    y: 0.0,
                },
                Vec2d {
                    x: self.width,
                    y: 38.0,
                },
                Vec2d {
                    x: self.offset_x.get(),
                    y: 0.0,
                },
                Vec2d {
                    x: self.max_x,
                    y: 0.0,
                },
            )
        }

        fn request_scroll(&self, delta: Vec2d) {
            let next = (self.offset_x.get() + delta.x).clamp(0.0, self.max_x);
            self.offset_x.set(next);
        }
    }

    #[test]
    fn tab_auto_scroll_policy_scrolls_near_right_and_left_edges_and_stays_idle_in_center() {
        let target = FakeHorizontalTarget {
            origin_x: 200.0,
            width: 800.0,
            offset_x: Cell::new(100.0),
            max_x: 500.0,
        };
        let mut scroller = AutoScroller::new(TabAutoScroll::policy());

        // First tick establishes baseline.
        let baseline = scroller.tick_at(
            Duration::from_millis(0),
            Vec2d { x: 600.0, y: 18.0 },
            &target,
        );
        assert!(baseline.is_idle());

        // Center of the tab bar: no scrolling.
        let center = scroller.tick_at(
            Duration::from_millis(16),
            Vec2d { x: 600.0, y: 18.0 },
            &target,
        );
        assert!(center.is_idle());
        assert_eq!(target.offset_x.get(), 100.0);

        // Near right edge (within 48px of 1000.0): scrolls right (positive delta).
        let right = scroller.tick_at(
            Duration::from_millis(32),
            Vec2d { x: 990.0, y: 18.0 },
            &target,
        );
        assert!(right.edges.contains(AutoScrollEdges::RIGHT));
        assert!(target.offset_x.get() > 100.0);

        // Near left edge (within 48px of 200.0): scrolls left (negative delta).
        let before_left = target.offset_x.get();
        let left = scroller.tick_at(
            Duration::from_millis(48),
            Vec2d { x: 205.0, y: 18.0 },
            &target,
        );
        assert!(left.edges.contains(AutoScrollEdges::LEFT));
        assert!(target.offset_x.get() < before_left);
    }
}
