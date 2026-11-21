mod choice;
mod setting_row;
mod stepper;

use self::choice::Choice;
use self::setting_row::{SECTION_GAP, section_title, setting_row};
use self::stepper::Stepper;
use crate::components::pad::pad;
use crate::fonts;
use crate::settings::{AutoSave, Settings, ThemeMode};
use crate::theme::CodeTheme;
use aimer::style::{LayoutSpacing, Theme};
use aimer::{
    BuildContext, Column, Container, Dimension, ProviderContext, ProviderHandle, Scrollable,
    SizedBox, StatelessWidget, Text, Widget,
};
use std::rc::Rc;

/// What the settings tab shows: the settings that can be changed, in sections, each a row with its name,
/// a line that says what it does, and the control that changes it. A change is made at once, is drawn
/// by everything that shows it, and is written to the settings file.
#[derive(Clone, StatelessWidget)]
pub struct SettingsPage;

/// Makes `edit` to the settings and keeps them: the editor follows the change, and so does the file.
fn change(store: &ProviderHandle<Settings>, edit: impl FnOnce(&mut Settings)) {
    store.update(edit);
    store.read().save();
}

impl StatelessWidget for SettingsPage {
    fn build(&self, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let (font_size, tab_spaces, mode, auto_save, auto_save_delay) =
            ctx.select::<Settings, (u32, usize, ThemeMode, AutoSave, u64)>(|settings| {
                (
                    settings.font_size(),
                    settings.tab_spaces(),
                    settings.theme(),
                    settings.auto_save(),
                    settings.auto_save_delay_ms(),
                )
            });
        let store = ProviderHandle::<Settings>::of(ctx);

        let font_store = store.clone();
        let font = Stepper::new(
            font_size as i64,
            *Settings::FONT_SIZES.start() as i64..=*Settings::FONT_SIZES.end() as i64,
            "px",
            Rc::new(move |size| {
                change(&font_store, |settings| settings.set_font_size(size as u32))
            }),
        );
        let tab_store = store.clone();
        let tab = Stepper::new(
            tab_spaces as i64,
            *Settings::TAB_SPACES.start() as i64..=*Settings::TAB_SPACES.end() as i64,
            "spaces",
            Rc::new(move |spaces| {
                change(&tab_store, |settings| {
                    settings.set_tab_spaces(spaces as usize)
                })
            }),
        );
        let save_store = store.clone();
        let save_way = Choice::new(
            AutoSave::ALL.iter().map(|way| way.label()).collect(),
            AutoSave::ALL.iter().position(|candidate| *candidate == auto_save).unwrap_or(0),
            Rc::new(move |index| change(&save_store, |settings| settings.set_auto_save(AutoSave::ALL[index]))),
        );
        let delay_store = store.clone();
        let save_delay = Stepper::new(
            auto_save_delay as i64,
            *Settings::AUTO_SAVE_DELAYS_MS.start() as i64..=*Settings::AUTO_SAVE_DELAYS_MS.end() as i64,
            "ms",
            Rc::new(move |delay| change(&delay_store, |settings| settings.set_auto_save_delay_ms(delay as u64))),
        )
        .step(100);
        let theme_store = store;
        let appearance = Choice::new(
            ThemeMode::ALL
                .iter()
                .map(|mode| mode.label())
                .collect(),
            ThemeMode::ALL
                .iter()
                .position(|candidate| *candidate == mode)
                .unwrap_or(0),
            Rc::new(move |index| {
                change(&theme_store, |settings| {
                    settings.set_theme(ThemeMode::ALL[index])
                })
            }),
        );

        let heading = |text: &'static str, size: u32| {
            Text::new(text).text_style(fonts::ui().font_size(size).color(theme.text))
        };
        let page = Column::new()
            .gaps(LayoutSpacing::all(SECTION_GAP))
            .children([
                heading("Settings", 22).boxed(),
                section_title("Editor", theme).boxed(),
                setting_row(
                    "Font size",
                    "The size of code, in logical pixels.",
                    font,
                    theme,
                )
                .boxed(),
                setting_row(
                    "Tab width",
                    "How many spaces the Tab key types.",
                    tab,
                    theme,
                )
                .boxed(),
                section_title("Saving", theme).boxed(),
                setting_row(
                    "Auto save",
                    "Saves a file that was changed, and checks the project when it does: off, a while after the last change, or when the editor loses the focus.",
                    save_way,
                    theme,
                )
                .boxed(),
                setting_row(
                    "Auto save delay",
                    "How long after the last change the file is saved, when auto save is after a delay.",
                    save_delay,
                    theme,
                )
                .boxed(),
                section_title("Appearance", theme).boxed(),
                setting_row(
                    "Theme",
                    "The colors of the editor and of the window around it.",
                    appearance,
                    theme,
                )
                .boxed(),
            ]);
        // A scrollable takes all the height it is given; the page is as tall as the content, and scrolls when the
        // window is not.
        SizedBox::new()
            .width(Dimension::Percent(100.0))
            .height(Dimension::Percent(100.0))
            .box_child(
                Container::new()
                    .color(theme.editor)
                    .padding(pad(28, 40, 28, 40))
                    .child(Scrollable::new().child(page)),
            )
    }
}
