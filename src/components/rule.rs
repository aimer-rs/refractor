use aimer::{Color, Container, Dimension, SizedBox, Widget};

/// A line one logical pixel thick across the width it is given.
pub fn horizontal(color: Color) -> impl Widget {
    SizedBox::new()
        .width(Dimension::Percent(100.0))
        .height(1)
        .box_child(
            Container::new().color(color).child(
                SizedBox::new()
                    .width(Dimension::Percent(100.0))
                    .height(1),
            ),
        )
}

/// A line one logical pixel thick down the height it is given.
pub fn vertical(color: Color) -> impl Widget {
    SizedBox::new()
        .width(1)
        .height(Dimension::Percent(100.0))
        .box_child(
            Container::new().color(color).child(
                SizedBox::new()
                    .width(1)
                    .height(Dimension::Percent(100.0)),
            ),
        )
}
