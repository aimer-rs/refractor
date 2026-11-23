mod choice;
mod setting_row;
mod sidebar;
mod stepper;

use self::choice::Choice;
use self::setting_row::{section_heading, setting_row};
use self::sidebar::SettingsSidebar;
use self::stepper::Stepper;
use crate::ui::components::pad::pad;
use crate::ui::components::rule;
use crate::services::settings::{AutoSave, Formatter, Settings, ThemeMode};
use crate::ui::theme::CodeTheme;
use aimer::style::Theme;
use aimer::{
    AnyWidget, BuildContext, Column, Container, Dimension, Expanded, Icons, ProviderContext,
    ProviderHandle, Row, Scrollable, SizedBox, State, StateView, StatefulWidget, StaticSvg, Widget,
};
use std::rc::Rc;

/// A category of settings shown in the left navigation rail.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(self) enum SettingsSection {
    #[default]
    Editor,
    Saving,
    Appearance,
}

impl SettingsSection {
    /// The sections, in the order the navigation rail lists them.
    pub const ALL: [SettingsSection; 3] = [
        SettingsSection::Editor,
        SettingsSection::Saving,
        SettingsSection::Appearance,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SettingsSection::Editor => "Editor",
            SettingsSection::Saving => "Saving",
            SettingsSection::Appearance => "Appearance",
        }
    }

    pub fn icon(self) -> &'static StaticSvg {
        match self {
            SettingsSection::Editor => Icons::CODE,
            SettingsSection::Saving => Icons::SAVE,
            SettingsSection::Appearance => Icons::PALETTE,
        }
    }
}

/// What the settings tab shows: a category navigation rail on the left and the selected category's
/// settings on the right. A change is made at once, is drawn by everything that shows it, and is
/// written to the settings file.
#[derive(Clone, StatefulWidget)]
pub struct SettingsPage;

struct SettingsPageState {
    section: SettingsSection,
}

/// Makes `edit` to the settings and keeps them: the editor follows the change, and so does the file.
fn change(store: &ProviderHandle<Settings>, edit: impl FnOnce(&mut Settings)) {
    store.update(edit);
    store.read().save();
}

impl State<SettingsPage> for SettingsPageState {
    fn create(_widget: SettingsPage) -> Self {
        Self {
            section: SettingsSection::default(),
        }
    }

    fn build(this: StateView<Self>, ctx: &BuildContext) -> impl Widget {
        let theme = *CodeTheme::of(ctx);
        let section = this.section;
        let updater = this.updater();
        let on_select: Rc<dyn Fn(SettingsSection)> = Rc::new(move |next| {
            updater.set_state(move |state| state.section = next);
        });

        let content = section_content(section, ctx, theme);

        SizedBox::new()
            .width(Dimension::Percent(100.0))
            .height(Dimension::Percent(100.0))
            .box_child(
                Row::new().children([
                    SettingsSidebar::new(section, on_select).boxed(),
                    rule::vertical(theme.divider).boxed(),
                    Expanded::new()
                        .child(
                            Container::new()
                                .color(theme.editor)
                                .padding(pad(20, 28, 20, 28))
                                .child(Scrollable::new().child(content)),
                        )
                        .boxed(),
                ]),
            )
    }
}

fn section_content(section: SettingsSection, ctx: &BuildContext, theme: CodeTheme) -> Column<AnyWidget> {
    let store = ProviderHandle::<Settings>::of(ctx);
    let mut rows: Vec<AnyWidget> = vec![section_heading(section.label(), theme).boxed()];
    rows.extend(match section {
        SettingsSection::Editor => editor_rows(ctx, store, theme),
        SettingsSection::Saving => saving_rows(ctx, store, theme),
        SettingsSection::Appearance => appearance_rows(ctx, store, theme),
    });
    Column::new().children(rows)
}

fn editor_rows(
    ctx: &BuildContext,
    store: ProviderHandle<Settings>,
    theme: CodeTheme,
) -> Vec<AnyWidget> {
    let (font_size, tab_spaces, formatter) =
        ctx.select::<Settings, (u32, usize, Formatter)>(|settings| {
            (
                settings.font_size(),
                settings.tab_spaces(),
                settings.formatter(),
            )
        });

    let font_store = store.clone();
    let font = Stepper::new(
        font_size as i64,
        *Settings::FONT_SIZES.start() as i64..=*Settings::FONT_SIZES.end() as i64,
        "px",
        Rc::new(move |size| change(&font_store, |settings| settings.set_font_size(size as u32))),
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

    let formatter_way = Choice::new(
        Formatter::ALL.iter().map(|way| way.label()).collect(),
        Formatter::ALL
            .iter()
            .position(|candidate| *candidate == formatter)
            .unwrap_or(0),
        Rc::new(move |index| {
            change(&store, |settings| {
                settings.set_formatter(Formatter::ALL[index])
            })
        }),
    );

    vec![
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
        setting_row(
            "Formatter",
            "How the active file is formatted: try the language server first and fall back, or use one.",
            formatter_way,
            theme,
        )
        .boxed(),
    ]
}

fn saving_rows(
    ctx: &BuildContext,
    store: ProviderHandle<Settings>,
    theme: CodeTheme,
) -> Vec<AnyWidget> {
    let (auto_save, auto_save_delay) =
        ctx.select::<Settings, (AutoSave, u64)>(|settings| {
            (settings.auto_save(), settings.auto_save_delay_ms())
        });

    let save_store = store.clone();
    let save_way = Choice::new(
        AutoSave::ALL.iter().map(|way| way.label()).collect(),
        AutoSave::ALL
            .iter()
            .position(|candidate| *candidate == auto_save)
            .unwrap_or(0),
        Rc::new(move |index| {
            change(&save_store, |settings| {
                settings.set_auto_save(AutoSave::ALL[index])
            })
        }),
    );

    let save_delay = Stepper::new(
        auto_save_delay as i64,
        *Settings::AUTO_SAVE_DELAYS_MS.start() as i64
            ..=*Settings::AUTO_SAVE_DELAYS_MS.end() as i64,
        "ms",
        Rc::new(move |delay| {
            change(&store, |settings| {
                settings.set_auto_save_delay_ms(delay as u64)
            })
        }),
    )
    .step(100);

    vec![
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
    ]
}

fn appearance_rows(
    ctx: &BuildContext,
    store: ProviderHandle<Settings>,
    theme: CodeTheme,
) -> Vec<AnyWidget> {
    let mode = ctx.select::<Settings, ThemeMode>(|settings| settings.theme());
    let icons = ThemeMode::ALL
        .iter()
        .map(|candidate| match candidate {
            ThemeMode::Dark => Icons::MOON,
            ThemeMode::Light => Icons::SUN,
        })
        .collect();
    let appearance = Choice::icons(
        icons,
        ThemeMode::ALL
            .iter()
            .position(|candidate| *candidate == mode)
            .unwrap_or(0),
        Rc::new(move |index| {
            change(&store, |settings| {
                settings.set_theme(ThemeMode::ALL[index])
            })
        }),
    );

    vec![
        setting_row(
            "Theme",
            "The colors of the editor and of the window around it.",
            appearance,
            theme,
        )
        .boxed(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_sections_cover_editor_saving_and_appearance_in_order() {
        assert_eq!(SettingsSection::default(), SettingsSection::Editor);
        assert_eq!(
            SettingsSection::ALL,
            [
                SettingsSection::Editor,
                SettingsSection::Saving,
                SettingsSection::Appearance,
            ]
        );
        assert_eq!(
            SettingsSection::ALL.map(SettingsSection::label),
            ["Editor", "Saving", "Appearance"]
        );
    }
}
