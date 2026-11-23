//! The parameter-hint bubble drawn above the caret while inside a function or macro call's arguments.
//!
//! Shows the active parameter in bold bright text and any other parameters grayed out, with a small pointer tip aimed
//! directly at the caret.

use crate::ui::components::popup::{Anchor, Popup};
use crate::ui::fonts;
use crate::services::lsp::SignatureHelp;
use crate::ui::semantic::Scheme;
use crate::services::settings::ThemeMode;
use crate::model::syntax::SyntaxPalette;
use crate::ui::theme::CodeTheme;
use aimer::style::{FontWeight, LayoutSpacing, TextAlign, TextOverflow, TextStyle};
use aimer::{
    AnyWidget, Color, Container, Dimension, Positioned, RichText, SizedBox, SpanStyle, Stack,
    StaticSvg, Svg, SvgStyle, TextSpan, Widget,
};

/// UI font size for parameter labels.
const FONT_SIZE: u32 = 13;
/// Estimated average character advance in the UI font at [`FONT_SIZE`].
const CHAR_WIDTH: f32 = 7.5;
/// Line height for wrapped parameter lists.
const LINE_HEIGHT: f32 = 18.0;
/// Inner padding of the bubble.
const PAD_X: u32 = 12;
const PAD_Y: u32 = 7;
/// Corner radius of `Popup::frame`.
const RADIUS: f32 = 8.0;
/// Dimensions of the triangular pointer tip attached to the bubble edge.
const ARROW_W: f32 = 12.0;
const ARROW_H: f32 = 6.0;
/// Gap between the tip of the pointer arrow and the caret box.
const GAP: f32 = 2.0;
/// Bounds on the bubble width.
const MIN_WIDTH: f32 = 44.0;
const MAX_WIDTH: f32 = 520.0;
/// Opacity of inactive parameters and comma separators so they read clearly as grayed out.
const INACTIVE_ALPHA: f32 = 0.40;

static ARROW_DOWN: StaticSvg = StaticSvg::new(
    "param_hint_arrow_down",
    r##"<svg viewBox="0 0 12 7" xmlns="http://www.w3.org/2000/svg"><path id="fill" d="M0,0 L12,0 L6,6 Z" fill="#000"/><path id="stroke" d="M0.5,0.5 L6,6 L11.5,0.5" fill="none" stroke="#000" stroke-width="1"/></svg>"##,
);

static ARROW_UP: StaticSvg = StaticSvg::new(
    "param_hint_arrow_up",
    r##"<svg viewBox="0 0 12 7" xmlns="http://www.w3.org/2000/svg"><path id="fill" d="M0,7 L12,7 L6,1 Z" fill="#000"/><path id="stroke" d="M0.5,6.5 L6,1 L11.5,6.5" fill="none" stroke="#000" stroke-width="1"/></svg>"##,
);

/// Where the bubble and its pointer tip go, in field coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub left: f32,
    pub top: f32,
    /// `true` when placed above the caret (arrow pointing down); `false` when flipped below.
    pub above: bool,
    /// Horizontal offset of the arrow relative to the bubble's left edge.
    pub arrow_x: f32,
}

/// Estimates the `(width, height)` of the bubble box (excluding the pointer arrow) for `help`.
pub fn measure_bubble(help: &SignatureHelp) -> (f32, f32) {
    let total_chars: usize = help
        .parameters
        .iter()
        .map(|param| param.chars().count())
        .sum::<usize>()
        + help.parameters.len().saturating_sub(1) * 2;
    let chrome_x = 2.0 * PAD_X as f32 + 2.0;
    let chrome_y = 2.0 * PAD_Y as f32 + 2.0;
    let max_content_w = (MAX_WIDTH - chrome_x).max(CHAR_WIDTH);
    let raw_content_w = total_chars as f32 * CHAR_WIDTH;
    let (width, lines) = if raw_content_w <= max_content_w {
        (
            (raw_content_w + chrome_x).clamp(MIN_WIDTH, MAX_WIDTH),
            1usize,
        )
    } else {
        let chars_per_line = (max_content_w / CHAR_WIDTH).floor().max(1.0) as usize;
        let lines = total_chars.div_ceil(chars_per_line).max(1);
        (MAX_WIDTH, lines)
    };
    let height = lines as f32 * LINE_HEIGHT + chrome_y;
    (width.ceil(), height.ceil())
}

