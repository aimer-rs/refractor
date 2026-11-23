use crate::services::settings::Settings;
use crate::ui::tab_menu::Action as TabAction;
use crate::ui::tree_menu::Action as TreeAction;
use crate::model::workspace::Workspace;
use aimer::events::file_open;
use aimer::file_picker::FilePicker;
use aimer::shortcut::menu::{AppMenu, Menu, set_app_menu};
use aimer::{
    AnyWidget, BuildContext, ChildBuilder, ProviderHandle, State, StateView, StatefulWidget, Venus,
    Widget,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub struct AimerKeyChord;

impl AimerKeyChord {
    /// The chord of the settings item, which is `Cmd+,` on a Mac and `Ctrl+,` elsewhere.
    const SETTINGS_CHORD: &str = "CmdOrCtrl+,";

    /// The chord of `File > New File`.
    const NEW_FILE_CHORD: &str = TreeAction::NewFile.chord().expect("chord");

    /// The chord of `File > New Folder`.
    const NEW_FOLDER_CHORD: &str = TreeAction::NewFolder.chord().expect("chord");

    /// The chord of `File > Open…`.
    const OPEN_CHORD: &str = "CmdOrCtrl+O";

    /// The chord of `File > Open Folder…`.
    const OPEN_FOLDER_CHORD: &str = "CmdOrCtrl+Shift+O";

    /// The chord of `File > Copy Path`.
    const COPY_PATH_CHORD: &str = TreeAction::CopyPath.chord().expect("chord");

    /// The chord of `File > Show in Finder` (or `Show in Explorer`).
    const SHOW_IN_FILE_MANAGER_CHORD: &str = TreeAction::ShowInFileManager
        .chord()
        .expect("chord");

    /// The chord of `File > Rename…`.
    const RENAME_CHORD: &str = TreeAction::Rename.chord().expect("chord");

    /// The chord of `File > Delete`.
    const DELETE_CHORD: &str = TreeAction::Delete.chord().expect("chord");

    /// The chord of `Navigation > Focus File Tree` (toggles between file tree and editor).
    const FOCUS_TREE_CHORD: &str = "CmdOrCtrl+1";

    /// The chord of `Navigation > Previous Tab`.
    const PREV_TAB_CHORD: &str = "CmdOrCtrl+Left";

    /// The chord of `Navigation > Next Tab`.
    const NEXT_TAB_CHORD: &str = "CmdOrCtrl+Right";

    /// The chord of `Navigation > Close Tab`.
    const CLOSE_TAB_CHORD: &str = "CmdOrCtrl+W";

    /// The chord of `Navigation > Go to Line/Column…`.
    const GO_TO_LINE_CHORD: &str = "CmdOrCtrl+L";

    /// The chord of `Code > Reformat Code`.
    const REFORMAT_CODE_CHORD: &str = "Cmd+Option+Control+R";

    /// The chord of `Git > Toggle Git Panel`.
    const TOGGLE_GIT_PANEL_CHORD: &str = "CmdOrCtrl+2";

    /// The chord of `Git > Commit…`.
    const GIT_COMMIT_CHORD: &str = "CmdOrCtrl+K";

    /// The chord of `Git > Push…`.
    const GIT_PUSH_CHORD: &str = "CmdOrCtrl+Shift+K";
}

/// A command from the `Git` menu bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GitAction {
    TogglePanel,
    Commit,
    Push,
}

/// A command from the `Navigation` menu bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NavAction {
    ToggleFileTree,
    PreviousTab,
    NextTab,
    CloseTab,
    GoToLine,
}

thread_local! {
    /// What the settings item of the menu does, and the number of the registration that set it: the window sets
    /// it while it is on screen. The menu belongs to the app and is made before the window has a tree, so it
    /// can only reach the window through this.
    static OPEN_SETTINGS: RefCell<Option<(u64, Rc<dyn Fn()>)>> = const { RefCell::new(None) };
    /// What the File menu's tree commands do for the active window.
    static TREE_ACTION: RefCell<Option<(u64, Rc<dyn Fn(TreeAction)>)>> = const { RefCell::new(None) };
    /// What the Navigation menu commands do for the active window.
    static NAV_ACTION: RefCell<Option<(u64, Rc<dyn Fn(NavAction)>)>> = const { RefCell::new(None) };
    /// What the Tabs menu commands do for the active window.
    static TAB_ACTION: RefCell<Option<(u64, Rc<dyn Fn(TabAction)>)>> = const { RefCell::new(None) };
    /// What `Code > Reformat Code` does for the active window.
    static REFORMAT_CODE: RefCell<Option<(u64, Rc<dyn Fn()>)>> = const { RefCell::new(None) };
    /// What the `Git` menu commands do for the active window.
    static GIT_ACTION: RefCell<Option<(u64, Rc<dyn Fn(GitAction)>)>> = const { RefCell::new(None) };
    /// The number the next registration gets.
    static NEXT_REGISTRATION: Cell<u64> = const { Cell::new(1) };
}

