use crate::services::settings::ThemeMode;
use aimer::{Color, Theme};

/// The colors of the editor. Derives `Theme` so `AnimatedTheme` can provide it to the tree.
///
/// There is a dark theme and a light one, chosen in the settings: the syntax colors in
/// [`crate::model::syntax::SyntaxPalette`] are chosen for the backgrounds of the one they are named after.
#[derive(Clone, Copy, Debug, PartialEq, Theme)]
pub struct CodeTheme {
    /// The strips along the top and the bottom: the tabs and the status bar.
    pub bar: Color,
    /// The file tree.
    pub sidebar: Color,
    /// The code, and what is behind it.
    pub editor: Color,
    /// Lines between things.
    pub divider: Color,
    /// Text that is not code: file names and the status.
    pub text: Color,
    pub muted: Color,
    /// Changed files and what is selected: the color of a name that git says is modified.
    pub accent: Color,
    /// The line that marks the tab being edited.
    pub marker: Color,
    /// The background of the open file in the tree.
    pub selected: Color,
    /// The background of what the pointer is over.
    pub hover: Color,
    /// The color of code that has no syntax color.
    pub code: Color,
    pub line_number: Color,
    pub line_number_active: Color,
    /// The band behind the line the caret is on.
    pub current_line: Color,
    pub selection: Color,
    /// A file git sees as deleted.
    pub deleted: Color,
    /// Something went wrong.
    pub warning: Color,
    /// Nothing is wrong: the language server has nothing against the file.
    pub success: Color,
    /// Something may be: the warnings of the language server.
    pub caution: Color,
    /// What can be pressed in the text: the counts of usages after a definition. The pink of the app.
    pub link: Color,
    /// The same under the pointer: brighter, and drawn underlined.
    pub link_hover: Color,
    /// The text of an inlay hint, such as the type of a name that the language server infers.
    pub hint_text: Color,
    /// The pill an inlay hint is drawn in.
    pub hint_background: Color,
    /// The line under what the language server says is an error: red.
    pub error_line: Color,
    /// The line under what it says is a warning: yellow.
    pub warning_line: Color,
    /// The line under what it says is worth saying and not worth worrying about, a weak warning: quieter than the others.
    pub weak_line: Color,
}

