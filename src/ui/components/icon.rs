use aimer::{Color, StaticSvg, Svg, SvgStyle};

/// A stroked Lucide icon in `color`.
pub fn icon(data: &'static StaticSvg, size: u32, color: Color) -> Svg {
    icon_stroked(data, size, color, 1.5)
}

/// A stroked Lucide icon in `color`, drawn with a line `width` thick: heavier than the usual for a mark
/// that has to be read at a glance.
pub fn icon_stroked(data: &'static StaticSvg, size: u32, color: Color, width: f32) -> Svg {
    Svg::new(data).width(size).height(size).style(
        "path",
        SvgStyle::new()
            .stroke(color)
            .stroke_width(width),
    )
}

/// A Material Icon Theme icon (see [`crate::ui::file_icons`]), in the colors it was drawn in.
pub fn material(data: &'static StaticSvg, size: u32) -> Svg {
    Svg::new(data).width(size).height(size)
}