/// Computes the bubble's placement in field coordinates: centered above `anchor`, flipping below only when
/// there is not enough room above and more room below, clamped horizontally to the visible field extent.
pub fn place_bubble(
    anchor: Anchor,
    (width, height): (f32, f32),
    extent: Option<(f32, f32)>,
) -> Placement {
    let above_top = anchor.y - GAP - ARROW_H - height;
    let below_top = anchor.y + anchor.height + GAP + ARROW_H;
    let above = match extent {
        Some((_, bottom)) => {
            if above_top >= 0.0 {
                true
            } else {
                let room_below = bottom - below_top;
                let room_above = anchor.y - GAP - ARROW_H;
                room_above >= room_below
            }
        }
        None => above_top >= 0.0,
    };
    let top = if above { above_top } else { below_top };

    let raw_left = anchor.x - width * 0.5;
    let left = match extent {
        Some((right, _)) => raw_left.min(right - width).max(0.0),
        None => raw_left.max(0.0),
    };
    let min_arrow = RADIUS;
    let max_arrow = (width - RADIUS - ARROW_W).max(min_arrow);
    let arrow_x = (anchor.x - left - ARROW_W * 0.5).clamp(min_arrow, max_arrow);
    Placement {
        left,
        top,
        above,
        arrow_x,
    }
}

/// Builds the positioned parameter-hint bubble relative to the caret at `(caret_x, caret_y)`.
pub fn param_hint_layer(
    help: &SignatureHelp,
    anchor: Anchor,
    extent: Option<(f32, f32)>,
    theme: CodeTheme,
    mode: ThemeMode,
) -> AnyWidget {
    let (width, height) = measure_bubble(help);
    let placement = place_bubble(anchor, (width, height), extent);
    let rel_left = placement.left - anchor.x;
    let rel_top = placement.top - anchor.y;

    let muted_style = SpanStyle::new()
        .font_weight(FontWeight::Normal)
        .color(theme.code.with_alpha(INACTIVE_ALPHA));

    let mut spans = Vec::with_capacity(help.parameters.len() * 4);
    for (index, param) in help.parameters.iter().enumerate() {
        if index > 0 {
            spans.push(TextSpan::new(",   ").style(muted_style));
        }
        if help.active_parameter == Some(index) {
            push_colored_param(&mut spans, param, theme, mode);
        } else {
            spans.push(TextSpan::new(param.as_str()).style(muted_style));
        }
    }

    let bubble = Container::new()
        .width(width)
        .height(height)
        .padding(
            LayoutSpacing::new()
                .left(PAD_X)
                .right(PAD_X)
                .top(PAD_Y)
                .bottom(PAD_Y),
        )
        .decoration(Popup::frame(theme))
        .child(
            RichText::new(TextSpan::root(spans))
                .text_align(TextAlign::MidCenter)
                .text_style(
                    TextStyle::new()
                        .font_family(fonts::ui_family())
                        .font_size(FONT_SIZE)
                        .color(theme.code)
                        .text_overflow(TextOverflow::Wrap),
                ),
        );

    let (arrow_svg, bubble_top, arrow_top, container_top) = if placement.above {
        (&ARROW_DOWN, 0.0, height - 1.0, rel_top)
    } else {
        (&ARROW_UP, ARROW_H, 0.0, rel_top - ARROW_H)
    };
    let arrow = Svg::new(arrow_svg)
        .width(ARROW_W)
        .height(ARROW_H + 1.0)
        .style("#fill", SvgStyle::new().fill(theme.bar))
        .style(
            "#stroke",
            SvgStyle::new()
                .stroke(theme.divider)
                .stroke_width(1.0),
        );

    Positioned::new()
        .left(Dimension::Px(rel_left))
        .top(Dimension::Px(container_top))
        .child(
            SizedBox::new()
                .width(width)
                .height(height + ARROW_H)
                .child(
                    Stack::new()
                        .add_child(
                            Positioned::new()
                                .left(Dimension::Px(0.0))
                                .top(Dimension::Px(bubble_top))
                                .child(bubble),
                        )
                        .add_child(
                            Positioned::new()
                                .left(Dimension::Px(placement.arrow_x))
                                .top(Dimension::Px(arrow_top))
                                .child(arrow),
                        ),
                ),
        )
        .boxed()
}

