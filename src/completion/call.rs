//! The parentheses of a call, which completing a function puts after its name: `push` becomes `push()`, with the caret
//! inside where there is something to put there and after it where there is not.

use crate::lsp::{Item, ItemKind};

/// A function's signature, split: see [`Call::signature`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Signature<'a> {
    /// The parameters, with the brackets round them.
    pub params: &'a str,
    /// What it returns, if it says.
    pub returns: Option<&'a str>,
}

/// The rules of completing a call.
pub struct Call;

impl Call {
    /// How many bytes of the line before the word are looked at to tell that it is an import.
    pub const IMPORT_LOOKBEHIND: usize = 64;

    /// Whether completing `item` adds `()`, and if it does, whether the caret goes between them: `None` where it is not a
    /// call to make. `next` is the byte after the word, and `before_word` the start of the line before it.
    ///
    /// A function that is named in an import, or that is already followed by its arguments, or whose text has them, is not
    /// given any. A macro is called with the caret in its brackets. For any other function the caret goes inside when the
    /// server says it takes arguments other than `self`, and after the brackets when it takes none, so that what is typed
    /// next is what follows the call.
    pub fn parentheses(item: &Item, next: Option<u8>, before_word: &str) -> Option<bool> {
        if !matches!(item.kind, ItemKind::Function | ItemKind::Method)
            || item.edit.text.contains('(')
            || next == Some(b'(')
        {
            return None;
        }
        if Self::is_import(before_word) {
            return None;
        }
        if item.label.ends_with('!') {
            return Some(true);
        }
        Some(
            item.detail
                .as_deref()
                .is_none_or(Self::takes_arguments),
        )
    }

