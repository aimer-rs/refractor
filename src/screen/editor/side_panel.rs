use super::file_tree::FileTree;
use crate::components::icon::icon;
use crate::components::pad::pad;
use crate::components::rule;
use crate::fonts;
use crate::theme::CodeTheme;
use crate::workspace::{Workspace, refresh_status};
use aimer::decoration::Decoration;
use aimer::input::button::Button;
use aimer::style::{BorderRadius, LayoutSpacing, Theme};
use aimer::{
    BoxAlignment, BuildContext, Color, Column, Container, Dimension, Expanded, Icons,
    ProviderContext, ProviderHandle, Row, SizedBox, StatelessWidget, Text, Widget, ZeroSizedBox,
};

/// How tall the header is: the same as the row of tabs beside it, so the two line up.
pub const HEADER_HEIGHT: u32 = 38;
/// The window's buttons (close, minimise, zoom) are drawn over the left of the header; the name
/// starts after them.
const WINDOW_BUTTONS: u32 = 84;

/// The left side: the folder's name with the buttons that act on the tree, and the tree.
#[derive(Clone, StatelessWidget)]
pub struct SidePanel;

/// A small button of the header.
fn tool(
    glyph: &'static aimer::StaticSvg,
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
                    if state.is_hovered {
                        theme.hover
                    } else {
                        Color::Transparent
                    }
                    .notype(),
                ])
                .child(icon(
                    glyph,
                    15,
                    if state.is_hovered {
                        theme.text
                    } else {
                        theme.muted
                    },
                ))
        })
}

impl StatelessWidget for SidePanel {
    fn build(&self, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let name = ctx.select::<Workspace, String>(|space| space.name().to_uppercase());
        let store = ProviderHandle::<Workspace>::of(ctx);
        let (reload, collapse) = (store.clone(), store);
        let header = SizedBox::new()
            .width(Dimension::Percent(100.0))
            .height(HEADER_HEIGHT)
            .box_child(
                Container::new()
                    .padding(pad(10, 10, 2, WINDOW_BUTTONS))
                    .child(
                        Row::new()
                            .vertical_alignment(BoxAlignment::Center)
                            .gaps(LayoutSpacing::all(2))
                            .children([
                                Expanded::new().child(ZeroSizedBox).boxed(),
                                tool(Icons::REFRESH_CW, theme, move || {
                                    reload.update(|space| space.refresh());
                                    refresh_status(&reload);
                                })
                                .boxed(),
                                tool(Icons::CHEVRONS_DOWN_UP, theme, move || {
                                    collapse.update(|space| space.collapse_all())
                                })
                                .boxed(),
                            ]),
                    ),
            );
        Container::new().color(theme.sidebar).child(
            Column::new().children([header.boxed(), Expanded::new().child(FileTree).boxed()]),
        )
    }
}

/// The line down the right edge of the side panel, between it and the editor.
pub fn edge(theme: CodeTheme) -> impl Widget {
    rule::vertical(theme.divider)
}
