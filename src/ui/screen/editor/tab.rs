use super::context_menu::show_tab_menu;
use super::file_row::change_color;
use super::tab_drag::{DraggedTab, TabAutoScroll, TabDragArea};
use crate::ui::components::dot::dot;
use crate::ui::components::icon::{icon, material};
use crate::ui::components::pad::pad;
use crate::ui::components::rule;
use crate::ui::file_icons::file_icon;
use crate::ui::fonts;
use crate::services::git::Change;
use crate::ui::theme::CodeTheme;
use crate::model::workspace::Workspace;
use aimer::dnd::{DragSession, DragTarget, DragTargetState};
use aimer::events::pointer::PointerButton;
use aimer::input::gesture::GestureEvent;
use aimer::input::gesture::gesture_detector::GestureDetector;
use aimer::input::mouse_region::MouseRegion;
use aimer::style::{BorderSlice, BorderStyle, BoxBorder, BoxDecoration, LayoutSpacing, Theme};
use aimer::{
    AnyWidget, BoxAlignment, BuildContext, Color, Container, Icons, ProviderContext,
    ProviderHandle, Row, SizedBox, State, StateView, StatefulWidget, StatelessWidget, Text, Widget,
};
use std::path::PathBuf;
use std::rc::Rc;

/// How tall a tab is.
pub const HEIGHT: u32 = 38;

/// One open file in the row of tabs: its icon and name, the letter git gives it if it changed, and
/// a button to close it that is a dot while there are unsaved changes. The tab of the file being
/// edited is marked by a line along its top and the color of the editor behind it.
#[derive(Clone, StatelessWidget)]
pub struct Tab {
    id: u64,
    /// Position of this tab among open document tabs, used to decide whether the drop indicator appears on the left or right edge.
    index: usize,
    name: String,
    path: PathBuf,
    active: bool,
    /// Whether the tab is pinned to the left of the tab bar.
    pinned: bool,
    /// Whether the tab is the settings and not a file: it has a gear for an icon and no changes to show.
    settings: bool,
    /// Tab bar edge auto-scroller started when a tab drag begins.
    auto_scroll: Option<Rc<TabAutoScroll>>,
}

impl Tab {
    pub fn new(
        id: u64,
        index: usize,
        name: impl Into<String>,
        path: PathBuf,
        active: bool,
        pinned: bool,
        auto_scroll: Rc<TabAutoScroll>,
    ) -> Self {
        Self {
            id,
            index,
            name: name.into(),
            path,
            active,
            pinned,
            settings: false,
            auto_scroll: Some(auto_scroll),
        }
    }

    /// The tab of the settings, which is not a file: it has no path, and nothing in it changes.
    pub fn settings(active: bool) -> Self {
        Self {
            id: Workspace::SETTINGS_TAB,
            index: 0,
            name: "Settings".to_string(),
            path: PathBuf::new(),
            active,
            pinned: false,
            settings: true,
            auto_scroll: None,
        }
    }
}

