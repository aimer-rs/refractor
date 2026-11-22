use super::*;

fn dark(kind: &str, modifiers: &[&str]) -> Option<Look> {
    Scheme::for_mode(ThemeMode::Dark)
        .rule_of(kind, modifiers)
        .map(|rule| rule.look)
}

fn rgb(red: u8, green: u8, blue: u8) -> Color {
    Color::Rgb(red, green, blue)
}

#[test]
fn the_kinds_of_the_scheme_have_its_colors() {
    assert_eq!(dark("method", &[]).unwrap().color, rgb(0x82, 0xAA, 0xFF));
    assert_eq!(dark("function", &[]).unwrap().color, rgb(0x82, 0xAA, 0xFF));
    assert_eq!(dark("keyword", &[]).unwrap().color, rgb(0xF8, 0x5F, 0xCD));
    assert_eq!(dark("struct", &[]).unwrap().color, rgb(0xFF, 0xCB, 0x6B));
    assert_eq!(dark("enum", &[]).unwrap().color, rgb(0x08, 0xB7, 0x20));
    assert_eq!(dark("variable", &[]).unwrap().color, rgb(0xE5, 0xFB, 0xCD));
    assert_eq!(dark("parameter", &[]).unwrap().color, rgb(0xA9, 0xDC, 0x76));
    assert_eq!(
        dark("constParameter", &[]).unwrap().color,
        rgb(0xFF, 0x9C, 0xAC)
    );
    assert_eq!(dark("static", &[]).unwrap().color, rgb(0x7D, 0xCF, 0xFF));
}

#[test]
fn a_trait_is_bold_and_a_constant_is_bold_and_the_mutable_are_underlined() {
    let interface = dark("interface", &[]).unwrap();
    assert_eq!(
        (interface.color, interface.bold, interface.underline),
        (rgb(0xFC, 0xA2, 0x48), true, false)
    );
    let constant = dark("variable", &["constant"]).unwrap();
    assert_eq!(
        (constant.color, constant.bold, constant.underline),
        (rgb(0xFF, 0x9C, 0xAC), true, false)
    );
    for (kind, mutable) in [
        ("variable", rgb(0xFF, 0x6B, 0x81)),
        ("parameter", rgb(0xFF, 0x6B, 0x81)),
        ("static", rgb(0x7D, 0xCF, 0xFF)),
    ] {
        let look = dark(kind, &["mutable"]).unwrap();
        assert_eq!(
            (look.color, look.underline, look.bold),
            (mutable, true, false),
            "{kind}"
        );
    }
    let plain = dark("variable", &[]).unwrap();
    assert!(!plain.underline && !plain.bold);
}

#[test]
fn the_rule_that_asks_for_the_most_modifiers_of_those_a_token_has_is_the_one() {
    // A variable that is mutable is not the plain variable of the scheme, and one that is static and mutable is a static one.
    assert_eq!(
        dark("variable", &["declaration", "mutable"])
            .unwrap()
            .color,
        rgb(0xFF, 0x6B, 0x81)
    );
    assert_eq!(
        dark("variable", &["static", "mutable"])
            .unwrap()
            .color,
        rgb(0x7D, 0xCF, 0xFF)
    );
    assert!(
        dark("variable", &["static", "mutable"])
            .unwrap()
            .underline
    );
    assert_eq!(
        dark("variable", &["static"]).unwrap().color,
        rgb(0x7D, 0xCF, 0xFF)
    );
    assert!(!dark("variable", &["static"]).unwrap().underline);
    assert_eq!(
        dark("variable", &["readonly", "declaration"])
            .unwrap()
            .color,
        rgb(0xA9, 0xDC, 0x76)
    );
    // A keyword of the flow of control is not the plain keyword.
    assert_eq!(
        dark("keyword", &["controlFlow"]).unwrap().color,
        rgb(0x9F, 0x1A, 0xD8)
    );
    assert_eq!(
        dark("keyword", &["controlFlow", "unsafe"])
            .unwrap()
            .color,
        rgb(0x9F, 0x1A, 0xD8),
        "a modifier no rule asks for is no matter"
    );
    assert_eq!(
        dark("keyword", &["unsafe"]).unwrap().color,
        rgb(0xF8, 0x5F, 0xCD)
    );
}

#[test]
fn what_no_rule_is_for_is_left_to_the_grammar() {
    for kind in [
        "string",
        "comment",
        "number",
        "operator",
        "namespace",
        "unknownKind",
        "",
    ] {
        assert!(dark(kind, &[]).is_none(), "{kind}");
        assert!(
            Scheme::for_mode(ThemeMode::Dark)
                .style_for(kind, &["mutable"])
                .is_none(),
            "{kind}"
        );
    }
    // A rule that asks for a modifier does not match a token that does not have it, and has no rule of its kind that asks for none.
    assert!(dark("const", &[]).is_some());
    assert!(
        dark("static", &["readonly"]).is_some(),
        "a static is a static whatever else it is"
    );
}

#[test]
fn the_style_of_a_token_is_its_color_and_its_weight_and_its_line() {
    let scheme = Scheme::for_mode(ThemeMode::Dark);
    let mutable = scheme
        .style_for("variable", &["mutable"])
        .unwrap();
    assert_eq!(mutable.color, Some(rgb(0xFF, 0x6B, 0x81)));
    assert_eq!(mutable.text_decoration, Some(TextDecoration::Underline));
    assert_eq!(mutable.font_weight, None);
    let bold = scheme.style_for("interface", &[]).unwrap();
    assert_eq!(bold.font_weight, Some(FontWeight::Bold));
    assert_eq!(bold.text_decoration, None);
}

#[test]
fn the_light_scheme_has_the_same_rules_in_colors_that_read_on_a_white_page() {
    let (dark_scheme, light_scheme) = (
        Scheme::for_mode(ThemeMode::Dark),
        Scheme::for_mode(ThemeMode::Light),
    );
    assert_eq!(dark_scheme.rules.len(), light_scheme.rules.len());
    for (dark_rule, light_rule) in dark_scheme.rules.iter().zip(light_scheme.rules) {
        assert_eq!(
            (dark_rule.kind, dark_rule.modifiers),
            (light_rule.kind, light_rule.modifiers)
        );
        assert_eq!(
            (dark_rule.look.bold, dark_rule.look.underline),
            (light_rule.look.bold, light_rule.look.underline)
        );
        let (red, green, blue, _) = light_rule.look.color.to_rgba();
        let luma = 0.299 * f32::from(red) + 0.587 * f32::from(green) + 0.114 * f32::from(blue);
        assert!(luma < 160.0, "{} on white: {luma}", light_rule.kind);
    }
}
