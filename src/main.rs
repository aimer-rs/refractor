mod app_shell;
mod components;
mod doc_comments;
mod document;
mod file_icons;
mod fonts;
mod git;
mod markdown;
mod screen;
mod syntax;
mod theme;
mod tree;
mod workspace;

use crate::app_shell::AppShell;
use crate::workspace::Workspace;
use aimer::native::macos_windowing::MacosWindowing;
use aimer::quiver::window_attr::WindowAttr;
use aimer::*;

// this is the entry point of the app
#[aimer::main]
fn main() {
    // The code font is registered before the app starts, as the framework asks.
    fonts::register();
    AimerApp::new()
        .window(
            WindowAttr::new()
                .title("Refractor")
                .inner_size(1100, 720)
                .min_inner_size(640, 400)
                .remember("refractor-main"),
        )
        .setup(|| {
            // The title bar is drawn over the page, so the editor's own title strip runs to the top.
            MacosWindowing::new()
                .titlebar_transparent(true)
                .title_hidden(true)
                .fullsize_content_view(true)
                .traffic_light_position(16.0, 14.0)
                .has_shadow(true)
                .accepts_first_mouse(true)
                .install();
        })
        .child(
            Provider::new()
                .create(Workspace::initial)
                .child(AppShell),
        )
        .run()
}