impl StatelessWidget for Tab {
    fn build(&self, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let (id, index, active, pinned) = (self.id, self.index, self.active, self.pinned);
        let name = self.name.clone();
        let path = self.path.clone();
        let changed = ctx.select::<Workspace, bool>({
            let path = path.clone();
            move |space| space.is_changed(&path)
        });
        let change = ctx.select::<Workspace, Option<Change>>(move |space| space.change_of(&path));
        let store = ProviderHandle::<Workspace>::of(ctx);
        let show = store.clone();
        let close = store.clone();
        let name_menu = store.clone();
        let action_menu = store.clone();
        let drop_store = store;
        let color = match change {
            Some(change) => change_color(Some(change), &theme),
            None if active => theme.text,
            None => theme.muted,
        };
        let letter = change.and_then(Change::letter);
        let label = name.clone();
        let settings = self.settings;
        let auto_scroll = self.auto_scroll.clone();
        // A dot while there are changes, which turns into the cross (or unpin for pinned tabs) under the pointer;
        // a pin icon when pinned (pressing it unpins), or the cross otherwise.
        let close_button = TabActionButton {
            theme,
            active,
            pinned,
            changed,
            on_primary: Rc::new(move || {
                close.update(|space| {
                    if pinned {
                        space.toggle_pin_tab(id);
                    } else {
                        space.request_close(id);
                    }
                })
            }),
            on_secondary: Rc::new(move |pos| {
                show_tab_menu(theme, pos, id, action_menu.clone());
            }),
        };
        // The tab being edited has a line along its bottom, which is the border of its own box: a
        // widget of a percentage width would be as wide as the whole row of tabs.
        let base_bg = if active {
            theme.marker.blend(theme.bar, 0.9)
        } else {
            theme.bar
        };
        let make_body = move |drop_side: Option<bool>| {
            let leading: AnyWidget = if settings {
                icon(Icons::SETTINGS, 15, color).notype()
            } else {
                material(file_icon(&label), 16).notype()
            };
            let mut parts = vec![
                leading.boxed(),
                Text::new(label.clone())
                    .text_style(fonts::ui().font_size(13).color(color))
                    .boxed(),
            ];
            if let Some(letter) = letter {
                parts.push(
                    Text::new(letter)
                        .text_style(fonts::ui().font_size(12).color(color))
                        .boxed(),
                );
            }
            let show = show.clone();
            let name_menu = name_menu.clone();
            let drag_data = (!settings).then(|| DraggedTab {
                id,
                index,
                pinned,
                name: label.clone(),
            });
            let name_button = TabDragArea::new(
                drag_data,
                auto_scroll.clone(),
                theme,
                move || show.update(|space| space.activate(id)),
                move |pos| show_tab_menu(theme, pos, id, name_menu.clone()),
                Container::new()
                    .padding(pad(0, 4, 0, 14))
                    .child(
                        Row::new()
                            .vertical_alignment(BoxAlignment::Center)
                            .gaps(LayoutSpacing::all(7))
                            .children(parts),
                    ),
            );
            let mut border = BoxBorder::new();
            let mut has_border = false;
            if active {
                border = border.bottom(
                    BorderSlice::new()
                        .color(theme.marker)
                        .style(BorderStyle::Solid)
                        .stroke(2.0),
                );
                has_border = true;
            }
            if let Some(after) = drop_side {
                let marker = BorderSlice::new()
                    .color(theme.marker)
                    .style(BorderStyle::Solid)
                    .stroke(2.0);
                border = if after {
                    border.right(marker)
                } else {
                    border.left(marker)
                };
                has_border = true;
            }
            let mut look = BoxDecoration::new().background_color(base_bg);
            if has_border {
                look = look.border(border);
            }
            SizedBox::new().height(HEIGHT).box_child(
                Container::new()
                    .padding(LayoutSpacing::new().bottom(if active { 0 } else { 2 }))
                    .box_decoration(look)
                    .child(
                        Row::new()
                            .vertical_alignment(BoxAlignment::Center)
                            .children([name_button.boxed(), close_button.clone().boxed()]),
                    ),
            )
        };
        let tab_box: AnyWidget = if settings {
            make_body(None).notype()
        } else {
            DragTarget::<DraggedTab>::new()
                .will_accept(move |dragged: &DraggedTab| {
                    dragged.id != id && dragged.pinned == pinned
                })
                .on_accept(move |dragged: DraggedTab| {
                    drop_store.update(|space| {
                        space.reorder_tab(dragged.id, id);
                    });
                })
                .child(move |state: DragTargetState| {
                    let drop_side = (state.is_hovered && state.will_accept).then(|| {
                        DragSession::with_payload(|dragged: &DraggedTab| dragged.index < index)
                            .unwrap_or(false)
                    });
                    make_body(drop_side)
                })
                .notype()
        };
        SizedBox::new().height(HEIGHT).box_child(
            Row::new().children([tab_box.boxed(), rule::vertical(theme.divider).boxed()]),
        )
    }
}

/// The right-hand close / pin button of a tab, tracking its own hover state while forwarding right-clicks to the tab menu.
#[derive(Clone, StatefulWidget)]
struct TabActionButton {
    theme: CodeTheme,
    active: bool,
    pinned: bool,
    changed: bool,
    on_primary: Rc<dyn Fn()>,
    on_secondary: Rc<dyn Fn(aimer::Vec2d)>,
}

struct TabActionButtonState {
    hovered: bool,
    theme: CodeTheme,
    active: bool,
    pinned: bool,
    changed: bool,
    on_primary: Rc<dyn Fn()>,
    on_secondary: Rc<dyn Fn(aimer::Vec2d)>,
}

impl State<TabActionButton> for TabActionButtonState {
    fn create(widget: TabActionButton) -> Self {
        Self {
            hovered: false,
            theme: widget.theme,
            active: widget.active,
            pinned: widget.pinned,
            changed: widget.changed,
            on_primary: widget.on_primary,
            on_secondary: widget.on_secondary,
        }
    }

    fn adopt_config_from(&mut self, new: Self) {
        self.theme = new.theme;
        self.active = new.active;
        self.pinned = new.pinned;
        self.changed = new.changed;
        self.on_primary = new.on_primary;
        self.on_secondary = new.on_secondary;
    }

    fn build(this: StateView<Self>, _ctx: &BuildContext) -> impl Widget {
        let hovered = this.hovered;
        let theme = this.theme;
        let end: aimer::AnyWidget = if this.changed && !hovered {
            dot(8, theme.text).notype()
        } else if this.pinned {
            icon(
                if hovered { Icons::PIN_OFF } else { Icons::PIN },
                13,
                if hovered { theme.text } else { theme.marker },
            )
            .notype()
        } else {
            icon(
                Icons::X,
                13,
                if hovered {
                    theme.text
                } else if this.active {
                    theme.muted
                } else {
                    Color::Transparent
                },
            )
            .notype()
        };
        let on_primary = Rc::clone(&this.on_primary);
        let on_secondary = Rc::clone(&this.on_secondary);
        let enter = this.clone();
        let exit = this.clone();
        MouseRegion::new()
            .on_hover_enter(move || enter.set_state(|s| s.hovered = true))
            .on_hover_exit(move || exit.set_state(|s| s.hovered = false))
            .child(
                GestureDetector::new()
                    .on_gesture(move |event: GestureEvent| {
                        if let GestureEvent::Tap { pointer } = event {
                            match pointer.button {
                                PointerButton::Primary => on_primary(),
                                PointerButton::Secondary => on_secondary(pointer.pos),
                                _ => {}
                            }
                        }
                    })
                    .child(
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
                            ),
                    ),
            )
    }
}
