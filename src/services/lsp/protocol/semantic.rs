//! Semantic tokens: what a server knows of each name in a file that the grammar does not, such as whether a variable is mutable,
//! or a type is a struct or an enum.
//!
//! A server says in its answer to `initialize` which kinds of token it has and which modifiers a token can carry, as two lists of
//! names, its legend. `textDocument/semanticTokens/full` then answers with a list of numbers, five for each token: how far it is from
//! the one before, its length, its kind and its modifiers, the last two as the place of the name in the legend and a set of bits.

use std::path::Path;

use serde_json::{Value, json};

use super::uri_of;

/// The method that asks for the tokens of a whole file.
pub const SEMANTIC_TOKENS_FULL: &str = "textDocument/semanticTokens/full";
/// The method with which a server says that the tokens it gave are not as they are now, and are to be asked for again: it is what a server
/// that has finished loading a project uses to say that the names it did not know are known.
pub const SEMANTIC_TOKENS_REFRESH: &str = "workspace/semanticTokens/refresh";

/// What a server says its tokens are called: the kinds a token can be and the modifiers it can carry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Legend {
    pub types: Vec<String>,
    pub modifiers: Vec<String>,
}

impl Legend {
    /// The name of the kind of token that is number `index` of the legend.
    pub fn type_name(&self, index: u32) -> Option<&str> {
        self.types
            .get(index as usize)
            .map(String::as_str)
    }

    /// The names of the modifiers that are set in `bits`, in the order of the legend.
    pub fn modifier_names(&self, bits: u32) -> impl Iterator<Item = &str> {
        self.modifiers
            .iter()
            .enumerate()
            .filter(move |(index, _)| *index < 32 && bits & (1 << index) != 0)
            .map(|(_, name)| name.as_str())
    }
}

/// A token as the server said it, with its place as the protocol has it, which the text it is about is needed to turn into bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Raw {
    pub line: u32,
    /// Where it starts on its line, in the units that were agreed on.
    pub start: u32,
    /// How long it is, in the same units.
    pub length: u32,
    /// Which kind of the legend it is.
    pub kind: u32,
    /// Which modifiers of the legend it has, as bits.
    pub modifiers: u32,
}

/// The kinds and modifiers that a server of rust-analyzer's sort has, which a client says it can show: a server says only those that
/// its client does.
const TYPES: &[&str] = &[
    "namespace",
    "type",
    "class",
    "enum",
    "interface",
    "struct",
    "typeParameter",
    "parameter",
    "variable",
    "property",
    "enumMember",
    "event",
    "function",
    "method",
    "macro",
    "keyword",
    "modifier",
    "comment",
    "string",
    "number",
    "regexp",
    "operator",
    "decorator",
    "angle",
    "arithmetic",
    "attribute",
    "attributeBracket",
    "bitwise",
    "boolean",
    "brace",
    "bracket",
    "builtinAttribute",
    "builtinType",
    "character",
    "colon",
    "comma",
    "comparison",
    "constParameter",
    "const",
    "derive",
    "deriveHelper",
    "dot",
    "escapeSequence",
    "formatSpecifier",
    "generic",
    "label",
    "lifetime",
    "logical",
    "macroBang",
    "parenthesis",
    "procMacro",
    "punctuation",
    "selfKeyword",
    "selfTypeKeyword",
    "semicolon",
    "static",
    "toolModule",
    "typeAlias",
    "union",
    "unresolvedReference",
];
const MODIFIERS: &[&str] = &[
    "declaration",
    "definition",
    "static",
    "abstract",
    "deprecated",
    "readonly",
    "defaultLibrary",
    "async",
    "documentation",
    "attribute",
    "callable",
    "constant",
    "consuming",
    "controlFlow",
    "crateRoot",
    "injected",
    "intraDocLink",
    "library",
    "macro",
    "mutable",
    "procMacro",
    "public",
    "reference",
    "trait",
    "unsafe",
];

/// What the client says it can do about tokens: all of the full ones, in the relative encoding, with a token on one line at most.
pub fn client_capability() -> Value {
    json!({
        "dynamicRegistration": false,
        "requests": {"full": true, "range": false},
        "tokenTypes": TYPES,
        "tokenModifiers": MODIFIERS,
        "formats": ["relative"],
        "overlappingTokenSupport": false,
        "multilineTokenSupport": false,
    })
}