impl CodeTheme {
    /// The colors for `mode`.
    pub fn for_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self::dark(),
            ThemeMode::Light => Self::light(),
        }
    }

    /// The color of the indent guide at `level`, cycling through [`crate::model::syntax::SyntaxPalette`]'s rainbow levels
    /// and interpolating between the dark and light palettes as the theme transitions.
    pub fn indent_guide(&self, level: usize) -> Color {
        use crate::model::syntax::SyntaxPalette;

        let dark_r = f32::from(Self::dark().sidebar.to_rgba().0);
        let light_r = f32::from(Self::light().sidebar.to_rgba().0);
        let current_r = f32::from(self.sidebar.to_rgba().0);
        let t = ((current_r - dark_r) / (light_r - dark_r)).clamp(0.0, 1.0);
        SyntaxPalette::dark()
            .rainbow(level)
            .lerp(SyntaxPalette::light().rainbow(level), t)
    }

    pub fn dark() -> Self {
        Self {
            bar: Color::Rgb(11, 11, 16),
            sidebar: Color::Rgb(15, 0, 8),
            editor: Color::Rgb(15, 10, 13),
            divider: Color::Rgb(34, 33, 45),
            text: Color::Rgb(232, 230, 240),
            muted: Color::Rgb(160, 0, 99),
            accent: Color::Rgb(84, 205, 236),
            marker: Color::Rgb(255, 92, 150),
            selected: Color::Rgb(107, 1, 54),
            hover: Color::Rgb(255, 255, 255).with_alpha(0.06),
            code: Color::Rgb(236, 235, 242),
            line_number: Color::Rgb(98, 96, 118),
            line_number_active: Color::Rgb(228, 226, 238),
            current_line: Color::Rgb(250, 24, 156).with_alpha(0.025),
            selection: Color::Rgb(255, 92, 150).with_alpha(0.32),
            deleted: Color::Rgb(238, 108, 108),
            warning: Color::Rgb(240, 124, 112),
            success: Color::Rgb(112, 200, 152),
            caution: Color::Rgb(236, 190, 96),
            link: Color::Rgba(255, 52, 170, 169),
            link_hover: Color::Rgb(255, 156, 196),
            hint_text: Color::Rgb(238, 190, 212),
            hint_background: Color::Rgb(255, 92, 150).with_alpha(0.30),
            error_line: Color::Rgb(242, 78, 78),
            warning_line: Color::Rgb(244, 200, 60),
            weak_line: Color::Rgb(128, 146, 184),
        }
    }

    /// The same roles on a white page: the same pinks and blues, darkened until the text on them reads.
    pub fn light() -> Self {
        Self {
            bar: Color::Rgb(240, 239, 245),
            sidebar: Color::Rgb(246, 245, 250),
            editor: Color::Rgb(255, 255, 255),
            divider: Color::Rgb(220, 218, 230),
            text: Color::Rgb(36, 34, 48),
            muted: Color::Rgb(150, 20, 100),
            accent: Color::Rgb(0, 120, 160),
            marker: Color::Rgb(214, 40, 110),
            selected: Color::Rgb(236, 216, 244),
            hover: Color::Rgb(0, 0, 0).with_alpha(0.06),
            code: Color::Rgb(36, 34, 48),
            line_number: Color::Rgb(160, 158, 176),
            line_number_active: Color::Rgb(40, 38, 52),
            current_line: Color::Rgb(250, 24, 156).with_alpha(0.05),
            selection: Color::Rgb(214, 40, 110).with_alpha(0.24),
            deleted: Color::Rgb(200, 50, 50),
            warning: Color::Rgb(200, 60, 50),
            success: Color::Rgb(30, 130, 80),
            caution: Color::Rgb(170, 110, 0),
            link: Color::Rgb(214, 40, 110),
            link_hover: Color::Rgb(250, 30, 130),
            hint_text: Color::Rgb(150, 30, 90),
            hint_background: Color::Rgb(214, 40, 110).with_alpha(0.12),
            error_line: Color::Rgb(214, 36, 36),
            warning_line: Color::Rgb(206, 150, 0),
            weak_line: Color::Rgb(104, 120, 160),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_mode_has_its_own_colors() {
        assert_eq!(CodeTheme::for_mode(ThemeMode::Dark), CodeTheme::dark());
        assert_eq!(CodeTheme::for_mode(ThemeMode::Light), CodeTheme::light());
        assert_ne!(CodeTheme::dark(), CodeTheme::light());
    }

    /// The brightness of a color, 0 to 255, as the eye weighs the three.
    fn luma(color: Color) -> f32 {
        let (red, green, blue, _) = color.to_rgba();
        0.299 * f32::from(red) + 0.587 * f32::from(green) + 0.114 * f32::from(blue)
    }

    #[test]
    fn a_link_is_pink_and_is_brighter_under_the_pointer_in_both_modes() {
        for (name, theme) in [("dark", CodeTheme::dark()), ("light", CodeTheme::light())] {
            for color in [theme.link, theme.link_hover] {
                let (red, green, blue, _) = color.to_rgba();
                assert!(
                    red > green && red > blue && blue > green,
                    "{name}: pink is red with some blue and little green"
                );
            }
            assert_ne!(theme.link, theme.link_hover, "{name}");
        }
        let dark = CodeTheme::dark();
        assert!(
            luma(dark.link_hover) > luma(dark.link) + 20.0,
            "on the dark page, brighter is lighter"
        );
        let light = CodeTheme::light();
        let saturation = |color: Color| {
            let (red, green, blue, _) = color.to_rgba();
            f32::from(red.max(green).max(blue) - red.min(green).min(blue))
        };
        assert!(
            saturation(light.link_hover) > saturation(light.link),
            "on the white page, brighter is more vivid"
        );
    }

    #[test]
    fn text_is_darker_than_its_page_in_the_light_theme_and_lighter_in_the_dark_one() {
        let (dark, light) = (CodeTheme::dark(), CodeTheme::light());
        for (name, page, text) in [
            ("code", dark.editor, dark.code),
            ("text", dark.bar, dark.text),
        ] {
            assert!(luma(text) > luma(page) + 100.0, "dark {name}");
        }
        for (name, page, text) in [
            ("code", light.editor, light.code),
            ("text", light.bar, light.text),
        ] {
            assert!(luma(page) > luma(text) + 100.0, "light {name}");
        }
    }

    #[test]
    fn the_lines_under_problems_are_red_for_an_error_yellow_for_a_warning_and_quiet_for_a_weak_one_in_both_modes()
     {
        for (name, theme) in [("dark", CodeTheme::dark()), ("light", CodeTheme::light())] {
            let (red, green, blue, _) = theme.error_line.to_rgba();
            assert!(
                red > green + 80 && red > blue + 80,
                "{name}: red is red: {:?}",
                theme.error_line
            );
            let (red, green, blue, _) = theme.warning_line.to_rgba();
            assert!(
                red > blue + 100 && green > blue + 80,
                "{name}: yellow is red and green and no blue: {:?}",
                theme.warning_line
            );
            let (red, green, blue, _) = theme.weak_line.to_rgba();
            let spread = i32::from(red.max(green).max(blue)) - i32::from(red.min(green).min(blue));
            assert!(
                spread < 90,
                "{name}: a weak warning is quiet, and not a color to be taken for the others: {:?}",
                theme.weak_line
            );
            assert_ne!(theme.weak_line, theme.error_line);
            assert_ne!(theme.weak_line, theme.warning_line);
        }
        // A line has to be seen against the page: lighter than a dark page, darker than a white one.
        let dark = CodeTheme::dark();
        for line in [dark.error_line, dark.warning_line, dark.weak_line] {
            assert!(luma(line) > luma(dark.editor) + 70.0, "{line:?}");
        }
        let light = CodeTheme::light();
        for line in [light.error_line, light.warning_line, light.weak_line] {
            assert!(luma(light.editor) > luma(line) + 60.0, "{line:?}");
        }
    }

    #[test]
    fn the_text_of_a_hint_reads_on_the_page_it_is_drawn_over_in_both_modes() {
        let (dark, light) = (CodeTheme::dark(), CodeTheme::light());
        assert!(luma(dark.hint_text) > luma(dark.editor) + 100.0, "dark");
        assert!(luma(light.editor) > luma(light.hint_text) + 100.0, "light");
        for theme in [dark, light] {
            assert!(
                theme.hint_background.to_rgba().3 < 255,
                "a pill lets the page through, so it is a tint and not a block"
            );
        }
    }

    #[test]
    fn indent_guides_cycle_through_the_rainbow_palette_for_each_theme_mode() {
        use crate::model::syntax::SyntaxPalette;

        for (mode, theme) in [
            (ThemeMode::Dark, CodeTheme::dark()),
            (ThemeMode::Light, CodeTheme::light()),
        ] {
            let palette = SyntaxPalette::for_mode(mode);
            for level in 0..SyntaxPalette::RAINBOW_LEVELS * 2 {
                assert_eq!(theme.indent_guide(level), palette.rainbow(level));
            }
        }
    }
}
