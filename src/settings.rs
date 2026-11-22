//! What the user can change in the editor, and the file it is kept in.
//!
//! The file is TOML, with one key for each setting. A key that is missing has its default, so a file
//! written by an older version, or by hand with one line in it, is read; a value out of range is
//! brought into it. A file that is not valid TOML, or names a theme that does not exist, is not read at
//! all: the defaults are used and the file is set aside under another name, so that the next save does
//! not destroy what the user wrote.

use serde::{Deserialize, Serialize};
use std::io;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

/// Which colors the editor is drawn in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
}

impl ThemeMode {
    /// The modes, in the order the settings screen offers them.
    pub const ALL: [ThemeMode; 2] = [ThemeMode::Dark, ThemeMode::Light];

    /// What the settings screen calls the mode.
    pub fn label(self) -> &'static str {
        match self {
            ThemeMode::Dark => "Dark",
            ThemeMode::Light => "Light",
        }
    }
}

/// When a file that has been changed is saved without being asked to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AutoSave {
    /// Never: a file is saved when the user says so.
    #[default]
    Off,
    /// A while after the last change.
    Delay,
    /// When the editor loses the focus: the window goes to the background, or the user goes from the text to something else.
    FocusLost,
}

impl AutoSave {
    /// The ways, in the order the settings screen offers them.
    pub const ALL: [AutoSave; 3] = [AutoSave::Off, AutoSave::Delay, AutoSave::FocusLost];

    /// What the settings screen calls the way.
    pub fn label(self) -> &'static str {
        match self {
            AutoSave::Off => "Off",
            AutoSave::Delay => "After a delay",
            AutoSave::FocusLost => "When focus is lost",
        }
    }
}

/// The settings of the editor. It is small and cloned freely; a setting is changed with its setter,
/// which keeps it in the range the editor can draw.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    font_size: u32,
    tab_spaces: usize,
    theme: ThemeMode,
    auto_save: AutoSave,
    auto_save_delay_ms: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            font_size: Self::DEFAULT_FONT_SIZE,
            tab_spaces: Self::DEFAULT_TAB_SPACES,
            theme: ThemeMode::default(),
            auto_save: AutoSave::default(),
            auto_save_delay_ms: Self::DEFAULT_AUTO_SAVE_DELAY_MS,
        }
    }
}

impl Settings {
    /// The size of code, in logical pixels, that the editor starts with.
    pub const DEFAULT_FONT_SIZE: u32 = 12;
    /// The sizes of code the editor offers: smaller is not legible, and larger makes a line a screenful.
    pub const FONT_SIZES: RangeInclusive<u32> = 8..=32;
    /// How many spaces a tab key makes, at the start.
    pub const DEFAULT_TAB_SPACES: usize = 4;
    /// The widths of a tab the editor offers.
    pub const TAB_SPACES: RangeInclusive<usize> = 1..=8;
    /// How long after the last change a file is saved, in milliseconds, when it is saved after a delay and the user has not said.
    pub const DEFAULT_AUTO_SAVE_DELAY_MS: u64 = 1000;
    /// The delays the editor offers: shorter is a save on every few keys, and longer is the work of a minute that may be lost.
    pub const AUTO_SAVE_DELAYS_MS: RangeInclusive<u64> = 100..=30_000;
    /// The file the settings are read from and written to, when the user has not named another one.
    pub const FILE_NAME: &'static str = "settings.toml";
    /// The variable that names another file for the settings: for trying the editor without touching
    /// the settings of the one that is used.
    pub const FILE_VARIABLE: &'static str = "REFRACTOR_SETTINGS";

    /// The size of code, in logical pixels.
    pub fn font_size(&self) -> u32 {
        self.font_size
    }

    /// How many spaces a tab key makes.
    pub fn tab_spaces(&self) -> usize {
        self.tab_spaces
    }

    pub fn theme(&self) -> ThemeMode {
        self.theme
    }

    /// When a changed file is saved without being asked to.
    pub fn auto_save(&self) -> AutoSave {
        self.auto_save
    }

    /// How long after the last change a file is saved, when it is saved after a delay, in milliseconds.
    pub fn auto_save_delay_ms(&self) -> u64 {
        self.auto_save_delay_ms
    }

