//! Workspace symbols: the names of the definitions of the whole project that fit what was typed, which is what a search for a
//! type or a function asks the server.
//!
//! A server answers `workspace/symbol` with a list of symbols, each with its name, what kind of definition it is, the module it
//! is in and where it is.

use serde_json::{Value, json};

use super::super::position::Position;
use super::{Place, path_of};

/// The method that asks for the symbols that fit a query.
pub const WORKSPACE_SYMBOL: &str = "workspace/symbol";

/// What a definition is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolKind {
    Module,
    Struct,
    Enum,
    /// A trait, which the protocol calls an interface.
    Trait,
    Class,
    /// A type alias, which a server such as rust-analyzer sends as a type parameter.
    TypeAlias,
    Function,
    Method,
    Field,
    Constant,
    Variable,
    EnumMember,
    /// What is none of these.
    Other,
}

impl SymbolKind {
    /// Whether the definition is a type, which is what a search for types lists.
    pub const fn is_type(self) -> bool {
        matches!(
            self,
            Self::Struct | Self::Enum | Self::Trait | Self::Class | Self::TypeAlias
        )
    }

    /// The kind of completion item that is the same sort of thing, for what is drawn the same in both lists: a struct is the same
    /// mark in a list of definitions as in a menu of completions.
    pub const fn item_kind(self) -> super::ItemKind {
        use super::ItemKind as K;
        match self {
            Self::Module => K::Module,
            Self::Struct => K::Struct,
            Self::Enum => K::Enum,
            Self::Trait => K::Interface,
            Self::Class => K::Class,
            Self::TypeAlias => K::TypeParameter,
            Self::Function => K::Function,
            Self::Method => K::Method,
            Self::Field => K::Field,
            Self::Constant => K::Constant,
            Self::Variable => K::Variable,
            Self::EnumMember => K::EnumMember,
            Self::Other => K::Other,
        }
    }

    fn of(number: u64) -> Self {
        match number {
            2 | 3 | 4 => Self::Module,
            5 => Self::Class,
            6 | 9 => Self::Method,
            7 | 8 => Self::Field,
            10 => Self::Enum,
            11 => Self::Trait,
            12 => Self::Function,
            13 => Self::Variable,
            14 => Self::Constant,
            22 => Self::EnumMember,
            23 => Self::Struct,
            26 => Self::TypeAlias,
            _ => Self::Other,
        }
    }
}

/// A definition the server found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    /// What it is in, such as the module or the type that has it, as the server writes it.
    pub container: Option<String>,
    /// Where its name is.
    pub place: Place,
}

/// The settings of the server that the symbols need: the definitions that are not types are listed too, which the server
/// leaves out unless it is asked.
pub fn server_options() -> Value {
    json!({"workspace": {"symbol": {"search": {"kind": "all_symbols", "limit": 256}}}})
}

/// The parameters of `workspace/symbol` for `query`.
pub fn symbol_params(query: &str) -> Value {
    json!({"query": query})
}

/// The symbols of an answer, in the order the server gave them. What is not in a file of this machine, or is not made as it
/// should be, is left out; so is a symbol that says where its file is but not where in it, which cannot be gone to.
pub fn symbols(result: &Value) -> Vec<Symbol> {
    let Some(list) = result.as_array() else {
        return Vec::new();
    };
    list.iter().filter_map(symbol).collect()
}