/// Makes the app's menu: `Settings…` in the app's own menu, as every Mac app has it, and the file actions in `File` and `Navigation`.
/// Call it once on the UI thread, before the app starts, which is when the native menu bar is built.
pub fn install() {
    let mac = cfg!(any(target_os = "macos", target_os = "ios"));
    set_app_menu(
        AppMenu::new()
            .application_item(
                "Settings\u{2026}",
                AimerKeyChord::SETTINGS_CHORD,
                open_settings,
            )
            .menu(
                Menu::new("File")
                    .item(
                        TreeAction::NewFile.label(mac),
                        AimerKeyChord::NEW_FILE_CHORD,
                        || dispatch_tree_action(TreeAction::NewFile),
                    )
                    .item(
                        TreeAction::NewFolder.label(mac),
                        AimerKeyChord::NEW_FOLDER_CHORD,
                        || dispatch_tree_action(TreeAction::NewFolder),
                    )
                    .separator()
                    .item("Open\u{2026}", AimerKeyChord::OPEN_CHORD, || pick(false))
                    .item(
                        "Open Folder\u{2026}",
                        AimerKeyChord::OPEN_FOLDER_CHORD,
                        || pick(true),
                    )
                    .separator()
                    .item(
                        TreeAction::CopyPath.label(mac),
                        AimerKeyChord::COPY_PATH_CHORD,
                        || dispatch_tree_action(TreeAction::CopyPath),
                    )
                    .item(
                        TreeAction::ShowInFileManager.label(mac),
                        AimerKeyChord::SHOW_IN_FILE_MANAGER_CHORD,
                        || dispatch_tree_action(TreeAction::ShowInFileManager),
                    )
                    .separator()
                    .item(
                        TreeAction::Rename.label(mac),
                        AimerKeyChord::RENAME_CHORD,
                        || dispatch_tree_action(TreeAction::Rename),
                    )
                    .item(
                        TreeAction::Delete.label(mac),
                        AimerKeyChord::DELETE_CHORD,
                        || dispatch_tree_action(TreeAction::Delete),
                    ),
            )
            .menu(
                Menu::new("Navigation")
                    .item("Focus File Tree", AimerKeyChord::FOCUS_TREE_CHORD, || {
                        dispatch_nav_action(NavAction::ToggleFileTree)
                    })
                    .separator()
                    .item("Previous Tab", AimerKeyChord::PREV_TAB_CHORD, || {
                        dispatch_nav_action(NavAction::PreviousTab)
                    })
                    .item("Next Tab", AimerKeyChord::NEXT_TAB_CHORD, || {
                        dispatch_nav_action(NavAction::NextTab)
                    })
                    .separator()
                    .item("Close Tab", AimerKeyChord::CLOSE_TAB_CHORD, || {
                        dispatch_nav_action(NavAction::CloseTab)
                    })
                    .separator()
                    .item(
                        "Go to Line/Column\u{2026}",
                        AimerKeyChord::GO_TO_LINE_CHORD,
                        || dispatch_nav_action(NavAction::GoToLine),
                    ),
            )
            .menu(
                Menu::new("Tabs")
                    .item_with("Close", None, || dispatch_tab_action(TabAction::Close))
                    .item_with("Close All", None, || {
                        dispatch_tab_action(TabAction::CloseAll)
                    })
                    .item_with("Close Other", None, || {
                        dispatch_tab_action(TabAction::CloseOther)
                    })
                    .separator()
                    .item_with("Pin", None, || dispatch_tab_action(TabAction::Pin)),
            )
            .menu(Menu::new("Code").item(
                "Reformat Code",
                AimerKeyChord::REFORMAT_CODE_CHORD,
                reformat_code,
            ))
            .menu(
                Menu::new("Git")
                    .item(
                        "Toggle Git Panel",
                        AimerKeyChord::TOGGLE_GIT_PANEL_CHORD,
                        || dispatch_git_action(GitAction::TogglePanel),
                    )
                    .separator()
                    .item("Commit\u{2026}", AimerKeyChord::GIT_COMMIT_CHORD, || {
                        dispatch_git_action(GitAction::Commit)
                    })
                    .item("Push\u{2026}", AimerKeyChord::GIT_PUSH_CHORD, || {
                        dispatch_git_action(GitAction::Push)
                    }),
            ),
    );
}

