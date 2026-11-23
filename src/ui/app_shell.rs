mod file_opener;
pub mod menu_commands;

use self::file_opener::FileOpener;
use self::menu_commands::MenuCommands;
use crate::ui::screen::editor::EditorScreen;
use crate::services::settings::{Settings, ThemeMode};
use crate::ui::theme::CodeTheme;
use aimer::animation::curves;
use aimer::style::AnimatedTheme;
use aimer::*;
use std::time::Duration;

/// Provides the theme to the tree, and opens the files the app is asked to open.
#[derive(Clone, StatelessWidget)]
pub struct AppShell;

impl StatelessWidget for AppShell {
    fn build(&self, ctx: &BuildContext) -> impl Widget {
        // The theme is the user's choice, and the change of it fades over the duration below.
        let mode = ctx.select::<Settings, ThemeMode>(|settings| settings.theme());
        AnimatedTheme::new()
            .data(CodeTheme::for_mode(mode))
            .duration(Duration::from_millis(300))
            .curve(curves::EASE_IN_OUT)
            .child(MenuCommands::new(FileOpener::new(EditorScreen)))
    }
}
