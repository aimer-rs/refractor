//! Drag-and-drop support for the file tree: dragging files and folders between directories, with a floating feedback pill and
//! spring-loaded auto-expansion when hovering over a collapsed folder.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use aimer::animation::AnimInstant;
use aimer::dnd::{DragAxis, DragOverlay, DragPayload, DragSession};
use aimer::events::element::ElementEvent;
use aimer::events::pointer::{PointerButton, PointerSource};
use aimer::input::gesture::{LONG_PRESS_DURATION, MOUSE_TAP_TOLERANCE, TAP_SLOP};
use aimer::modal::ModalScope;
use aimer::style::{LayoutSpacing, TextOverflow};
use aimer::{
    AnyElement, AnyWidget, BoxAlignment, BuildContext, Container, Drawable, Element, EventElement,
    EventResult, EventTreeRole, FollowUp, InteractionBounds, InteractionDisagreement,
    InteractionSource, LayoutElement, PointerKey, ProviderHandle, Rebuildable, ResolvedSize,
    Row as Line, Size, Text, Vec2d, Venus, VisitorElement, Widget, claim_pointer, list,
};

use crate::ui::components::icon::material;
use crate::ui::components::popup::Popup;
use crate::ui::file_icons::{file_icon, folder_icon};
use crate::ui::fonts;
use crate::ui::theme::CodeTheme;
use crate::model::workspace::{Workspace, notifier, refresh_status};

/// Payload carried while dragging one or more file or folder rows in the file tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraggedEntries {
    pub paths: Vec<PathBuf>,
    pub name: String,
    pub is_dir: bool,
}

/// How far a mouse press must travel before turning into a drag instead of a click, in logical pixels.
const MOUSE_DRAG_THRESHOLD: f32 = 4.0;

/// How long a dragged item must hover over a collapsed folder before the folder automatically expands.
pub const AUTO_EXPAND_DELAY: Duration = Duration::from_millis(800);

/// Spring-loaded folder timer: expands a collapsed folder if a drag hovers over it for [`AUTO_EXPAND_DELAY`], and cancels cleanly
/// if the pointer leaves or the drag ends before then.
#[derive(Default)]
pub struct AutoExpandTimer {
    token: Rc<Cell<u64>>,
    armed: Rc<Cell<bool>>,
}

impl AutoExpandTimer {
    /// Arms auto-expansion for `dir` if not already armed.
    pub fn arm(&self, dir: PathBuf, store: ProviderHandle<Workspace>) {
        if self.armed.replace(true) {
            return;
        }
        let next = self.token.get().wrapping_add(1);
        self.token.set(next);
        let Some(venus) = Venus::current() else {
            return;
        };
        let token = Rc::clone(&self.token);
        let armed = Rc::clone(&self.armed);
        let waited = venus.sleep(AUTO_EXPAND_DELAY);
        venus.spawn(async move {
            let _ = waited.await;
            if token.get() == next && DragSession::is_active() {
                armed.set(false);
                store.update(|space| {
                    space.expand(&dir);
                    space.refresh();
                });
            }
        });
    }

    /// Cancels any pending auto-expansion.
    pub fn cancel(&self) {
        if self.armed.replace(false) {
            self.token.set(self.token.get().wrapping_add(1));
        }
    }
}