/// What `File > Open…` and `File > Open Folder…` do: show the system's picker, for files or for a folder, and
/// hand what is chosen to the same handler that takes what is dropped on the app's icon (a folder other than
/// the open one gets its own window). Does nothing when the picker is dismissed or there is no runtime.
fn pick(folder: bool) {
    let Some(venus) = Venus::current() else {
        return;
    };
    venus.spawn(async move {
        let picker = FilePicker::new()
            .title(if folder { "Open Folder" } else { "Open" })
            .folder(folder)
            .multiple(!folder);
        let Ok(files) = picker.pick().await else {
            return;
        };
        let paths: Vec<_> = files
            .iter()
            .filter_map(|file| file.path().map(ToOwned::to_owned))
            .collect();
        if !paths.is_empty() {
            file_open::deliver_files(&paths);
        }
    });
}

/// What the settings item does: asks the window that is on screen to open the settings, and does nothing
/// when there is none.
fn open_settings() {
    // Cloned out first: the handler updates the workspace, which must not run with the cell borrowed.
    let handler = OPEN_SETTINGS.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|(_, handler)| Rc::clone(handler))
    });
    if let Some(handler) = handler {
        handler();
    }
}

/// Dispatches a `File` menu tree command to the window on screen, if there is one.
fn dispatch_tree_action(action: TreeAction) {
    let handler = TREE_ACTION.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|(_, handler)| Rc::clone(handler))
    });
    if let Some(handler) = handler {
        handler(action);
    }
}

/// Dispatches a `Navigation` menu command to the window on screen, if there is one.
fn dispatch_nav_action(action: NavAction) {
    let handler = NAV_ACTION.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|(_, handler)| Rc::clone(handler))
    });
    if let Some(handler) = handler {
        handler(action);
    }
}

/// Dispatches a `Tabs` menu command to the window on screen, if there is one.
fn dispatch_tab_action(action: TabAction) {
    let handler = TAB_ACTION.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|(_, handler)| Rc::clone(handler))
    });
    if let Some(handler) = handler {
        handler(action);
    }
}

/// Dispatches `Code > Reformat Code` to the window on screen, if there is one.
fn reformat_code() {
    let handler = REFORMAT_CODE.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|(_, handler)| Rc::clone(handler))
    });
    if let Some(handler) = handler {
        handler();
    }
}

/// Dispatches a `Git` menu command to the window on screen, if there is one.
fn dispatch_git_action(action: GitAction) {
    let handler = GIT_ACTION.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|(_, handler)| Rc::clone(handler))
    });
    if let Some(handler) = handler {
        handler(action);
    }
}

/// Keeps a handler of the menu registered for as long as it lives, unless another registration took its place.
struct Registration {
    number: u64,
}

impl Registration {
    fn new(
        settings: Rc<dyn Fn()>,
        tree: Rc<dyn Fn(TreeAction)>,
        nav: Rc<dyn Fn(NavAction)>,
        tab: Rc<dyn Fn(TabAction)>,
        reformat: Rc<dyn Fn()>,
    ) -> Self {
        let number = NEXT_REGISTRATION.with(|next| next.replace(next.get() + 1));
        OPEN_SETTINGS.with(|slot| *slot.borrow_mut() = Some((number, settings)));
        TREE_ACTION.with(|slot| *slot.borrow_mut() = Some((number, tree)));
        NAV_ACTION.with(|slot| *slot.borrow_mut() = Some((number, nav)));
        TAB_ACTION.with(|slot| *slot.borrow_mut() = Some((number, tab)));
        REFORMAT_CODE.with(|slot| *slot.borrow_mut() = Some((number, reformat)));
        Self { number }
    }
}