/// Appends syntax-colored spans for an active parameter such as `input_size: usize` or `f: unsafe fn(&mut T) -> Option<E>`,
/// using the semantic color scheme so primitives, enums, structs, type parameters, keywords, and `unsafe` are distinct.
fn push_colored_param(spans: &mut Vec<TextSpan>, param: &str, theme: CodeTheme, mode: ThemeMode) {
    let scheme = Scheme::for_mode(mode);
    let palette = SyntaxPalette::for_mode(mode);
    let color_of = |kind: &str, fallback: Color| {
        scheme
            .rule_of(kind, &[])
            .map_or(fallback, |rule| rule.look.color)
    };
    let bold = |color: Color| {
        SpanStyle::new()
            .font_weight(FontWeight::Bold)
            .color(color)
    };
    let punct_style = SpanStyle::new()
        .font_weight(FontWeight::Normal)
        .color(theme.code.with_alpha(0.75));

    // Split `name: Type` at the first top-level `: ` so the parameter name is colored separately from its type.
    let type_part = if let Some(colon) = find_param_colon(param) {
        let name_part = &param[..colon];
        spans.push(TextSpan::new(name_part).style(bold(theme.code)));
        spans.push(TextSpan::new(":").style(punct_style));
        &param[colon + 1..]
    } else {
        param
    };

    let bytes = type_part.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let ch = type_part[i..]
            .chars()
            .next()
            .expect("valid utf-8");
        if ch == '\'' {
            let start = i;
            i += ch.len_utf8();
            while i < bytes.len() {
                let next = type_part[i..]
                    .chars()
                    .next()
                    .expect("valid utf-8");
                if next.is_alphanumeric() || next == '_' {
                    i += next.len_utf8();
                } else {
                    break;
                }
            }
            spans.push(TextSpan::new(&type_part[start..i]).style(bold(palette.attribute)));
        } else if ch.is_alphabetic() || ch == '_' {
            let start = i;
            i += ch.len_utf8();
            while i < bytes.len() {
                let next = type_part[i..]
                    .chars()
                    .next()
                    .expect("valid utf-8");
                if next.is_alphanumeric() || next == '_' {
                    i += next.len_utf8();
                } else {
                    break;
                }
            }
            let word = &type_part[start..i];
            let style = match word {
                "unsafe" => bold(scheme.unsafe_foreground())
                    .background_color(scheme.unsafe_background())
                    .background_radius(Scheme::UNSAFE_RADIUS),
                "mut" | "ref" | "const" | "static" | "impl" | "dyn" | "fn" | "self" | "for"
                | "where" | "extern" => bold(color_of("keyword", palette.keyword)),
                "Self" => bold(color_of("selfTypeKeyword", palette.types)),
                "bool" | "char" | "str" | "i8" | "i16" | "i32" | "i64" | "i128" | "isize"
                | "u8" | "u16" | "u32" | "u64" | "u128" | "usize" | "f32" | "f64" => {
                    bold(color_of("builtinType", palette.types))
                }
                "Option" | "Result" | "Ordering" | "Cow" | "Bound" | "Poll" | "ControlFlow"
                | "Entry" | "IpAddr" | "SocketAddr" | "SeekFrom" => {
                    bold(color_of("enum", palette.types))
                }
                _ if word.len() == 1 && word.chars().all(|c| c.is_ascii_uppercase()) => {
                    bold(color_of("typeParameter", palette.types))
                }
                _ if word.starts_with(|c: char| c.is_ascii_uppercase()) => {
                    bold(color_of("struct", palette.types))
                }
                _ => bold(theme.code),
            };
            spans.push(TextSpan::new(word).style(style));
        } else {
            let start = i;
            i += ch.len_utf8();
            while i < bytes.len() {
                let next = type_part[i..]
                    .chars()
                    .next()
                    .expect("valid utf-8");
                if next == '\'' || next.is_alphabetic() || next == '_' {
                    break;
                }
                i += next.len_utf8();
            }
            spans.push(TextSpan::new(&type_part[start..i]).style(punct_style));
        }
    }
}

