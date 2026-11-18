mod file_opener;

use self::file_opener::FileOpener;
use crate::screen::editor::EditorScreen;
use crate::theme::CodeTheme;
use aimer::animation::curves;
use aimer::style::AnimatedTheme;
use aimer::*;
use std::time::Duration;

/// Provides the theme to the tree, and opens the files the app is asked to open.
#[derive(Clone, StatelessWidget)]
pub struct AppShell;

impl StatelessWidget for AppShell {
    fn build(&self, _ctx: &BuildContext) -> impl Widget {
        AnimatedTheme::new()
            .data(CodeTheme::dark())
            .duration(Duration::from_millis(300))
            .curve(curves::EASE_IN_OUT)
            .child(FileOpener::new(EditorScreen))
    }
}