impl Drop for AutoExpandTimer {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Moves `dragged` entries into `dest_dir` inside `store`, clearing drag state, updating the status bar on error, and refreshing
/// git status on success.
pub fn accept_drop(dragged: DraggedEntries, dest_dir: PathBuf, store: &ProviderHandle<Workspace>) {
    let notify = notifier(store);
    let mut ok = false;
    store.update(|space| {
        space.clear_dragging();
        match space.move_paths_to(&dragged.paths, &dest_dir, notify) {
            Ok(_) => {
                space.message = None;
                ok = true;
            }
            Err(error) => {
                space.message = Some(format!("Could not move: {error}"));
            }
        }
    });
    if ok {
        refresh_status(store);
    }
}

/// The floating feedback pill drawn under the cursor while dragging one or more file or folder rows.
pub fn drag_feedback(entries: &DraggedEntries, theme: CodeTheme) -> AnyWidget {
    let kind = material(
        if entries.is_dir {
            folder_icon(&entries.name)
        } else {
            file_icon(&entries.name)
        },
        16,
    );
    let label = if entries.paths.len() > 1 {
        format!("{} (+{})", entries.name, entries.paths.len() - 1)
    } else {
        entries.name.clone()
    };
    Container::new()
        .fit_content()
        .decoration(Popup::frame(theme))
        .padding(
            LayoutSpacing::new()
                .left(10)
                .right(12)
                .top(5)
                .bottom(5),
        )
        .child(
            Line::new()
                .vertical_alignment(BoxAlignment::Center)
                .gaps(LayoutSpacing::all(6))
                .children(list![
                    kind,
                    Text::new(label).text_style(
                        fonts::ui()
                            .font_size(13)
                            .color(theme.text)
                            .text_overflow(TextOverflow::Clip),
                    ),
                ]),
        )
        .boxed()
}

/// Combines primary click, secondary click (context menu), and primary drag-and-drop on one file-tree row so pointer capture never
/// conflicts between clicking and dragging.
pub struct TreeRowInteraction<W> {
    data: Rc<dyn Fn() -> DraggedEntries>,
    feedback: Rc<dyn Fn(&DraggedEntries) -> AnyWidget>,
    on_drag_state: Rc<dyn Fn(Option<Vec<PathBuf>>)>,
    on_tap: Rc<dyn Fn()>,
    on_right_tap: Rc<dyn Fn(Vec2d)>,
    child: W,
}

impl<W: Widget + 'static> TreeRowInteraction<W> {
    pub fn new(
        data: impl Fn() -> DraggedEntries + 'static,
        feedback: impl Fn(&DraggedEntries) -> AnyWidget + 'static,
        on_drag_state: impl Fn(Option<Vec<PathBuf>>) + 'static,
        on_tap: impl Fn() + 'static,
        on_right_tap: impl Fn(Vec2d) + 'static,
        child: W,
    ) -> Self {
        Self {
            data: Rc::new(data),
            feedback: Rc::new(feedback),
            on_drag_state: Rc::new(on_drag_state),
            on_tap: Rc::new(on_tap),
            on_right_tap: Rc::new(on_right_tap),
            child,
        }
    }
}

impl<W: Widget + 'static> Widget for TreeRowInteraction<W> {
    fn to_element(self, ctx: &BuildContext) -> AnyElement {
        RawTreeRowInteraction {
            child: self.child.to_element(ctx),
            data: self.data,
            feedback: self.feedback,
            on_drag_state: self.on_drag_state,
            on_tap: self.on_tap,
            on_right_tap: self.on_right_tap,
            scope: ModalScope::capture(ctx),
            bounds: InteractionBounds::new(),
            press: RefCell::new(None),
            dragging: Cell::new(false),
        }
        .boxed()
    }

    fn name(&self) -> &'static str {
        "TreeRowInteraction"
    }
}

#[derive(Clone, Copy)]
struct Press {
    pointer: PointerKey,
    button: PointerButton,
    at: Vec2d,
    when: AnimInstant,
}

struct RawTreeRowInteraction {
    child: AnyElement,
    data: Rc<dyn Fn() -> DraggedEntries>,
    feedback: Rc<dyn Fn(&DraggedEntries) -> AnyWidget>,
    on_drag_state: Rc<dyn Fn(Option<Vec<PathBuf>>)>,
    on_tap: Rc<dyn Fn()>,
    on_right_tap: Rc<dyn Fn(Vec2d)>,
    scope: ModalScope,
    bounds: InteractionBounds,
    press: RefCell<Option<Press>>,
    dragging: Cell<bool>,
}

impl RawTreeRowInteraction {
    fn distance(a: Vec2d, b: Vec2d) -> f32 {
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        (dx * dx + dy * dy).sqrt()
    }