    /// The same as a duration.
    pub fn auto_save_delay(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.auto_save_delay_ms)
    }

    pub fn set_auto_save(&mut self, auto_save: AutoSave) {
        self.auto_save = auto_save;
    }

    /// Sets the delay of the save, brought into [`Self::AUTO_SAVE_DELAYS_MS`].
    pub fn set_auto_save_delay_ms(&mut self, delay: u64) {
        self.auto_save_delay_ms = delay.clamp(
            *Self::AUTO_SAVE_DELAYS_MS.start(),
            *Self::AUTO_SAVE_DELAYS_MS.end(),
        );
    }

    /// Sets the size of code, brought into [`Self::FONT_SIZES`].
    pub fn set_font_size(&mut self, size: u32) {
        self.font_size = size.clamp(*Self::FONT_SIZES.start(), *Self::FONT_SIZES.end());
    }

    /// Sets how many spaces a tab key makes, brought into [`Self::TAB_SPACES`].
    pub fn set_tab_spaces(&mut self, spaces: usize) {
        self.tab_spaces = spaces.clamp(*Self::TAB_SPACES.start(), *Self::TAB_SPACES.end());
    }

    pub fn set_theme(&mut self, theme: ThemeMode) {
        self.theme = theme;
    }

    /// Reads settings from the text of a file. Keys that are missing have their defaults and values out
    /// of range are brought into it; an error says why the text is not settings.
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        let mut settings: Self = toml::from_str(text)?;
        settings.set_font_size(settings.font_size);
        settings.set_tab_spaces(settings.tab_spaces);
        settings.set_auto_save_delay_ms(settings.auto_save_delay_ms);
        Ok(settings)
    }

    /// The text of the file that holds the settings.
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("a struct of numbers and a unit enum is TOML")
    }

    /// Reads the settings from `path`. A file that is not there gives the defaults, and so does one that
    /// is not settings, which is first renamed to the same name with `.invalid` added (and the reason is
    /// reported), so that saving does not replace it.
    pub fn load_from(path: &Path) -> Self {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                if error.kind() != io::ErrorKind::NotFound {
                    aimer::console::warn!(
                        "Could not read the settings {}: {error}",
                        path.display()
                    );
                }
                return Self::default();
            }
        };
        match Self::from_toml(&text) {
            Ok(settings) => settings,
            Err(error) => {
                let mut aside = path.as_os_str().to_owned();
                aside.push(".invalid");
                aimer::console::warn!(
                    "The settings {} are not valid and are set aside as {}: {error}",
                    path.display(),
                    PathBuf::from(&aside).display()
                );
                if let Err(error) = std::fs::rename(path, &aside) {
                    aimer::console::warn!("Could not set the settings aside: {error}");
                }
                Self::default()
            }
        }
    }

    /// Writes the settings to `path`, making the folder if it is missing. The file is replaced whole or
    /// not at all: a crash in the middle leaves the old settings, not half of the new.
    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(folder) = path
            .parent()
            .filter(|folder| !folder.as_os_str().is_empty())
        {
            std::fs::create_dir_all(folder)?;
        }
        let mut temporary = path.as_os_str().to_owned();
        temporary.push(".tmp");
        let temporary = PathBuf::from(temporary);
        let written = std::fs::write(&temporary, self.to_toml())
            .and_then(|()| std::fs::rename(&temporary, path));
        if written.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        written
    }

    /// The file the settings of this user are in: the one the [`FILE_VARIABLE`](Self::FILE_VARIABLE)
    /// names, or the one in the folder the platform keeps the settings of an app in. `None` when neither
    /// can be worked out, and then the settings are only kept while the editor runs.
    pub fn file() -> Option<PathBuf> {
        config_file(
            std::env::var_os(Self::FILE_VARIABLE).map(PathBuf::from),
            std::env::var_os("HOME").map(PathBuf::from),
            std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        )
    }

    /// The settings of this user: those in [`Self::file`], or the defaults.
    pub fn load() -> Self {
        Self::file().map_or_else(Self::default, |path| Self::load_from(&path))
    }

    /// Writes the settings to [`Self::file`], and says so if it cannot: a setting that is not kept is
    /// not an error the user can do anything about at the moment of the click.
    pub fn save(&self) {
        let Some(path) = Self::file() else { return };
        if let Err(error) = self.save_to(&path) {
            aimer::console::warn!("Could not save the settings {}: {error}", path.display());
        }
    }
}

