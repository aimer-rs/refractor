use aimer::decoration::Decoration;
use aimer::style::BorderRadius;
use aimer::{Color, Container, SizedBox, Widget};

/// A filled dot of `diameter` logical pixels, which marks something as changed. The outer box is
/// what keeps it that size: a container fills what it is given across a row.
pub fn dot(diameter: u32, color: Color) -> impl Widget {
    SizedBox::new()
        .width(diameter)
        .height(diameter)
        .box_child(
            Container::new()
                .decoration([BorderRadius::all(diameter / 2 + 1).notype(), color.notype()])
                .child(
                    SizedBox::new()
                        .width(diameter)
                        .height(diameter),
                ),
        )
}
