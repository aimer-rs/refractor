use aimer::{Color, Theme};

/// The colors of the editor. Derives `Theme` so `AnimatedTheme` can provide it to the tree.
///
/// There is one theme, a dark one: the syntax colors in [`crate::syntax::SyntaxPalette`] are chosen
/// for these backgrounds.
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
}

impl CodeTheme {
    pub fn dark() -> Self {
        Self {
            bar: Color::Rgb(11, 11, 16),
            sidebar: Color::Rgb(12, 12, 17),
            editor: Color::Rgb(10, 10, 15),
            divider: Color::Rgb(34, 33, 45),
            text: Color::Rgb(232, 230, 240),
            muted: Color::Rgb(138, 136, 154),
            accent: Color::Rgb(84, 205, 236),
            marker: Color::Rgb(255, 92, 150),
            selected: Color::Rgb(60, 34, 72),
            hover: Color::Rgb(255, 255, 255).with_alpha(0.06),
            code: Color::Rgb(236, 235, 242),
            line_number: Color::Rgb(98, 96, 118),
            line_number_active: Color::Rgb(228, 226, 238),
            current_line: Color::Rgb(250, 24, 156).with_alpha(0.025),
            selection: Color::Rgb(118, 72, 170).with_alpha(0.5),
            deleted: Color::Rgb(238, 108, 108),
            warning: Color::Rgb(240, 124, 112),
        }
    }
}