/// Works out the file of the settings from where it can be told: `named` wins, then the folder macOS keeps
/// the settings of apps in, then `XDG_CONFIG_HOME`, then `~/.config`. Separate from [`Settings::file`] so
/// that it does not read the environment.
fn config_file(
    named: Option<PathBuf>,
    home: Option<PathBuf>,
    xdg: Option<PathBuf>,
) -> Option<PathBuf> {
    if let Some(named) = named.filter(|path| !path.as_os_str().is_empty()) {
        return Some(named);
    }
    let folder = if cfg!(target_os = "macos") {
        home?
            .join("Library")
            .join("Application Support")
            .join("Refractor")
    } else {
        xdg.filter(|path| path.is_absolute())
            .or_else(|| home.map(|home| home.join(".config")))?
            .join("refractor")
    };
    Some(folder.join(Settings::FILE_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of its own for one test, which is removed with it.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("refractor-settings-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("the temporary folder can be made");
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_defaults_are_what_the_editor_had_before_there_were_settings() {
        let settings = Settings::default();
        assert_eq!(settings.font_size(), 12);
        assert_eq!(settings.tab_spaces(), 4);
        assert_eq!(settings.theme(), ThemeMode::Dark);
    }

    #[test]
    fn a_file_is_saved_only_when_the_user_says_so_until_the_user_says_otherwise() {
        let settings = Settings::default();
        assert_eq!(settings.auto_save(), AutoSave::Off);
        assert_eq!(
            settings.auto_save_delay(),
            std::time::Duration::from_millis(1000)
        );
    }

    #[test]
    fn the_ways_to_save_by_itself_are_off_after_a_delay_and_when_the_focus_is_lost_and_are_written_as_words()
     {
        assert_eq!(
            AutoSave::ALL,
            [AutoSave::Off, AutoSave::Delay, AutoSave::FocusLost]
        );
        assert_eq!(
            AutoSave::ALL.map(AutoSave::label),
            ["Off", "After a delay", "When focus is lost"]
        );
        for (mode, word) in [
            (AutoSave::Off, "off"),
            (AutoSave::Delay, "delay"),
            (AutoSave::FocusLost, "focus-lost"),
        ] {
            let mut settings = Settings::default();
            settings.set_auto_save(mode);
            let text = settings.to_toml();
            assert!(text.contains(&format!("auto_save = \"{word}\"")), "{text}");
            assert_eq!(Settings::from_toml(&text).unwrap().auto_save(), mode);
        }
        assert!(
            Settings::from_toml("auto_save = \"always\"").is_err(),
            "a way that there is not"
        );
    }

    #[test]
    fn the_delay_of_the_save_is_brought_into_the_range_the_editor_offers_and_is_kept_in_milliseconds()
     {
        let mut settings = Settings::default();
        settings.set_auto_save_delay_ms(0);
        assert_eq!(
            settings.auto_save_delay_ms(),
            *Settings::AUTO_SAVE_DELAYS_MS.start()
        );
        settings.set_auto_save_delay_ms(u64::MAX);
        assert_eq!(
            settings.auto_save_delay_ms(),
            *Settings::AUTO_SAVE_DELAYS_MS.end()
        );
        settings.set_auto_save_delay_ms(2500);
        assert_eq!(
            settings.auto_save_delay(),
            std::time::Duration::from_millis(2500)
        );
        assert!(
            settings
                .to_toml()
                .contains("auto_save_delay_ms = 2500")
        );
        let read = Settings::from_toml("auto_save_delay_ms = 3\n").unwrap();
        assert_eq!(
            read.auto_save_delay_ms(),
            *Settings::AUTO_SAVE_DELAYS_MS.start(),
            "a file that says less is brought into the range"
        );
        assert!(Settings::from_toml("auto_save_delay_ms = -5").is_err());
        assert!(Settings::from_toml("auto_save_delay_ms = \"soon\"").is_err());
    }

    #[test]
    fn a_file_from_before_there_was_a_way_to_save_by_itself_still_reads() {
        let settings = Settings::from_toml("font_size = 14\ntab_spaces = 2\ntheme = \"light\"\n")
            .expect("the old keys");
        assert_eq!(
            (settings.auto_save(), settings.auto_save_delay_ms()),
            (AutoSave::Off, Settings::DEFAULT_AUTO_SAVE_DELAY_MS)
        );
    }

    #[test]
    fn setters_bring_a_value_into_its_range() {
        let mut settings = Settings::default();
        settings.set_font_size(0);
        assert_eq!(settings.font_size(), 8);
        settings.set_font_size(u32::MAX);
        assert_eq!(settings.font_size(), 32);
        settings.set_tab_spaces(0);
        assert_eq!(settings.tab_spaces(), 1);
        settings.set_tab_spaces(usize::MAX);
        assert_eq!(settings.tab_spaces(), 8);
        settings.set_tab_spaces(2);
        assert_eq!(settings.tab_spaces(), 2);
    }

    #[test]
    fn settings_survive_a_round_trip_through_toml() {
        let mut settings = Settings::default();
        settings.set_font_size(15);
        settings.set_tab_spaces(2);
        settings.set_theme(ThemeMode::Light);
        let text = settings.to_toml();
        assert!(
            text.contains("theme = \"light\""),
            "the theme is written as a word: {text}"
        );
        assert_eq!(
            Settings::from_toml(&text).expect("what was written is read"),
            settings
        );
    }

    #[test]
    fn a_file_with_some_keys_has_the_defaults_for_the_rest() {
        let settings = Settings::from_toml("font_size = 20\n").expect("one key is a file");
        assert_eq!(settings.font_size(), 20);
        assert_eq!(settings.tab_spaces(), 4);
        assert_eq!(
            Settings::from_toml("").expect("an empty file is all defaults"),
            Settings::default()
        );
    }

    #[test]
    fn values_out_of_range_are_brought_into_it() {
        let settings =
            Settings::from_toml("font_size = 400\ntab_spaces = 0\n").expect("numbers are read");
        assert_eq!(settings.font_size(), 32);
        assert_eq!(settings.tab_spaces(), 1);
    }

    #[test]
    fn a_value_of_the_wrong_kind_is_an_error() {
        assert!(Settings::from_toml("font_size = \"big\"").is_err());
        assert!(Settings::from_toml("font_size = -3").is_err());
        assert!(Settings::from_toml("theme = \"solarized\"").is_err());
        assert!(Settings::from_toml("this is not toml").is_err());
    }

    #[test]
    fn a_file_that_is_missing_gives_the_defaults() {
        let scratch = Scratch::new("missing");
        assert_eq!(
            Settings::load_from(&scratch.0.join("none.toml")),
            Settings::default()
        );
    }

    #[test]
    fn saving_makes_the_folder_and_replaces_the_file_whole() {
        let scratch = Scratch::new("save");
        let path = scratch.0.join("inside").join("settings.toml");
        let mut settings = Settings::default();
        settings.set_tab_spaces(8);
        settings
            .save_to(&path)
            .expect("the folder is made and the file written");
        assert_eq!(Settings::load_from(&path), settings);
        settings.set_theme(ThemeMode::Light);
        settings
            .save_to(&path)
            .expect("a file is replaced");
        assert_eq!(Settings::load_from(&path), settings);
        let left: Vec<_> = std::fs::read_dir(path.parent().expect("it has a folder"))
            .expect("the folder is read")
            .map(|entry| entry.expect("an entry").file_name())
            .collect();
        assert_eq!(left, ["settings.toml"], "no temporary file is left behind");
    }

    #[test]
    fn a_file_that_is_not_settings_is_set_aside_and_not_lost() {
        let scratch = Scratch::new("invalid");
        let path = scratch.0.join("settings.toml");
        std::fs::write(&path, "theme = \"solarized\"\n").expect("the file is written");
        assert_eq!(Settings::load_from(&path), Settings::default());
        assert!(
            !path.exists(),
            "the file that was not read is not left where it would be replaced"
        );
        let aside = scratch.0.join("settings.toml.invalid");
        assert_eq!(
            std::fs::read_to_string(aside).expect("it is kept"),
            "theme = \"solarized\"\n"
        );
    }

    #[test]
    fn the_file_is_found_where_the_platform_keeps_the_settings_of_an_app() {
        let home = Some(PathBuf::from("/home/user"));
        assert_eq!(
            config_file(Some(PathBuf::from("/tmp/mine.toml")), home.clone(), None),
            Some(PathBuf::from("/tmp/mine.toml")),
            "a file that is named wins"
        );
        let found = config_file(Some(PathBuf::new()), home.clone(), None)
            .expect("the home folder is enough");
        if cfg!(target_os = "macos") {
            assert_eq!(
                found,
                PathBuf::from("/home/user/Library/Application Support/Refractor/settings.toml")
            );
        } else {
            assert_eq!(
                found,
                PathBuf::from("/home/user/.config/refractor/settings.toml")
            );
            assert_eq!(
                config_file(None, home, Some(PathBuf::from("/xdg"))),
                Some(PathBuf::from("/xdg/refractor/settings.toml"))
            );
        }
        assert_eq!(
            config_file(None, None, None),
            None,
            "with nothing to go on there is no file"
        );
    }
}