    fn should_start_drag(press: &Press, pos: Vec2d) -> bool {
        if press.button != PointerButton::Primary {
            return false;
        }
        let dist = Self::distance(press.at, pos);
        match press.pointer.source {
            PointerSource::Mouse => dist >= MOUSE_DRAG_THRESHOLD,
            PointerSource::Touch => {
                dist >= TAP_SLOP
                    && AnimInstant::now().duration_since(press.when) >= LONG_PRESS_DURATION
            }
        }
    }

    fn within_tap_tolerance(press: &Press, pos: Vec2d) -> bool {
        let limit = match press.pointer.source {
            PointerSource::Mouse => MOUSE_TAP_TOLERANCE,
            PointerSource::Touch => TAP_SLOP,
        };
        Self::distance(press.at, pos) <= limit
    }

    fn begin_drag(&self, press: &Press) -> bool {
        let entries = (self.data)();
        if entries.paths.is_empty() {
            return false;
        }
        let dragging_paths = entries.paths.clone();
        if !DragSession::begin(press.pointer, DragPayload::new(entries.clone()), press.at) {
            return false;
        }
        claim_pointer(press.pointer);
        let grab_offset = Vec2d { x: 16.0, y: 12.0 };
        let feedback_fn = Rc::clone(&self.feedback);
        let feedback_entries = entries;
        DragOverlay::show_in(
            self.scope.clone(),
            Some(Rc::new(move || feedback_fn(&feedback_entries))),
            grab_offset,
            press.at,
            DragAxis::Free,
        );
        self.dragging.set(true);
        (self.on_drag_state)(Some(dragging_paths));
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
            (self.on_drag_state)(None);
        }
    }
}

impl VisitorElement for RawTreeRowInteraction {
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

impl Element for RawTreeRowInteraction {
    fn name(&self) -> &'static str {
        "TreeRowInteraction"
    }
}