/// What the client says of the tokens of the whole workspace: that the server may tell it to ask for them again.
pub fn workspace_capability() -> Value {
    json!({"refreshSupport": true})
}

/// The legend of the server, from what it answered to `initialize`: none where it has no tokens of a whole file to give.
pub fn legend(result: &Value) -> Option<Legend> {
    let provider = result.pointer("/capabilities/semanticTokensProvider")?;
    // A server that gives the tokens of a range only has none of a file.
    match provider.get("full")? {
        Value::Bool(false) | Value::Null => return None,
        _ => {}
    }
    let names = |key: &str| -> Option<Vec<String>> {
        provider
            .pointer(&format!("/legend/{key}"))?
            .as_array()?
            .iter()
            .map(|name| name.as_str().map(str::to_owned))
            .collect()
    };
    Some(Legend {
        types: names("tokenTypes")?,
        modifiers: names("tokenModifiers")?,
    })
}

/// The parameters of `textDocument/semanticTokens/full` for the file `path`.
pub fn token_params(path: &Path) -> Value {
    json!({"textDocument": {"uri": uri_of(path)}})
}

/// The tokens the server sent, in the order it sent them, with their places made absolute. What is not made as it should be, a
/// token that is not whole, is left out with what follows it.
pub fn tokens(result: &Value) -> Vec<Raw> {
    let Some(data) = result.get("data").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut found = Vec::with_capacity(data.len() / 5);
    let (mut line, mut start) = (0u32, 0u32);
    for token in data.chunks_exact(5) {
        let mut numbers = [0u32; 5];
        for (number, value) in numbers.iter_mut().zip(token) {
            let Some(value) = value
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
            else {
                return found;
            };
            *number = value;
        }
        let [delta_line, delta_start, length, kind, modifiers] = numbers;
        line = line.saturating_add(delta_line);
        // On the same line a token is placed from the start of the one before, and on another from the start of its line.
        start = if delta_line == 0 {
            start.saturating_add(delta_start)
        } else {
            delta_start
        };
        found.push(Raw {
            line,
            start,
            length,
            kind,
            modifiers,
        });
    }
    found
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn provider(full: Value) -> Value {
        json!({"capabilities": {"semanticTokensProvider": {
            "legend": {"tokenTypes": ["namespace", "variable", "keyword"], "tokenModifiers": ["declaration", "mutable", "controlFlow"]},
            "full": full,
        }}})
    }

    #[test]
    fn the_legend_is_read_from_what_the_server_answers_to_initialize() {
        let expected = Legend {
            types: vec!["namespace".into(), "variable".into(), "keyword".into()],
            modifiers: vec!["declaration".into(), "mutable".into(), "controlFlow".into()],
        };
        assert_eq!(legend(&provider(json!(true))), Some(expected.clone()));
        assert_eq!(
            legend(&provider(json!({"delta": true}))),
            Some(expected),
            "a server that has deltas has the whole file as well"
        );
    }

    #[test]
    fn a_server_that_has_no_tokens_of_a_file_has_no_legend() {
        assert_eq!(legend(&provider(json!(false))), None);
        assert_eq!(legend(&provider(Value::Null)), None);
        assert_eq!(legend(&json!({"capabilities": {}})), None, "no provider");
        let mut range_only = provider(json!(true));
        range_only["capabilities"]["semanticTokensProvider"]
            .as_object_mut()
            .unwrap()
            .remove("full");
        assert_eq!(legend(&range_only), None, "only a range");
        let mut broken = provider(json!(true));
        broken["capabilities"]["semanticTokensProvider"]["legend"]["tokenTypes"] = json!(["a", 5]);
        assert_eq!(legend(&broken), None, "a name that is not a name");
    }

    #[test]
    fn a_legend_names_the_kind_of_a_token_and_the_modifiers_that_are_set() {
        let legend = legend(&provider(json!(true))).unwrap();
        assert_eq!(legend.type_name(1), Some("variable"));
        assert_eq!(legend.type_name(3), None, "a kind it does not have");
        assert_eq!(legend.modifier_names(0).count(), 0);
        assert_eq!(
            legend.modifier_names(0b101).collect::<Vec<_>>(),
            ["declaration", "controlFlow"]
        );
        assert_eq!(
            legend.modifier_names(0b1000).count(),
            0,
            "a bit that is not a modifier"
        );
        assert_eq!(
            legend.modifier_names(u32::MAX).count(),
            3,
            "and one that is outside the legend of a server of many"
        );
    }

    #[test]
    fn the_tokens_are_placed_from_the_one_before_on_a_line_and_from_the_start_of_the_line_on_another()
     {
        // `fn main() {` then `    let mut x = 1;`: three tokens on the first line and two on the second.
        let found = tokens(&json!({"resultId": "1", "data": [
            0, 0, 2, 2, 0,
            0, 3, 4, 1, 1,
            1, 4, 3, 2, 0,
            0, 4, 3, 2, 4,
            0, 4, 1, 1, 3,
        ]}));
        let at = |line, start, length, kind, modifiers| Raw {
            line,
            start,
            length,
            kind,
            modifiers,
        };
        assert_eq!(
            found,
            vec![
                at(0, 0, 2, 2, 0),
                at(0, 3, 4, 1, 1),
                at(1, 4, 3, 2, 0),
                at(1, 8, 3, 2, 4),
                at(1, 12, 1, 1, 3)
            ]
        );
    }

    #[test]
    fn what_is_not_made_as_it_should_be_is_left_out() {
        assert!(
            tokens(&Value::Null).is_empty(),
            "the server had none to say"
        );
        assert!(tokens(&json!({"data": []})).is_empty());
        assert!(tokens(&json!({"data": "no"})).is_empty());
        let whole = [0, 0, 2, 1, 0, 0, 3, 4, 1, 1];
        // A token that is not whole at the end is not one.
        let mut partial = whole.to_vec();
        partial.extend([1, 1]);
        assert_eq!(tokens(&json!({"data": partial})).len(), 2);
        // Nor is one that has a number that is not one, and what follows it is not trusted: it is placed from it.
        assert_eq!(
            tokens(&json!({"data": [0, 0, 2, 1, 0, 0, -3, 4, 1, 1, 0, 1, 1, 1, 0]})).len(),
            1
        );
        assert_eq!(
            tokens(&json!({"data": [0, 0, 2, 1, 0, 0, 3, 4294967296u64, 1, 1]})).len(),
            1,
            "a number that is too large"
        );
    }

    #[test]
    fn the_tokens_of_a_file_are_asked_for_by_its_address() {
        assert_eq!(
            token_params(&PathBuf::from("/p/src/main.rs")),
            json!({"textDocument": {"uri": "file:///p/src/main.rs"}})
        );
        assert_eq!(SEMANTIC_TOKENS_FULL, "textDocument/semanticTokens/full");
        assert_eq!(SEMANTIC_TOKENS_REFRESH, "workspace/semanticTokens/refresh");
    }

    #[test]
    fn the_server_is_told_that_the_client_has_the_tokens_of_a_file_and_may_be_told_to_ask_for_them_again()
     {
        let params = super::super::initialize_params(Path::new("/p"), 1);
        let tokens = &params["capabilities"]["textDocument"]["semanticTokens"];
        assert_eq!(tokens["requests"]["full"], json!(true));
        assert_eq!(tokens["formats"], json!(["relative"]));
        assert_eq!(
            tokens["multilineTokenSupport"],
            json!(false),
            "a token is on one line"
        );
        let kinds: Vec<&str> = tokens["tokenTypes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        let modifiers: Vec<&str> = tokens["tokenModifiers"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        for needed in [
            "struct",
            "enum",
            "interface",
            "variable",
            "parameter",
            "method",
            "function",
            "keyword",
            "static",
            "constParameter",
        ] {
            assert!(kinds.contains(&needed), "{needed}");
        }
        for needed in ["mutable", "readonly", "controlFlow", "constant", "static"] {
            assert!(modifiers.contains(&needed), "{needed}");
        }
        assert_eq!(
            params["capabilities"]["workspace"]["semanticTokens"]["refreshSupport"],
            json!(true)
        );
        assert!(
            serde_json::from_value::<lsp_types::InitializeParams>(params).is_ok(),
            "what the protocol has"
        );
    }
}
