use crate::workspace::Workspace;
use aimer::events::file_open;
use aimer::file_picker::FilePicker;
use aimer::shortcut::menu::{AppMenu, Menu, set_app_menu};
use aimer::{
    AnyWidget, BuildContext, ChildBuilder, ProviderHandle, State, StateView, StatefulWidget, Venus, Widget,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The chord of the settings item, which is `Cmd+,` on a Mac and `Ctrl+,` elsewhere.
const SETTINGS_CHORD: &str = "CmdOrCtrl+,";

/// The chord of `File > Open…`.
const OPEN_CHORD: &str = "CmdOrCtrl+O";

/// The chord of `File > Open Folder…`.
const OPEN_FOLDER_CHORD: &str = "CmdOrCtrl+Shift+O";

thread_local! {
    /// What the settings item of the menu does, and the number of the registration that set it: the window sets
    /// it while it is on screen. The menu belongs to the app and is made before the window has a tree, so it
    /// can only reach the window through this.
    static OPEN_SETTINGS: RefCell<Option<(u64, Rc<dyn Fn()>)>> = const { RefCell::new(None) };
    /// The number the next registration gets.
    static NEXT_REGISTRATION: Cell<u64> = const { Cell::new(1) };
}

/// Makes the app's menu: `Settings…` in the app's own menu, as every Mac app has it. Call it once on the
/// UI thread, before the app starts, which is when the native menu bar is built.
pub fn install() {
    set_app_menu(
        AppMenu::new()
            .application_item("Settings\u{2026}", SETTINGS_CHORD, open_settings)
            .menu(
                Menu::new("File")
                    .item("Open\u{2026}", OPEN_CHORD, || pick(false))
                    .item("Open Folder\u{2026}", OPEN_FOLDER_CHORD, || pick(true)),
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

/// Keeps a handler of the menu registered for as long as it lives, unless another registration took its place.
struct Registration {
    number: u64,
}

impl Registration {
    fn new(handler: Rc<dyn Fn()>) -> Self {
        let number = NEXT_REGISTRATION.with(|next| next.replace(next.get() + 1));
        OPEN_SETTINGS.with(|slot| *slot.borrow_mut() = Some((number, handler)));
        Self { number }
    }
}

impl Drop for Registration {
    /// A screen that is built again makes its new state before the old one is dropped, so the old registration is
    /// dropped when the new one is the one in place: it leaves that alone.
    fn drop(&mut self) {
        // Taken out of the cell before it is dropped: the handler may own a handle whose drop reaches back here.
        let old = OPEN_SETTINGS.with(|slot| {
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
        drop(old);
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
        self.registration = Some(Registration::new(Rc::new(move || {
            store.update(|space| space.open_settings())
        })));
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
        assert!(KeyChord::parse(SETTINGS_CHORD).is_ok());
        assert!(KeyChord::parse(OPEN_CHORD).is_ok());
        assert!(KeyChord::parse(OPEN_FOLDER_CHORD).is_ok());
    }

    #[test]
    fn the_settings_item_reaches_the_window_that_registered_and_stops_when_it_is_gone() {
        install();
        let chord = KeyChord::parse(SETTINGS_CHORD).expect("the chord is valid");
        let (opened, handler) = counter();

        // No window yet: the item is there and does nothing.
        assert_eq!(activate_chord(&chord, false), Activation::Ran);
        assert_eq!(opened.get(), 0);

        let registration = Registration::new(handler);
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
    fn a_window_that_replaces_another_is_not_unregistered_when_the_old_one_is_dropped() {
        // A rebuild of the shell makes the new state before the old one is dropped, so the old registration is
        // dropped after the new one was made, and must leave what the new one set.
        install();
        let chord = KeyChord::parse(SETTINGS_CHORD).expect("the chord is valid");
        let (old_count, old_handler) = counter();
        let (new_count, new_handler) = counter();

        let old = Registration::new(old_handler);
        let new = Registration::new(new_handler);
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
            registration: Some(Registration::new(handler)),
        }
    }

    #[test]
    fn a_rebuild_of_the_shell_keeps_the_menu_reaching_the_window() {
        // What the framework does when a parent is built again (a change of theme does it): it makes a new state,
        // which runs `init_state` and so registers, hands it to the live state to take the configuration from,
        // and drops it. The live state has to be what the menu reaches after that.
        install();
        let chord = KeyChord::parse(SETTINGS_CHORD).expect("the chord is valid");
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