impl EventElement for RawTreeRowInteraction {
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
                    if !Self::should_start_drag(press, info.pos) {
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
                    let end = Rc::clone(&self.on_drag_state);
                    DragOverlay::resolve_on_next_frame(Some(Rc::new(move |_| end(None))));
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

impl LayoutElement for RawTreeRowInteraction {
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

impl Drawable for RawTreeRowInteraction {
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
        self.bounds.disagreement("TreeRowInteraction")
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

impl Rebuildable for RawTreeRowInteraction {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_drag_requires_moving_past_threshold_and_only_for_primary_button() {
        let press = Press {
            pointer: PointerKey::new(PointerSource::Mouse, 0),
            button: PointerButton::Primary,
            at: Vec2d { x: 10.0, y: 10.0 },
            when: AnimInstant::now(),
        };
        assert!(!RawTreeRowInteraction::should_start_drag(
            &press,
            Vec2d { x: 12.0, y: 10.0 }
        ));
        assert!(RawTreeRowInteraction::should_start_drag(
            &press,
            Vec2d { x: 15.0, y: 10.0 }
        ));

        let secondary = Press {
            button: PointerButton::Secondary,
            ..press
        };
        assert!(!RawTreeRowInteraction::should_start_drag(
            &secondary,
            Vec2d { x: 50.0, y: 50.0 }
        ));
    }

    #[test]
    fn auto_expand_timer_arms_once_and_resets_on_cancel() {
        let timer = AutoExpandTimer::default();
        assert!(!timer.armed.get());
        timer.cancel();
        assert!(!timer.armed.get());
        timer.armed.set(true);
        let before = timer.token.get();
        timer.cancel();
        assert!(!timer.armed.get());
        assert_ne!(timer.token.get(), before);
    }

    #[test]
    fn dragging_a_tree_row_onto_another_row_delivers_drag_drop_inside_scrollable() {
        use aimer::dnd::DragTarget;
        use aimer::input::mouse_region::MouseRegion;
        use aimer::{AimerApp, Column, Container, HeadlessOptions, Scrollable, SizedBox};

        DragSession::cancel_any();
        DragOverlay::hide();

        let dropped: Rc<RefCell<Option<DraggedEntries>>> = Rc::default();
        let dropped_in = Rc::clone(&dropped);
        let hovered = Rc::new(Cell::new(false));
        let hovered_in = Rc::clone(&hovered);
        let dragging_active = Rc::new(Cell::new(false));
        let dragging_in = Rc::clone(&dragging_active);

        let root_widget = Scrollable::new()
            .vertical_scroll_bar(None)
            .child(
                Container::new()
                    .padding(crate::ui::components::pad::pad(2, 0, 12, 0))
                    .child(
                        Column::new()
                            .list(vec![0usize, 1usize])
                            .builder(move |&idx| {
                                let dropped_in = Rc::clone(&dropped_in);
                                let hovered_in = Rc::clone(&hovered_in);
                                let dragging_in = Rc::clone(&dragging_in);
                                let path = if idx == 0 {
                                    PathBuf::from("/repo/dir")
                                } else {
                                    PathBuf::from("/repo/a.rs")
                                };
                                let target = DragTarget::<DraggedEntries>::new()
                                    .will_accept(move |entries: &DraggedEntries| {
                                        idx == 0 && entries.paths == [PathBuf::from("/repo/a.rs")]
                                    })
                                    .on_accept(move |entries| {
                                        *dropped_in.borrow_mut() = Some(entries);
                                    })
                                    .child(move |state| {
                                        if idx == 0 && state.is_hovered && state.will_accept {
                                            hovered_in.set(true);
                                        }
                                        MouseRegion::new()
                                            .child(SizedBox::new().width(200).height(24))
                                    });
                                TreeRowInteraction::new(
                                    move || DraggedEntries {
                                        paths: vec![path.clone()],
                                        name: "a.rs".into(),
                                        is_dir: idx == 0,
                                    },
                                    |_| SizedBox::new().width(80).height(20).boxed(),
                                    move |active| dragging_in.set(active.is_some()),
                                    || {},
                                    |_| {},
                                    target,
                                )
                            }),
                    ),
            );

        let mut app = AimerApp::start_headless_with(
            root_widget,
            HeadlessOptions {
                size: aimer::quiver::winit::dpi::PhysicalSize::new(600, 400),
                scale_factor: 2.0,
            },
        );
        app.pump_frames(2);

        use aimer::quiver::winit::dpi::PhysicalPosition;
        use aimer::quiver::winit::event::{DeviceId, ElementState, MouseButton, WindowEvent};
        let device_id = DeviceId::dummy();
        // Scale 2.0: logical (50, 38) -> physical (100, 76); logical (50, 14) -> physical (100, 28)
        app.send_window_event(WindowEvent::CursorMoved {
            device_id,
            position: PhysicalPosition::new(100.0, 76.0),
        });
        app.send_window_event(WindowEvent::MouseInput {
            device_id,
            state: ElementState::Pressed,
            button: MouseButton::Left,
        });
        // Step 1: move 5px within the source row itself (starts drag & triggers DragOver on source row's own DragTarget).
        app.send_window_event(WindowEvent::CursorMoved {
            device_id,
            position: PhysicalPosition::new(100.0, 66.0),
        });
        app.pump_frames(2);
        assert!(
            dragging_active.get(),
            "dragged row opacity state should be active"
        );
        // Step 2: move to the target row above.
        app.send_window_event(WindowEvent::CursorMoved {
            device_id,
            position: PhysicalPosition::new(100.0, 28.0),
        });
        app.pump_frames(2);
        assert!(
            DragSession::is_active(),
            "drag session should be active after move"
        );
        assert!(
            hovered.get(),
            "target row should be hovered during DragOver"
        );
        app.send_window_event(WindowEvent::MouseInput {
            device_id,
            state: ElementState::Released,
            button: MouseButton::Left,
        });
        app.pump_frames(2);
        assert_eq!(
            dropped
                .borrow()
                .as_ref()
                .map(|e| e.paths.clone()),
            Some(vec![PathBuf::from("/repo/a.rs")]),
            "target row should receive DragDrop"
        );
        assert!(
            !dragging_active.get(),
            "dragged row opacity state clears on drop"
        );

        DragSession::cancel_any();
        DragOverlay::hide();
    }
}