fn symbol(item: &Value) -> Option<Symbol> {
    let name = item.get("name")?.as_str()?.to_owned();
    let location = item.get("location")?;
    let path = path_of(location.get("uri")?.as_str()?)?;
    let position = |pointer: &str| -> Option<Position> {
        let at = location.pointer(pointer)?;
        Some(Position {
            line: at.get("line")?.as_u64()?.try_into().ok()?,
            character: at.get("character")?.as_u64()?.try_into().ok()?,
        })
    };
    Some(Symbol {
        name,
        kind: SymbolKind::of(item.get("kind")?.as_u64()?),
        container: item
            .get("containerName")
            .and_then(Value::as_str)
            .filter(|container| !container.is_empty())
            .map(str::to_owned),
        place: Place {
            path,
            at: position("/range/start")?,
            end: position("/range/end")?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer() -> Value {
        json!([
            {"name": "RequestBuilder", "kind": 23, "containerName": "ureq::request",
             "location": {"uri": "file:///p/src/request.rs",
                          "range": {"start": {"line": 9, "character": 11}, "end": {"line": 9, "character": 25}}}},
            {"name": "request", "kind": 12,
             "location": {"uri": "file:///p/src/lib.rs",
                          "range": {"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 14}}}},
            {"name": "NotHere", "kind": 23, "containerName": "x",
             "location": {"uri": "https://example.com/x.rs",
                          "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}}}},
            {"name": "NoRange", "kind": 23, "location": {"uri": "file:///p/src/lib.rs"}},
            {"name": "NoKind", "location": {"uri": "file:///p/src/lib.rs",
                          "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}}}},
            {"name": "Empty", "kind": 10, "containerName": "",
             "location": {"uri": "file:///p/src/lib.rs",
                          "range": {"start": {"line": 1, "character": 5}, "end": {"line": 1, "character": 10}}}},
        ])
    }

    #[test]
    fn the_symbols_of_an_answer_are_in_the_order_given_with_what_cannot_be_gone_to_left_out() {
        let found = symbols(&answer());
        let names: Vec<&str> = found
            .iter()
            .map(|symbol| symbol.name.as_str())
            .collect();
        assert_eq!(names, ["RequestBuilder", "request", "Empty"]);
        let first = &found[0];
        assert_eq!(first.kind, SymbolKind::Struct);
        assert_eq!(first.container.as_deref(), Some("ureq::request"));
        assert_eq!(
            first.place.path,
            std::path::PathBuf::from("/p/src/request.rs")
        );
        assert_eq!(
            first.place.at,
            Position {
                line: 9,
                character: 11
            }
        );
        assert_eq!(
            first.place.end,
            Position {
                line: 9,
                character: 25
            }
        );
        assert_eq!(found[1].container, None);
        assert_eq!(found[2].container, None, "an empty container is none");
    }

    #[test]
    fn every_kind_of_definition_is_the_kind_of_item_that_is_the_same_sort_of_thing() {
        use super::super::ItemKind as K;
        for (symbol, item) in [
            (SymbolKind::Struct, K::Struct),
            (SymbolKind::Enum, K::Enum),
            (SymbolKind::Trait, K::Interface),
            (SymbolKind::Class, K::Class),
            (SymbolKind::TypeAlias, K::TypeParameter),
            (SymbolKind::Function, K::Function),
            (SymbolKind::Method, K::Method),
            (SymbolKind::Field, K::Field),
            (SymbolKind::Constant, K::Constant),
            (SymbolKind::Variable, K::Variable),
            (SymbolKind::Module, K::Module),
            (SymbolKind::EnumMember, K::EnumMember),
            (SymbolKind::Other, K::Other),
        ] {
            assert_eq!(symbol.item_kind(), item, "{symbol:?}");
        }
    }

    #[test]
    fn what_is_not_a_list_has_no_symbols() {
        assert!(symbols(&Value::Null).is_empty());
        assert!(symbols(&json!({"name": "x"})).is_empty());
        assert!(symbols(&json!([])).is_empty());
    }

    #[test]
    fn the_kinds_are_told_into_types_and_the_rest() {
        for (number, kind, is_type) in [
            (23, SymbolKind::Struct, true),
            (10, SymbolKind::Enum, true),
            (11, SymbolKind::Trait, true),
            (5, SymbolKind::Class, true),
            (26, SymbolKind::TypeAlias, true),
            (12, SymbolKind::Function, false),
            (6, SymbolKind::Method, false),
            (8, SymbolKind::Field, false),
            (14, SymbolKind::Constant, false),
            (2, SymbolKind::Module, false),
            (22, SymbolKind::EnumMember, false),
            (999, SymbolKind::Other, false),
        ] {
            assert_eq!(SymbolKind::of(number), kind);
            assert_eq!(kind.is_type(), is_type, "{kind:?}");
        }
    }

    #[test]
    fn the_request_and_the_settings_say_what_is_wanted() {
        assert_eq!(WORKSPACE_SYMBOL, "workspace/symbol");
        assert_eq!(symbol_params("req"), json!({"query": "req"}));
        assert_eq!(
            server_options()["workspace"]["symbol"]["search"]["kind"],
            json!("all_symbols")
        );
    }

    #[test]
    fn the_server_is_started_with_the_settings_of_the_symbols_and_of_the_lenses() {
        let params = super::super::initialize_params(std::path::Path::new("/p"), 1);
        let options = &params["initializationOptions"];
        assert_eq!(
            options["workspace"]["symbol"]["search"]["kind"],
            json!("all_symbols")
        );
        assert_eq!(options["lens"]["enable"], json!(true));
    }
}
