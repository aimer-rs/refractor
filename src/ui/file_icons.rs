//! Material Icon Theme icons for files and folders, picked by name and extension.
//!
//! The icons are in `assets/icons` and are embedded when something asks for them: each is parsed on its first use and shared.

use aimer::StaticSvg;

/// An embedded icon of `assets/icons`, by file name without the extension.
macro_rules! icon {
    ($name:literal) => {{
        static ICON: StaticSvg = StaticSvg::new(
            $name,
            include_str!(concat!("../../assets/icons/", $name, ".svg")),
        );
        &ICON
    }};
}

/// The icon of a file called `name`: by its whole name when that is known (`Cargo.toml`), and by its
/// extension otherwise. A file of an unknown kind gets the plain document.
pub fn file_icon(name: &str) -> &'static StaticSvg {
    let lower = name.to_ascii_lowercase();
    let by_name: Option<&'static StaticSvg> = match lower.as_str() {
        ".editorconfig" => Some(icon!("editorconfig")),
        ".gitattributes" => Some(icon!("git")),
        ".gitignore" => Some(icon!("git")),
        "agents.md" => Some(icon!("agent")),
        "aimer.lock" => Some(icon!("lock")),
        "aimer.toml" => Some(icon!("toml")),
        "cargo.lock" => Some(icon!("lock")),
        "cargo.toml" => Some(icon!("rust")),
        "changelog.md" => Some(icon!("changelog")),
        "claude.md" => Some(icon!("claude")),
        "dockerfile" => Some(icon!("docker")),
        "gemini.md" => Some(icon!("gemini")),
        "license" => Some(icon!("license")),
        "license.md" => Some(icon!("license")),
        "makefile" => Some(icon!("makefile")),
        "package-lock.json" => Some(icon!("nodejs")),
        "package.json" => Some(icon!("nodejs")),
        "readme.md" => Some(icon!("readme")),
        "rustfmt.toml" => Some(icon!("toml")),
        _ => None,
    };
    if let Some(icon) = by_name {
        return icon;
    }
    let extension = lower
        .rsplit_once('.')
        .map_or("", |(_, extension)| extension);
    match extension {
        "c" => icon!("c"),
        "cc" => icon!("cpp"),
        "cmake" => icon!("cmake"),
        "cpp" => icon!("cpp"),
        "css" => icon!("css"),
        "db" => icon!("database"),
        "fish" => icon!("console"),
        "gif" => icon!("image"),
        "go" => icon!("go"),
        "gradle" => icon!("gradle"),
        "h" => icon!("h"),
        "hpp" => icon!("cpp"),
        "html" => icon!("html"),
        "java" => icon!("java"),
        "jpeg" => icon!("image"),
        "jpg" => icon!("image"),
        "js" => icon!("javascript"),
        "json" => icon!("json"),
        "jsx" => icon!("react"),
        "kt" => icon!("kotlin"),
        "lock" => icon!("lock"),
        "lockb" => icon!("lock"),
        "log" => icon!("log"),
        "lua" => icon!("lua"),
        "markdown" => icon!("markdown"),
        "md" => icon!("markdown"),
        "mjs" => icon!("javascript"),
        "mov" => icon!("video"),
        "mp3" => icon!("audio"),
        "mp4" => icon!("video"),
        "otf" => icon!("font"),
        "pdf" => icon!("pdf"),
        "php" => icon!("php"),
        "plist" => icon!("xml"),
        "png" => icon!("image"),
        "ps1" => icon!("powershell"),
        "py" => icon!("python"),
        "rb" => icon!("ruby"),
        "rs" => icon!("rust"),
        "sh" => icon!("console"),
        "sql" => icon!("database"),
        "svg" => icon!("svg"),
        "swift" => icon!("swift"),
        "toml" => icon!("toml"),
        "ts" => icon!("typescript"),
        "tsx" => icon!("react"),
        "ttf" => icon!("font"),
        "txt" => icon!("document"),
        "wav" => icon!("audio"),
        "webp" => icon!("image"),
        "woff" => icon!("font"),
        "woff2" => icon!("font"),
        "xml" => icon!("xml"),
        "yaml" => icon!("yaml"),
        "yml" => icon!("yaml"),
        "zip" => icon!("zip"),
        "zsh" => icon!("console"),
        _ => icon!("document"),
    }
}

