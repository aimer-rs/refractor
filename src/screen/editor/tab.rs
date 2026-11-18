use super::file_row::change_color;
use crate::components::dot::dot;
use crate::components::icon::{icon, material};
use crate::components::pad::pad;
use crate::components::rule;
use crate::file_icons::file_icon;
use crate::fonts;
use crate::git::Change;
use crate::theme::CodeTheme;
use crate::workspace::Workspace;
use aimer::input::button::Button;
use aimer::style::{BorderSlice, BorderStyle, BoxBorder, BoxDecoration, LayoutSpacing, Theme};
use aimer::{
    BoxAlignment, BuildContext, Color, Container, Icons, ProviderContext, ProviderHandle, Row,
    SizedBox, StatelessWidget, Text, Widget,
};
use std::path::PathBuf;

/// How tall a tab is.
pub const HEIGHT: u32 = 38;

/// One open file in the row of tabs: its icon and name, the letter git gives it if it changed, and
/// a button to close it that is a dot while there are unsaved changes. The tab of the file being
/// edited is marked by a line along its top and the color of the editor behind it.
#[derive(Clone, StatelessWidget)]
pub struct Tab {
    id: u64,
    name: String,
    path: PathBuf,
    active: bool,
}

impl Tab {
    pub fn new(id: u64, name: impl Into<String>, path: PathBuf, active: bool) -> Self {
        Self {
            id,
            name: name.into(),
            path,
            active,
        }
    }
}

impl StatelessWidget for Tab {
    fn build(&self, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let (id, active) = (self.id, self.active);
        let name = self.name.clone();
        let path = self.path.clone();
        let changed = ctx.select::<Workspace, bool>({
            let path = path.clone();
            move |space| space.is_changed(&path)
        });
        let change = ctx.select::<Workspace, Option<Change>>(move |space| space.change_of(&path));
        let store = ProviderHandle::<Workspace>::of(ctx);
        let show = store.clone();
        let close = store;
        let color = match change {
            Some(change) => change_color(Some(change), &theme),
            None if active => theme.text,
            None => theme.muted,
        };
        let letter = change.and_then(Change::letter);
        let label = name.clone();
        // The name and the cross are two buttons side by side, and not one inside the other: a press
        // belongs to the innermost, and the outer one would be the only one to see it.
        let name_button = Button::new()
            .no_decor()
            .on_press(move || show.update(|space| space.activate(id)))
            .build(move |_| {
                let mut parts = vec![
                    material(file_icon(&label), 16).boxed(),
                    Text::new(label.clone())
                        .text_style(
                            fonts::ui()
                                .font_size(13)
                                .color(color),
                        )
                        .boxed(),
                ];
                if let Some(letter) = letter {
                    parts.push(
                        Text::new(letter)
                            .text_style(
                                fonts::ui()
                                    .font_size(12)
                                    .color(color),
                            )
                            .boxed(),
                    );
                }
                Container::new()
                    .padding(pad(0, 4, 0, 14))
                    .child(
                        Row::new()
                            .vertical_alignment(BoxAlignment::Center)
                            .gaps(LayoutSpacing::all(7))
                            .children(parts),
                    )
            });
        // A dot while there are changes, which turns into the cross under the pointer, as it does in
        // most editors; the cross otherwise.
        let close_button = Button::new()
            .no_decor()
            .on_press(move || close.update(|space| space.request_close(id)))
            .build(move |cross| {
                let end: aimer::AnyWidget = if changed && !cross.is_hovered {
                    dot(8, theme.text).notype()
                } else {
                    icon(
                        Icons::X,
                        13,
                        if cross.is_hovered {
                            theme.text
                        } else if active {
                            theme.muted
                        } else {
                            Color::Transparent
                        },
                    )
                    .notype()
                };
                // A container fills the height it is given, so the cross is centered by a row inside it.
                Container::new()
                    .padding(pad(0, 12, 0, 6))
                    .child(
                        Row::new()
                            .vertical_alignment(BoxAlignment::Center)
                            .children([SizedBox::new()
                                .width(14)
                                .height(14)
                                .box_child(end)
                                .boxed()]),
                    )
            });
        // The tab being edited has a line along its top, which is the border of its own box: a
        // widget of a percentage width would be as wide as the whole row of tabs.
        let mut look =
            BoxDecoration::new().background_color(if active { theme.marker.blend(theme.bar, 0.9) } else { theme.bar });
        if active {
            look = look.border(
                BoxBorder::new().bottom(
                    BorderSlice::new()
                        .color(theme.marker)
                        .style(BorderStyle::Solid)
                        .stroke(2.0),
                ),
            );
        }
        let body = SizedBox::new()
            .height(HEIGHT)
            .box_child(
                Container::new()
                    .padding(LayoutSpacing::new().bottom(if active { 0 } else { 2 }))
                    .box_decoration(look)
                    .child(
                        Row::new()
                            .vertical_alignment(BoxAlignment::Center)
                            .children([name_button.boxed(), close_button.boxed()]),
                    ),
            );
        SizedBox::new()
            .height(HEIGHT)
            .box_child(Row::new().children([body.boxed(), rule::vertical(theme.divider).boxed()]))
    }
}