    /// The signature that `detail` is, as the server writes one, `fn(a: u8) -> String`, split into the parameters in their
    /// brackets and what it returns. `None` for what is not the signature of a function: a type, a keyword, a macro.
    pub fn signature(detail: &str) -> Option<Signature<'_>> {
        let open = detail.find('(')?;
        // What is before the brackets is the word `fn`, and what qualifies it: `unsafe`, `async`, `const`.
        let qualifiers = detail[..open].split_whitespace();
        if qualifiers.clone().next_back() != Some("fn") {
            return None;
        }
        let mut depth = 0usize;
        let close = detail[open..].char_indices().find_map(|(at, ch)| {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(open + at);
                    }
                }
                _ => {}
            }
            None
        })?;
        let rest = detail[close + 1..].trim_start();
        let returns = rest
            .strip_prefix("->")
            .map(str::trim)
            .filter(|returns| !returns.is_empty());
        Some(Signature { params: &detail[open..=close], returns })
    }

    /// Whether a line that begins with `before_word` is an import: a name in it is not called.
    fn is_import(before_word: &str) -> bool {
        let line = before_word.trim_start();
        let line = line.strip_prefix("pub").map_or(line, |rest| {
            match rest.trim_start().strip_prefix('(') {
                // `pub(crate)` and `pub(super)`.
                Some(restriction) => restriction
                    .split_once(')')
                    .map_or("", |(_, after)| after),
                None => rest,
            }
        });
        line.trim_start().starts_with("use ")
    }

    /// Whether the signature `detail`, such as `fn(&self, key: K) -> bool`, has an argument other than `self`.
    fn takes_arguments(detail: &str) -> bool {
        let Some(open) = detail.find('(') else {
            // Not a signature: a call is made with the caret where an argument may go.
            return true;
        };
        let mut depth = 0usize;
        let mut arguments = Vec::new();
        let mut start = open + 1;
        for (at, ch) in detail[open + 1..]
            .char_indices()
            .map(|(at, ch)| (at + open + 1, ch))
        {
            match ch {
                '(' | '[' | '<' => depth += 1,
                // The arrow of a return type is not a bracket.
                '>' if detail[..at].ends_with('-') => {}
                ')' | ']' | '>' if depth > 0 => depth -= 1,
                ')' => {
                    arguments.push(&detail[start..at]);
                    break;
                }
                ',' if depth == 0 => {
                    arguments.push(&detail[start..at]);
                    start = at + 1;
                }
                _ => {}
            }
        }
        arguments
            .iter()
            .map(|argument| argument.trim())
            .any(|argument| !argument.is_empty() && !Self::is_receiver(argument))
    }

    /// Whether `argument` is `self`, in any of its forms: `self`, `&self`, `&mut self`, `mut self`, `&'a self`, `self: Box<Self>`.
    fn is_receiver(argument: &str) -> bool {
        let head = argument.split(':').next().unwrap_or(argument);
        head.split(|ch: char| ch.is_whitespace() || ch == '&')
            .next_back()
            == Some("self")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::Edit;

    fn function(label: &str, detail: Option<&str>) -> Item {
        Item {
            label: label.to_owned(),
            kind: ItemKind::Function,
            detail: detail.map(str::to_owned),
            label_detail: None,
            sort: label.to_owned(),
            filter: label.to_owned(),
            edit: Edit {
                range: None,
                text: label.to_owned(),
                extra: vec![],
            },
            resolve: None,
            deprecated: false,
        }
    }

    #[test]
    fn a_function_that_takes_arguments_is_called_with_the_caret_inside_and_one_that_takes_none_with_it_after()
     {
        let call = |detail: Option<&str>| Call::parentheses(&function("f", detail), None, "");
        assert_eq!(call(Some("fn(x: u32)")), Some(true));
        assert_eq!(call(Some("fn(&mut self, x: i32) -> bool")), Some(true));
        assert_eq!(
            call(Some("fn(x: Vec<(u8, u8)>)")),
            Some(true),
            "a comma in brackets does not part arguments"
        );
        assert_eq!(call(Some("fn()")), Some(false));
        assert_eq!(
            call(Some("fn() -> Option<(u8, u8)>")),
            Some(false),
            "what is returned is not what is taken"
        );
        assert_eq!(
            call(Some("fn(&self)")),
            Some(false),
            "the receiver is not typed"
        );
        assert_eq!(call(Some("fn(&'a mut self) -> &'a T")), Some(false));
        assert_eq!(call(Some("fn(self: Box<Self>)")), Some(false));
        assert_eq!(call(Some("fn(self) -> Self")), Some(false));
        assert_eq!(
            call(Some("fn(&self, f: impl Fn(u8) -> u8)")),
            Some(true),
            "an arrow in an argument is not a bracket"
        );
        assert_eq!(
            call(None),
            Some(true),
            "the signature is not known: the caret is where an argument goes"
        );
        assert_eq!(call(Some("not a signature")), Some(true));
    }

    #[test]
    fn a_signature_is_split_into_its_parameters_and_what_it_returns() {
        fn split(detail: &str) -> Option<(&str, Option<&str>)> {
            Call::signature(detail).map(|signature| (signature.params, signature.returns))
        }
        assert_eq!(
            split("fn(a: u8, b: Vec<(u8, u8)>) -> Option<(u8, u8)>"),
            Some(("(a: u8, b: Vec<(u8, u8)>)", Some("Option<(u8, u8)>")))
        );
        assert_eq!(split("fn()"), Some(("()", None)), "returns nothing");
        assert_eq!(split("fn(&self) -> String"), Some(("(&self)", Some("String"))));
        assert_eq!(
            split("fn(self, f: impl Fn(u8) -> u8) -> bool"),
            Some(("(self, f: impl Fn(u8) -> u8)", Some("bool"))),
            "the arrow of an argument is not the one of the signature"
        );
        assert_eq!(
            split("fn(a: u8) -> impl Fn() -> u8"),
            Some(("(a: u8)", Some("impl Fn() -> u8"))),
            "the first arrow after the arguments is the signature's, and what follows it is all it returns"
        );
        assert_eq!(split("unsafe fn(buf: *mut u8) -> String"), Some(("(buf: *mut u8)", Some("String"))), "a qualified function");
        assert_eq!(split("fn(x: u8) where T: Copy"), Some(("(x: u8)", None)), "a clause that is not a return");
    }

    #[test]
    fn what_is_not_the_signature_of_a_function_has_none() {
        for detail in ["", "keyword", "Vec<T>", "macro_rules! println", "struct Foo(u8)", "fn(unclosed", "(a, b)", "const X: (u8, u8)"] {
            assert!(Call::signature(detail).is_none(), "{detail:?}");
        }
    }

    #[test]
    fn a_macro_is_called_with_the_caret_in_its_brackets() {
        assert_eq!(
            Call::parentheses(
                &function("println!", Some("macro_rules! println")),
                None,
                ""
            ),
            Some(true)
        );
    }

    #[test]
    fn only_functions_and_methods_are_called() {
        for kind in [
            ItemKind::Variable,
            ItemKind::Struct,
            ItemKind::Module,
            ItemKind::Keyword,
            ItemKind::Field,
            ItemKind::Constant,
        ] {
            let mut item = function("f", Some("fn()"));
            item.kind = kind;
            assert_eq!(Call::parentheses(&item, None, ""), None, "{kind:?}");
        }
        let mut method = function("push", Some("fn(&mut self, value: T)"));
        method.kind = ItemKind::Method;
        assert_eq!(Call::parentheses(&method, None, ""), Some(true));
    }

    #[test]
    fn a_function_that_is_called_already_or_is_imported_or_has_its_arguments_is_left_as_it_is() {
        let item = function("f", Some("fn(x: u32)"));
        assert_eq!(
            Call::parentheses(&item, Some(b'('), ""),
            None,
            "already called: `f|(1)`"
        );
        assert_eq!(Call::parentheses(&item, Some(b')'), ""), Some(true));
        let mut with_arguments = function("f", Some("fn(x: u32)"));
        with_arguments.edit.text = "f(${1:x})".to_owned();
        assert_eq!(
            Call::parentheses(&with_arguments, None, ""),
            None,
            "the server made the call"
        );
        for line in [
            "use std::mem::",
            "    use std::mem::",
            "pub use a::",
            "pub(crate) use a::",
            "pub(in a::b) use c::",
        ] {
            assert_eq!(Call::parentheses(&item, None, line), None, "{line:?}");
        }
        for line in ["let used = ", "    users::", "x.", "pub fn f() { "] {
            assert_eq!(Call::parentheses(&item, None, line), Some(true), "{line:?}");
        }
    }
}
