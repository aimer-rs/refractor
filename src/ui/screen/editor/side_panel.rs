use super::file_tree::FileTree;
use super::git_panel::GitPanel;
use crate::ui::components::icon::icon;
use crate::ui::components::pad::pad;
use crate::ui::components::rule::{self};
use crate::ui::theme::CodeTheme;
use crate::model::workspace::{SidePanelTab, Workspace, toggle_git_panel};
use aimer::decoration::Decoration;
use aimer::input::button::Button;
use aimer::style::{BorderRadius, LayoutSpacing, Theme};
use aimer::{
    AnyWidget, BoxAlignment, BuildContext, Color, Column, Container, Dimension, Expanded, Icons,
    Padding, ProviderContext, ProviderHandle, Row, SizedBox, StatelessWidget, Widget, ZeroSizedBox,
    list,
};

/// How tall the header is: the same as the row of tabs beside it, so the two line up.
pub const HEADER_HEIGHT: u32 = 38;
/// The window's buttons (close, minimise, zoom) are drawn over the left of the header; the name
/// starts after them.
const WINDOW_BUTTONS: u32 = 84;

/// The left side: switches between the file tree (`CmdOrCtrl+1`) and the Git panel (`CmdOrCtrl+2`).
#[derive(Clone, StatelessWidget)]
pub struct SidePanel;

impl StatelessWidget for SidePanel {
    fn build(&self, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let tab = ctx.select::<Workspace, SidePanelTab>(|space| space.side_panel_tab);
        let store = ProviderHandle::<Workspace>::of(ctx);
        let git_prompt = store.read().git_prompt();

        Container::new()
            .color(theme.sidebar)
            .child(Column::new().children(list![
                // SizedBox::new()
                //     .width(Dimension::Percent(100.0))
                //     .height(HEADER_HEIGHT)
                //     .child(
                //         Container::new()
                //             .padding(pad(10, 10, 2, WINDOW_BUTTONS))
                //             .child(
                //                 Row::new()
                //                     .vertical_alignment(BoxAlignment::Center)
                //                     .gaps(LayoutSpacing::all(2))
                //                     .children(tabs(tab, theme, store)),
                //             ),
                //     ),
                Expanded::new().child(
                    Padding::new()
                        .padding(LayoutSpacing::horizontal(6.into()))
                        .child(match tab {
                            SidePanelTab::Files => FileTree.boxed(),
                            SidePanelTab::Git => GitPanel::new(git_prompt).boxed(),
                        })
                )
            ]))
    }
}

/// A small button of the header.
fn tool(
    glyph: &'static aimer::StaticSvg,
    active: bool,
    theme: CodeTheme,
    on_press: impl Fn() + 'static,
) -> impl Widget {
    Button::new()
        .no_decor()
        .on_press(on_press)
        .build(move |state| {
            Container::new()
                .padding(pad(4, 5, 4, 5))
                .decoration([
                    BorderRadius::all(5).notype(),
                    if active {
                        theme.marker.with_alpha(0.22)
                    } else if state.is_hovered {
                        theme.hover
                    } else {
                        Color::Transparent
                    }
                    .notype(),
                ])
                .child(icon(
                    glyph,
                    15,
                    if active {
                        theme.marker
                    } else if state.is_hovered {
                        theme.text
                    } else {
                        theme.muted
                    },
                ))
        })
}

fn tabs(tab: SidePanelTab, theme: CodeTheme, store: ProviderHandle<Workspace>) -> Vec<AnyWidget> {
    // let files_store = store.clone();
    // let git_store = store.clone();
    // let collapse = store.clone();
    list![
        // Expanded::new().child(ZeroSizedBox).boxed(),
        // tool(
        //     Icons::FOLDER_OPEN,
        //     tab == SidePanelTab::Files,
        //     theme,
        //     move || {
        //         files_store.update(|space| space.set_side_panel_tab(SidePanelTab::Files));
        //     },
        // ),
        // tool(
        //     Icons::GIT_COMMIT_HORIZONTAL,
        //     tab == SidePanelTab::Git,
        //     theme,
        //     move || {
        //         toggle_git_panel(&git_store);
        //     }
        // ),
        // tool(
        //     Icons::CHEVRONS_DOWN_UP,
        //     tab == SidePanelTab::Files,
        //     theme,
        //     move || { collapse.update(|space| space.collapse_all()) }
        // ),
        // tool(Icons::REFRESH_CW, false, theme, move || {
        //     reload.update(|space| space.refresh());
        //     refresh_status(&reload);
        //     refresh_git_panel(&reload);
        // }),
    ]
}

/// The line down the right edge of the side panel, between it and the editor.
pub fn edge(theme: CodeTheme) -> impl Widget {
    rule::vertical(theme.divider)
}