/// The icon of a folder called `name`. Material Icon Theme has no open variants in `assets/icons`,
/// so an open folder is told from a closed one by the chevron in front of it.
pub fn folder_icon(name: &str) -> &'static StaticSvg {
    match name.to_ascii_lowercase().as_str() {
        ".claude" => icon!("folder-claude"),
        ".config" => icon!("folder-config"),
        ".git" => icon!("folder-git"),
        ".github" => icon!("folder-github"),
        ".vscode" => icon!("folder-vscode"),
        "agents" => icon!("folder-base"),
        "android" => icon!("folder-android"),
        "app" => icon!("folder-app"),
        "assets" => icon!("folder-resource"),
        "build" => icon!("folder-dist"),
        "builds" => icon!("folder-dist"),
        "ci" => icon!("folder-ci"),
        "components" => icon!("folder-components"),
        "config" => icon!("folder-config"),
        "core" => icon!("folder-core"),
        "database" => icon!("folder-database"),
        "dist" => icon!("folder-dist"),
        "doc" => icon!("folder-docs"),
        "docs" => icon!("folder-docs"),
        "hooks" => icon!("folder-hook"),
        "images" => icon!("folder-images"),
        "img" => icon!("folder-images"),
        "include" => icon!("folder-include"),
        "ios" => icon!("folder-ios"),
        "lib" => icon!("folder-lib"),
        "linux" => icon!("folder-linux"),
        "logs" => icon!("folder-log"),
        "macos" => icon!("folder-macos"),
        "mobile" => icon!("folder-mobile"),
        "node_modules" => icon!("folder-node"),
        "plugins" => icon!("folder-plugin"),
        "public" => icon!("folder-public"),
        "resources" => icon!("folder-resource"),
        "rust" => icon!("folder-rust"),
        "scripts" => icon!("folder-scripts"),
        "src" => icon!("folder-src"),
        "target" => icon!("folder-target"),
        "temp" => icon!("folder-temp"),
        "templates" => icon!("folder-template"),
        "test" => icon!("folder-test"),
        "tests" => icon!("folder-test"),
        "theme" => icon!("folder-theme"),
        "tmp" => icon!("folder-temp"),
        "tools" => icon!("folder-tools"),
        "utils" => icon!("folder-utils"),
        "windows" => icon!("folder-windows"),
        _ => icon!("folder-base"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_name_wins_over_its_extension() {
        assert_eq!(
            file_icon("Cargo.toml").source(),
            file_icon("rust.rs").source(),
            "Cargo.toml has the rust icon, not the toml one"
        );
        assert_ne!(
            file_icon("Cargo.toml").source(),
            file_icon("Other.toml").source()
        );
        assert_eq!(
            file_icon("README.md").source(),
            file_icon("readme.md").source(),
            "the case of a name does not matter"
        );
    }

    #[test]
    fn an_extension_picks_its_icon_and_an_unknown_one_the_document() {
        assert_eq!(file_icon("main.rs").source(), file_icon("lib.RS").source());
        assert_eq!(
            file_icon("notes.unknown").source(),
            file_icon("noextension").source()
        );
        assert_ne!(
            file_icon("main.rs").source(),
            file_icon("notes.unknown").source()
        );
    }

    #[test]
    fn a_folder_is_known_by_its_name_and_any_other_is_the_plain_folder() {
        assert_ne!(
            folder_icon("src").source(),
            folder_icon("something").source()
        );
        assert_eq!(folder_icon("zzz").source(), folder_icon("yyy").source());
    }

    #[test]
    fn every_embedded_icon_parses() {
        for name in [
            "Cargo.toml",
            "a.rs",
            "a.md",
            "a.json",
            "a.png",
            "a.sh",
            "LICENSE",
        ] {
            let _ = file_icon(name).document();
        }
        for name in ["src", "target", ".vscode", "x"] {
            let _ = folder_icon(name).document();
        }
    }
}
