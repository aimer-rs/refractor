use aimer::style::LayoutSpacing;

/// Padding as CSS writes it: top, right, bottom, left.
pub fn pad(top: u32, right: u32, bottom: u32, left: u32) -> LayoutSpacing {
    LayoutSpacing::new()
        .top(top)
        .right(right)
        .bottom(bottom)
        .left(left)
}