impl Drop for Registration {
    /// A screen that is built again makes its new state before the old one is dropped, so the old registration is
    /// dropped when the new one is the one in place: it leaves that alone.
    fn drop(&mut self) {
        // Taken out of the cell before it is dropped: the handler may own a handle whose drop reaches back here.
        let old_settings = OPEN_SETTINGS.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot
                .as_ref()
                .is_some_and(|(number, _)| *number == self.number)
            {
                slot.take()
            } else {
                None
            }
        });
        let old_tree = TREE_ACTION.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot
                .as_ref()
                .is_some_and(|(number, _)| *number == self.number)
            {
                slot.take()
            } else {
                None
            }
        });
        let old_nav = NAV_ACTION.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot
                .as_ref()
                .is_some_and(|(number, _)| *number == self.number)
            {
                slot.take()
            } else {
                None
            }
        });
        let old_tab = TAB_ACTION.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot
                .as_ref()
                .is_some_and(|(number, _)| *number == self.number)
            {
                slot.take()
            } else {
                None
            }
        });
        let old_reformat = REFORMAT_CODE.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot
                .as_ref()
                .is_some_and(|(number, _)| *number == self.number)
            {
                slot.take()
            } else {
                None
            }
        });
        let old_git = GIT_ACTION.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot
                .as_ref()
                .is_some_and(|(number, _)| *number == self.number)
            {
                slot.take()
            } else {
                None
            }
        });
        drop(old_settings);
        drop(old_tree);
        drop(old_nav);
        drop(old_tab);
        drop(old_reformat);
        drop(old_git);
    }
}

/// Connects the items of the app's menu to the window: `Settings…` opens the settings tab of the workspace.
///
/// It draws only its child.
#[derive(Clone, StatefulWidget)]
pub struct MenuCommands {
    child: ChildBuilder,
}

impl MenuCommands {
    pub fn new(child: impl Widget + 'static) -> Self {
        Self {
            child: ChildBuilder::from_widget(child),
        }
    }
}

struct MenuCommandsState {
    child: ChildBuilder,
    /// Keeps the menu reaching this window for as long as the window has this screen.
    registration: Option<Registration>,
}

impl State<MenuCommands> for MenuCommandsState {
    fn create(widget: MenuCommands) -> Self {
        Self {
            child: widget.child,
            registration: None,
        }
    }

    fn init_state(&mut self) {
        let store = ProviderHandle::<Workspace>::of(BuildContext::current().get());
        let settings = ProviderHandle::<Settings>::of(BuildContext::current().get());
        let settings_store = store.clone();
        let nav_store = store.clone();
        let tab_store = store.clone();
        let format_store = store.clone();
        let git_store = store.clone();
        let reg = Registration::new(
            Rc::new(move || settings_store.update(|space| space.open_settings())),
            Rc::new(move |action| {
                let target = store.read().target_for_menu_bar();
                crate::model::workspace::run_tree_action(action, target, &store);
            }),
            Rc::new(move |action| match action {
                NavAction::ToggleFileTree => nav_store.update(|space| space.toggle_tree_focus()),
                NavAction::PreviousTab => nav_store.update(|space| space.step_tab(-1)),
                NavAction::NextTab => nav_store.update(|space| space.step_tab(1)),
                NavAction::CloseTab => nav_store.update(|space| space.close_active_tab()),
                NavAction::GoToLine => nav_store.read().open_go_to_line(),
            }),
            Rc::new(move |action| {
                tab_store.update(|space| space.run_tab_action_on_active(action));
            }),
            Rc::new(move || {
                let (formatter, tab_spaces) = {
                    let settings = settings.read();
                    (settings.formatter(), settings.tab_spaces())
                };
                crate::model::workspace::format_active_document(&format_store, formatter, tab_spaces);
            }),
        );
        let reg_num = reg.number;
        GIT_ACTION.with(|slot| {
            *slot.borrow_mut() = Some((
                reg_num,
                Rc::new(move |action| match action {
                    GitAction::TogglePanel => crate::model::workspace::toggle_git_panel(&git_store),
                    GitAction::Commit => crate::model::workspace::open_git_commit(&git_store),
                    GitAction::Push => crate::model::workspace::open_git_push(&git_store),
                }),
            ));
        });
        self.registration = Some(reg);
    }

