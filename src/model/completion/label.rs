//! How the text that the server says beside a name is shown.

use crate::services::lsp::ItemKind;

/// The rules of the text beside a name.
pub struct Label;

impl Label {
    /// The name of an item as it is shown. The server writes a module as `unix::`, which is what is put in when it is chosen, so that
    /// the path goes on; shown, it is `unix`.
    pub fn name(label: &str, kind: ItemKind) -> &str {
        match kind {
            ItemKind::Module => label.strip_suffix("::").unwrap_or(label),
            _ => label,
        }
    }

    /// The path an item is imported from, which the server says as `(use std::fmt::Debug)`, as it is shown beside the name: the path
    /// alone, `std::fmt::Debug`. Anything else the server says beside a name, such as `of ToString`, is left as it is, with the
    /// blanks around it cut.
    pub fn detail(text: &str) -> String {
        let text = text.trim();
        let inner = text
            .strip_prefix('(')
            .and_then(|inner| inner.strip_suffix(')'))
            .map_or(text, str::trim);
        match inner.strip_prefix("use ") {
            Some(path) => path.trim().to_owned(),
            None => text.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_module_is_shown_without_the_colons_the_server_puts_after_it_and_nothing_else_is_changed() {
        assert_eq!(Label::name("unix::", ItemKind::Module), "unix");
        assert_eq!(
            Label::name("unix", ItemKind::Module),
            "unix",
            "a module the server wrote without them"
        );
        assert_eq!(
            Label::name("a::b::", ItemKind::Module),
            "a::b",
            "only the last colons"
        );
        assert_eq!(
            Label::name("::", ItemKind::Module),
            "",
            "nothing else to show"
        );
        assert_eq!(
            Label::name("Vec::", ItemKind::Struct),
            "Vec::",
            "what is not a module is as it was said"
        );
        assert_eq!(
            Label::name("unimplemented!", ItemKind::Function),
            "unimplemented!"
        );
    }

    #[test]
    fn an_import_is_the_path_without_its_brackets_and_its_use() {
        assert_eq!(Label::detail("(use std::fmt::Debug)"), "std::fmt::Debug");
        assert_eq!(
            Label::detail(" (use crate::util::UriExt) "),
            "crate::util::UriExt",
            "with blanks round it"
        );
        assert_eq!(
            Label::detail("( use a::B )"),
            "a::B",
            "and inside the brackets"
        );
        assert_eq!(
            Label::detail("use a::B"),
            "a::B",
            "a server that says it without the brackets"
        );
        assert_eq!(
            Label::detail("(use a::{B, C})"),
            "a::{B, C}",
            "a path with brackets of its own keeps them"
        );
    }

    #[test]
    fn what_is_not_an_import_is_left_as_it_is() {
        assert_eq!(Label::detail("of ToString"), "of ToString");
        assert_eq!(
            Label::detail("(self, K, V)"),
            "(self, K, V)",
            "brackets that are not an import are kept"
        );
        assert_eq!(
            Label::detail("(user)"),
            "(user)",
            "a word that only begins like use"
        );
        assert_eq!(
            Label::detail("  of Iterator "),
            "of Iterator",
            "with the blanks cut"
        );
        assert_eq!(Label::detail(""), "");
        assert_eq!(Label::detail("use"), "use", "nothing after it");
    }
}
