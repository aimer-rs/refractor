use crate::components::icon::{icon, material};
use crate::components::pad::pad;
use crate::file_icons::{file_icon, folder_icon};
use crate::fonts;
use crate::git::Change;
use crate::theme::CodeTheme;
use crate::tree::Row;
use crate::workspace::{Workspace, notifier};
use aimer::decoration::Decoration;
use aimer::input::button::Button;
use aimer::share::ShareRef;
use aimer::style::{LayoutSpacing, TextOverflow, Theme};
use aimer::{
    BoxAlignment, BuildContext, Color, Container, Dimension, Expanded, Icons, ProviderContext,
    ProviderHandle, Row as Line, SizedBox, StatelessWidget, Text, Widget,
};

/// How far each level of the tree is indented, in logical pixels.
const INDENT: u32 = 12;
/// How tall a row is.
const HEIGHT: u32 = 24;

/// The color of a name git has something to say of.
pub fn change_color(change: Option<Change>, theme: &CodeTheme) -> Color {
    match change {
        None => theme.text,
        Some(Change::Ignored) => theme.muted,
        Some(Change::Deleted) => theme.deleted,
        Some(Change::Conflict) => theme.warning,
        Some(_) => theme.accent,
    }
}

/// One row of the file tree: a folder, which opens and closes, or a file, which opens in a tab.
///
/// The row watches only what concerns it, whether its file is the one shown and what git says of
/// it, so typing does not rebuild the tree.
#[derive(Clone, StatelessWidget)]
pub struct FileRow {
    row: ShareRef<Row>,
}

impl FileRow {
    pub fn new(row: ShareRef<Row>) -> Self {
        Self { row }
    }
}

impl StatelessWidget for FileRow {
    fn build(&self, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let path = self.row.path.clone();
        let shown = ctx.select::<Workspace, bool>({
            let path = path.clone();
            move |space| {
                space
                    .active_document()
                    .is_some_and(|document| document.path == path)
            }
        });
        let (change, inside) = ctx.select::<Workspace, (Option<Change>, bool)>({
            let path = path.clone();
            move |space| (space.change_of(&path), space.has_changes_inside(&path))
        });
        let store = ProviderHandle::<Workspace>::of(ctx);
        let row = self.row.clone();

        Button::new()
            .no_decor()
            .on_press({
                let row = self.row.clone();
                move || {
                    if row.is_dir {
                        store.update(|space| space.toggle(&row.path));
                    } else {
                        let notify = notifier(&store);
                        // The reason a file could not be opened is in the workspace's message.
                        store.update(|space| {
                            let _ = space.open(&row.path, notify);
                        });
                    }
                }
            })
            .build(move |state| {
                let background = if shown {
                    theme.selected
                } else if state.is_hovered {
                    theme.hover
                } else {
                    Color::Transparent
                };
                // A folder with changes inside is named in the color of a change, though it has none itself.
                let name_color = match (change, row.is_dir && inside) {
                    (None, true) => theme.accent,
                    (change, _) => change_color(change, &theme),
                };
                let lead: aimer::AnyWidget = if row.is_dir {
                    icon(
                        if row.expanded {
                            Icons::CHEVRON_DOWN
                        } else {
                            Icons::CHEVRON_RIGHT
                        },
                        14,
                        theme.muted,
                    )
                    .notype()
                } else {
                    SizedBox::new()
                        .width(14)
                        .notype()
                };
                let kind = material(
                    if row.is_dir {
                        folder_icon(&row.name)
                    } else {
                        file_icon(&row.name)
                    },
                    16,
                );
                // The letter of a change, or a dot on a folder with changes inside it.
                let mark: aimer::AnyWidget =
                    match (change.and_then(Change::letter), row.is_dir && inside) {
                        (Some(letter), _) => Text::new(letter)
                            .text_style(
                                fonts::ui()
                                    .font_size(12)
                                    .color(name_color),
                            )
                            .notype(),
                        (None, true) => crate::components::dot::dot(6, theme.accent).notype(),
                        (None, false) => SizedBox::new()
                            .width(1)
                            .notype(),
                    };
                SizedBox::new()
                    .width(Dimension::Percent(100.0))
                    .height(HEIGHT)
                    .box_child(
                        Container::new()
                            .width(Dimension::Percent(100.0))
                            .padding(pad(0, 14, 0, 8 + INDENT * row.depth as u32))
                            .decoration([background.notype()])
                            .child(
                                Line::new()
                                    .vertical_alignment(BoxAlignment::Center)
                                    .gaps(LayoutSpacing::all(6))
                                    .children([
                                        lead.boxed(),
                                        kind.boxed(),
                                        Expanded::new()
                                            .child(
                                                Text::new(row.name.clone()).text_style(
                                                    fonts::ui()
                                                        .font_size(13)
                                                        .color(name_color)
                                                        .text_overflow(TextOverflow::Ellipsis)
                                                        .font_weight(aimer::FontWeight::Normal),
                                                ),
                                            )
                                            .boxed(),
                                        mark.boxed(),
                                    ]),
                            ),
                    )
            })
    }
}
