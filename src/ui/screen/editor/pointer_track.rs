//! A transparent overlay widget that records the latest pointer window coordinates without
//! consuming any events. Used so gutter marker callbacks (which carry no coordinates of their own)
//! can anchor a popup menu at the clicked position.

use std::cell::Cell;
use std::rc::Rc;

use aimer::events::element::ElementEvent;
use aimer::{
    AnyElement, BuildContext, Dimension, Drawable, Element, EventElement, EventResult,
    EventTreeRole, InteractionBounds, InteractionDisagreement, InteractionSource, LayoutElement,
    Rebuildable, ResolvedSize, Size, SizedBox, Vec2d, VisitorElement, Widget,
};

/// Transparent pass-through widget that fills its parent's constraints and writes the pointer
/// window position of every pointer event into `pointer`.
pub struct PointerTrack {
    pointer: Rc<Cell<Vec2d>>,
}

impl PointerTrack {
    /// Creates a tracker that stores pointer coordinates in `pointer`.
    pub fn new(pointer: Rc<Cell<Vec2d>>) -> Self {
        Self { pointer }
    }
}

impl Widget for PointerTrack {
    fn to_element(self, ctx: &BuildContext) -> AnyElement {
        let child = SizedBox::new()
            .width(Dimension::Percent(100.0))
            .height(Dimension::Percent(100.0))
            .to_element(ctx);
        AnyElement::new(RawPointerTrack {
            pointer: self.pointer,
            child,
            bounds: InteractionBounds::new(),
        })
    }
}

struct RawPointerTrack {
    pointer: Rc<Cell<Vec2d>>,
    child: AnyElement,
    bounds: InteractionBounds,
}

impl VisitorElement for RawPointerTrack {
    fn visit_children<'a>(&'a self, visitor: &mut dyn FnMut(&'a dyn Element)) {
        visitor(self.child.as_ref());
    }

    fn visit_retained_children<'a>(&'a self, visitor: &mut dyn FnMut(usize, &'a dyn Element)) {
        visitor(0, self.child.as_ref());
    }
}

impl Element for RawPointerTrack {
    fn name(&self) -> &'static str {
        "PointerTrack"
    }
}

impl EventElement for RawPointerTrack {
    fn event_tree_role(&self) -> EventTreeRole {
        EventTreeRole::IndexedTarget
    }

    fn on_event(&self, event: &ElementEvent) -> EventResult {
        if let Some(pos) = event.get_pointer_pos() {
            self.pointer.set(pos);
        }
        EventResult::ignored()
    }
}

impl LayoutElement for RawPointerTrack {
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

impl Drawable for RawPointerTrack {
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
        self.bounds.disagreement("PointerTrack")
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
        false
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

impl Rebuildable for RawPointerTrack {}
