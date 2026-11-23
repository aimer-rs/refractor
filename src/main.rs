mod model;
mod services;
mod ui;

use crate::model::workspace::Workspace;
use crate::services::settings::Settings;
use crate::ui::app_shell::AppShell;
use aimer::native::macos_windowing::MacosWindowing;
use aimer::quiver::window_attr::WindowAttr;
use aimer::screen::{DisplayFeature, MediaQuery};
use aimer::*;

// this is the entry point of the app
#[aimer::main]
fn main() {
    let workspace = Workspace::initial();
    let workspace_path = workspace.root.clone();

    ui::fonts::register();
    ui::app_shell::menu_commands::install();
    AimerApp::new()
        .window(
            WindowAttr::new()
                .title("Refractor")
                .inner_size(1100, 720)
                .min_inner_size(640, 400)
                .remember(format!(
                    "refractor-main-{}",
                    workspace_path.to_string_lossy()
                )),
        )
        .setup(|| {
            // The title bar is drawn over the page, so the editor's own title strip runs to the top.
            MacosWindowing::new()
                .titlebar_transparent(true)
                // In full screen macOS shows the titlebar over the top of the page when the pointer reaches the top: the
                // lights float over the editor's own strip there, with no gray bar behind them.
                .fullscreen_titlebar_transparent(true)
                .title_hidden(true)
                .fullsize_content_view(true)
                .traffic_light_position(16.0, 14.0)
                .has_shadow(true)
                .accepts_first_mouse(true)
                .install();
        })
        .child(
            // The settings are outside the workspace: the shell reads the theme from them, and the tabs read the
            // rest of them, and the workspace has no use for them.
            Provider::new().create(Settings::load).child(
                Provider::new()
                    .create(move || workspace.clone())
                    .child(AppShell),
            ),
        )
        .run()
}