    fn adopt_config_from(&mut self, mut new: Self) {
        self.child = new.child;
        // The new state ran `init_state` and registered, and it is dropped when this returns. The live state takes
        // its registration over, so what is registered is kept by what stays; the one it had is the one that goes.
        self.registration = new.registration.take();
    }

    fn build(this: StateView<Self>, _ctx: &BuildContext) -> impl Widget {
        AnyWidget::from(this.child.clone().boxed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aimer::shortcut::menu::activate_chord;
    use aimer::shortcut::{Activation, KeyChord};
    use std::cell::Cell;

    fn counter() -> (Rc<Cell<u32>>, Rc<dyn Fn()>) {
        let count = Rc::new(Cell::new(0));
        let counted = Rc::clone(&count);
        (count, Rc::new(move || counted.set(counted.get() + 1)))
    }

    #[test]
    fn the_settings_chord_is_one_the_framework_reads() {
        for chord in [
            AimerKeyChord::SETTINGS_CHORD,
            AimerKeyChord::NEW_FILE_CHORD,
            AimerKeyChord::NEW_FOLDER_CHORD,
            AimerKeyChord::OPEN_CHORD,
            AimerKeyChord::OPEN_FOLDER_CHORD,
            AimerKeyChord::COPY_PATH_CHORD,
            AimerKeyChord::SHOW_IN_FILE_MANAGER_CHORD,
            AimerKeyChord::RENAME_CHORD,
            AimerKeyChord::DELETE_CHORD,
            AimerKeyChord::FOCUS_TREE_CHORD,
            AimerKeyChord::PREV_TAB_CHORD,
            AimerKeyChord::NEXT_TAB_CHORD,
            AimerKeyChord::CLOSE_TAB_CHORD,
            AimerKeyChord::GO_TO_LINE_CHORD,
            AimerKeyChord::REFORMAT_CODE_CHORD,
        ] {
            assert!(KeyChord::parse(chord).is_ok(), "{chord}");
        }
        assert_eq!(AimerKeyChord::REFORMAT_CODE_CHORD, "Cmd+Option+Control+R");
    }

    #[test]
    fn the_code_menu_lists_reformat_code_and_dispatches_the_chord_to_the_window() {
        install();
        aimer::shortcut::menu::with_app_menu(|app_menu| {
            let app_menu = app_menu.expect("installed");
            let code = app_menu
                .menus()
                .iter()
                .find(|menu| menu.title() == "Code")
                .expect("Code menu");
            let titles: Vec<&str> = code
                .entries()
                .iter()
                .filter_map(|entry| match entry {
                    aimer::shortcut::menu::MenuEntry::Item(item) => Some(item.title()),
                    aimer::shortcut::menu::MenuEntry::Separator => None,
                })
                .collect();
            assert_eq!(titles, ["Reformat Code"]);
        });
        let chord =
            KeyChord::parse(AimerKeyChord::REFORMAT_CODE_CHORD).expect("the chord is valid");
        let (formatted, handler) = counter();
        let registration =
            Registration::new(Rc::new(|| {}), noop_tree(), noop_nav(), noop_tab(), handler);
        assert_eq!(activate_chord(&chord, false), Activation::Ran);
        assert_eq!(formatted.get(), 1);
        drop(registration);
        assert_eq!(activate_chord(&chord, false), Activation::Ran);
        assert_eq!(formatted.get(), 1);
    }

    #[test]
    fn the_navigation_menu_lists_focus_tab_switching_and_close_tab() {
        install();
        aimer::shortcut::menu::with_app_menu(|app_menu| {
            let app_menu = app_menu.expect("installed");
            let nav = app_menu
                .menus()
                .iter()
                .find(|menu| menu.title() == "Navigation")
                .expect("Navigation menu");
            let titles: Vec<&str> = nav
                .entries()
                .iter()
                .filter_map(|entry| match entry {
                    aimer::shortcut::menu::MenuEntry::Item(item) => Some(item.title()),
                    aimer::shortcut::menu::MenuEntry::Separator => None,
                })
                .collect();
            assert_eq!(
                titles,
                [
                    "Focus File Tree",
                    "Previous Tab",
                    "Next Tab",
                    "Close Tab",
                    "Go to Line/Column\u{2026}",
                ]
            );
        });
    }

    #[test]
    fn the_file_menu_lists_the_file_tree_commands_with_their_chords() {
        install();
        let mac = cfg!(any(target_os = "macos", target_os = "ios"));
        aimer::shortcut::menu::with_app_menu(|app_menu| {
            let app_menu = app_menu.expect("installed");
            let file = app_menu
                .menus()
                .iter()
                .find(|menu| menu.title() == "File")
                .expect("File menu");
            let titles: Vec<&str> = file
                .entries()
                .iter()
                .filter_map(|entry| match entry {
                    aimer::shortcut::menu::MenuEntry::Item(item) => Some(item.title()),
                    aimer::shortcut::menu::MenuEntry::Separator => None,
                })
                .collect();
            assert_eq!(
                titles,
                [
                    "New File",
                    "New Folder",
                    "Open\u{2026}",
                    "Open Folder\u{2026}",
                    "Copy Path",
                    TreeAction::ShowInFileManager.label(mac),
                    "Rename\u{2026}",
                    "Delete",
                ]
            );
        });
    }

    fn noop_tree() -> Rc<dyn Fn(TreeAction)> {
        Rc::new(|_| {})
    }

    fn noop_nav() -> Rc<dyn Fn(NavAction)> {
        Rc::new(|_| {})
    }

    fn noop_tab() -> Rc<dyn Fn(TabAction)> {
        Rc::new(|_| {})
    }

    fn noop_reformat() -> Rc<dyn Fn()> {
        Rc::new(|| {})
    }

    #[test]
    fn the_tabs_menu_lists_close_close_all_close_other_and_pin_and_dispatches_to_the_window() {
        install();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let recorded = Rc::clone(&seen);
        let registration = Registration::new(
            Rc::new(|| {}),
            noop_tree(),
            noop_nav(),
            Rc::new(move |action| recorded.borrow_mut().push(action)),
            noop_reformat(),
        );
        let (titles, ids) = aimer::shortcut::menu::with_app_menu(|app_menu| {
            let app_menu = app_menu.expect("installed");
            let tabs = app_menu
                .menus()
                .iter()
                .find(|menu| menu.title() == "Tabs")
                .expect("Tabs menu");
            let mut titles = Vec::new();
            let mut ids = Vec::new();
            for entry in tabs.entries() {
                if let aimer::shortcut::menu::MenuEntry::Item(item) = entry {
                    titles.push(item.title().to_owned());
                    ids.push(item.id());
                }
            }
            (titles, ids)
        });
        assert_eq!(titles, ["Close", "Close All", "Close Other", "Pin"]);
        for id in ids {
            assert!(aimer::shortcut::menu::activate_item(id));
        }
        assert_eq!(
            *seen.borrow(),
            vec![
                TabAction::Close,
                TabAction::CloseAll,
                TabAction::CloseOther,
                TabAction::Pin,
            ]
        );
        drop(registration);
    }

    #[test]
    fn the_settings_item_reaches_the_window_that_registered_and_stops_when_it_is_gone() {
        install();
        let chord = KeyChord::parse(AimerKeyChord::SETTINGS_CHORD).expect("the chord is valid");
        let (opened, handler) = counter();

        // No window yet: the item is there and does nothing.
        assert_eq!(activate_chord(&chord, false), Activation::Ran);
        assert_eq!(opened.get(), 0);

        let registration = Registration::new(
            handler,
            noop_tree(),
            noop_nav(),
            noop_tab(),
            noop_reformat(),
        );
        assert_eq!(activate_chord(&chord, false), Activation::Ran);
        assert_eq!(opened.get(), 1);
        assert_eq!(
            activate_chord(&chord, true),
            Activation::Held,
            "holding the keys does not open it again"
        );
        assert_eq!(opened.get(), 1);

        drop(registration);
        assert_eq!(activate_chord(&chord, false), Activation::Ran);
        assert_eq!(opened.get(), 1, "a window that is gone is not asked");
    }

    #[test]
    fn the_file_menu_tree_chords_reach_the_registered_handler() {
        install();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let recorded = Rc::clone(&seen);
        let registration = Registration::new(
            Rc::new(|| {}),
            Rc::new(move |action| recorded.borrow_mut().push(action)),
            noop_nav(),
            noop_tab(),
            noop_reformat(),
        );
        for (chord_str, expected) in [
            (AimerKeyChord::NEW_FILE_CHORD, TreeAction::NewFile),
            (AimerKeyChord::NEW_FOLDER_CHORD, TreeAction::NewFolder),
            (AimerKeyChord::COPY_PATH_CHORD, TreeAction::CopyPath),
            (
                AimerKeyChord::SHOW_IN_FILE_MANAGER_CHORD,
                TreeAction::ShowInFileManager,
            ),
            (AimerKeyChord::RENAME_CHORD, TreeAction::Rename),
            (AimerKeyChord::DELETE_CHORD, TreeAction::Delete),
        ] {
            let chord = KeyChord::parse(chord_str).expect("valid chord");
            assert_eq!(
                activate_chord(&chord, false),
                Activation::Ran,
                "{chord_str}"
            );
            assert_eq!(seen.borrow().last().copied(), Some(expected));
        }
        drop(registration);
    }

    #[test]
    fn the_navigation_menu_chords_reach_the_registered_handler() {
        install();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let recorded = Rc::clone(&seen);
        let registration = Registration::new(
            Rc::new(|| {}),
            noop_tree(),
            Rc::new(move |action| recorded.borrow_mut().push(action)),
            noop_tab(),
            noop_reformat(),
        );
        for (chord_str, expected) in [
            (AimerKeyChord::FOCUS_TREE_CHORD, NavAction::ToggleFileTree),
            (AimerKeyChord::PREV_TAB_CHORD, NavAction::PreviousTab),
            (AimerKeyChord::NEXT_TAB_CHORD, NavAction::NextTab),
            (AimerKeyChord::CLOSE_TAB_CHORD, NavAction::CloseTab),
        ] {
            let chord = KeyChord::parse(chord_str).expect("valid chord");
            assert_eq!(
                activate_chord(&chord, false),
                Activation::Ran,
                "{chord_str}"
            );
            assert_eq!(seen.borrow().last().copied(), Some(expected));
        }
        drop(registration);
    }

    #[test]
    fn a_window_that_replaces_another_is_not_unregistered_when_the_old_one_is_dropped() {
        // A rebuild of the shell makes the new state before the old one is dropped, so the old registration is
        // dropped after the new one was made, and must leave what the new one set.
        install();
        let chord = KeyChord::parse(AimerKeyChord::SETTINGS_CHORD).expect("the chord is valid");
        let (old_count, old_handler) = counter();
        let (new_count, new_handler) = counter();

        let old = Registration::new(
            old_handler,
            noop_tree(),
            noop_nav(),
            noop_tab(),
            noop_reformat(),
        );
        let new = Registration::new(
            new_handler,
            noop_tree(),
            noop_nav(),
            noop_tab(),
            noop_reformat(),
        );
        drop(old);
        assert_eq!(activate_chord(&chord, false), Activation::Ran);
        assert_eq!(
            (old_count.get(), new_count.get()),
            (0, 1),
            "the window that is there is the one asked"
        );

        drop(new);
        assert_eq!(activate_chord(&chord, false), Activation::Ran);
        assert_eq!(new_count.get(), 1, "and with it gone nothing is");
    }

    fn state_with(handler: Rc<dyn Fn()>) -> MenuCommandsState {
        MenuCommandsState {
            child: ChildBuilder::from_widget(aimer::ZeroSizedBox),
            registration: Some(Registration::new(
                handler,
                noop_tree(),
                noop_nav(),
                noop_tab(),
                noop_reformat(),
            )),
        }
    }

    #[test]
    fn a_rebuild_of_the_shell_keeps_the_menu_reaching_the_window() {
        // What the framework does when a parent is built again (a change of theme does it): it makes a new state,
        // which runs `init_state` and so registers, hands it to the live state to take the configuration from,
        // and drops it. The live state has to be what the menu reaches after that.
        install();
        let chord = KeyChord::parse(AimerKeyChord::SETTINGS_CHORD).expect("the chord is valid");
        let (live_count, live_handler) = counter();
        let (new_count, new_handler) = counter();
        let mut live = state_with(live_handler);
        let new = state_with(new_handler);

        live.adopt_config_from(new);
        assert_eq!(activate_chord(&chord, false), Activation::Ran);
        assert_eq!(
            (live_count.get(), new_count.get()),
            (0, 1),
            "the window is reached by what its live state registered last"
        );

        drop(live);
        assert_eq!(activate_chord(&chord, false), Activation::Ran);
        assert_eq!(new_count.get(), 1, "and when the window is gone nothing is");
    }
}