/// Finds the `:` separating a parameter's name from its type (ignoring `::` path separators).
fn find_param_colon(param: &str) -> Option<usize> {
    let bytes = param.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b':' {
            if i + 1 < bytes.len() && bytes[i + 1] == b':' {
                i += 2;
                continue;
            }
            return Some(i);
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(params: &[&str], active: Option<usize>) -> SignatureHelp {
        SignatureHelp {
            parameters: params.iter().map(|s| (*s).to_owned()).collect(),
            active_parameter: active,
        }
    }

    fn anchor(x: f32, y: f32) -> Anchor {
        Anchor { x, y, height: 20.0 }
    }

    #[test]
    fn bubble_prefers_centered_above_caret_with_arrow_pointing_at_caret() {
        let size = measure_bubble(&sig(&["value: T"], Some(0)));
        let placed = place_bubble(anchor(200.0, 100.0), size, Some((800.0, 600.0)));
        assert!(placed.above);
        assert_eq!(placed.top, 100.0 - GAP - ARROW_H - size.1);
        assert_eq!(placed.left, 200.0 - size.0 * 0.5);
        // Arrow center aligns with anchor.x.
        let arrow_center = placed.left + placed.arrow_x + ARROW_W * 0.5;
        assert!((arrow_center - 200.0).abs() < 0.01);
    }

    #[test]
    fn bubble_flips_below_when_near_top_edge_of_viewport() {
        let size = measure_bubble(&sig(&["a: i32", "b: &str"], Some(1)));
        let placed = place_bubble(anchor(200.0, 8.0), size, Some((800.0, 600.0)));
        assert!(!placed.above);
        assert_eq!(placed.top, 8.0 + 20.0 + GAP + ARROW_H);
    }

    #[test]
    fn bubble_clamps_horizontally_inside_viewport_and_keeps_arrow_within_corners() {
        let size = measure_bubble(&sig(
            &["first: i32", "second: &str", "third: bool"],
            Some(0),
        ));
        let near_left = place_bubble(anchor(10.0, 100.0), size, Some((800.0, 600.0)));
        assert_eq!(near_left.left, 0.0);
        assert!(near_left.arrow_x >= RADIUS);

        let near_right = place_bubble(anchor(790.0, 100.0), size, Some((800.0, 600.0)));
        assert_eq!(near_right.left, 800.0 - size.0);
        assert!(near_right.arrow_x <= size.0 - RADIUS - ARROW_W);
    }

    #[test]
    fn long_parameter_list_caps_at_max_width_and_grows_in_height() {
        let one = measure_bubble(&sig(&["x: i32"], Some(0)));
        let many = measure_bubble(&sig(
            &[
                "alpha: String",
                "beta: Vec<usize>",
                "gamma: HashMap<String, i64>",
                "delta: Option<PathBuf>",
                "epsilon: Duration",
            ],
            Some(2),
        ));
        assert_eq!(many.0, MAX_WIDTH);
        assert!(many.1 > one.1);
    }
}
