//! What the Cargo manifest schema (`manifest.schema.json`, from `cargo-util-schemas`) says of a key path: which keys a table may
//! have, of what types, and what they are documented as.
//!
//! The schema is read in place: what is found is a reference into it, so that asking about a key costs a walk of a few nodes and no
//! copy of the schema.

use std::collections::HashSet;
use std::sync::OnceLock;

use serde_json::Value;

/// The schema, as JSON that is parsed once.
pub struct Schema {
    root: Value,
}

/// A node of the schema that a value may be, with the description that it is to be shown with.
#[derive(Clone, Copy)]
pub struct Alt<'a> {
    pub node: &'a Value,
    doc: Option<&'a str>,
}

/// What is found at the end of a path of keys.
pub enum Walk<'a> {
    /// The nodes the key may be; `declared` is true for a named property and false for a map entry such as a crate name.
    Found { alts: Vec<Alt<'a>>, declared: bool },
    /// Nothing constrains it, such as `package.metadata`.
    Open { index: usize },
    /// A table that does not have the key at `index`.
    Unknown { index: usize },
    /// The parent at `index` is not a table, which the check of its type says.
    Scalar { index: usize },
}

static NOTHING: Value = Value::Null;

impl Schema {
    /// The Cargo manifest schema.
    pub fn cargo() -> &'static Schema {
        static CARGO: OnceLock<Schema> = OnceLock::new();
        CARGO.get_or_init(|| {
            Schema::parse(include_str!("manifest.schema.json"))
                .expect("the bundled manifest schema is JSON")
        })
    }

    /// A schema from its JSON text.
    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        Ok(Self {
            root: serde_json::from_str(text)?,
        })
    }

    /// Follows a `$ref`, and turns the boolean schema `true` into one that allows anything. A description written beside the reference
    /// is used where the target has none. `None` for what is not a schema.
    fn resolve<'a>(&'a self, node: &'a Value) -> Option<Alt<'a>> {
        match node {
            Value::Bool(true) | Value::Null => Some(Alt {
                node: &NOTHING,
                doc: None,
            }),
            Value::Object(map) => {
                let Some(reference) = map.get("$ref").and_then(Value::as_str) else {
                    return Some(Alt { node, doc: None });
                };
                let name = reference.trim_start_matches("#/$defs/");
                let target = self
                    .root
                    .get("$defs")
                    .and_then(|defs| defs.get(name));
                let mut found = match target {
                    Some(target) => self.resolve(target)?,
                    None => Alt {
                        node: &NOTHING,
                        doc: None,
                    },
                };
                if found.description().is_none() {
                    found.doc = map.get("description").and_then(Value::as_str);
                }
                Some(found)
            }
            _ => None,
        }
    }

    /// Every concrete alternative of `node`: `anyOf` and `oneOf` are flattened and `null` is dropped.
    pub fn expand<'a>(&'a self, node: &'a Value) -> Vec<Alt<'a>> {
        let mut out = Vec::new();
        self.expand_into(node, &mut out, None);
        out
    }

    fn expand_into<'a>(
        &'a self,
        node: &'a Value,
        out: &mut Vec<Alt<'a>>,
        inherited: Option<&'a str>,
    ) {
        let Some(found) = self.resolve(node) else {
            return;
        };
        let description = found.description().or(inherited);
        let branches = found
            .node
            .get("anyOf")
            .or_else(|| found.node.get("oneOf"))
            .and_then(Value::as_array);
        if let Some(branches) = branches {
            for branch in branches {
                self.expand_into(branch, out, description);
            }
            return;
        }
        if found.node.get("type").is_some() && declared_types(found.node).is_empty() {
            return;
        }
        out.push(Alt {
            node: found.node,
            doc: description,
        });
    }

    /// The nodes for `segment` inside `alts`; `[]` is an element of an array.
    fn child_of<'a>(&'a self, alts: &[Alt<'a>], segment: &str) -> Step<'a> {
        let mut found = Vec::new();
        let (mut declared, mut open, mut closed) = (false, false, false);
        for alt in alts {
            if segment == "[]" {
                match alt.node.get("items") {
                    Some(items) => found.extend(self.expand(items)),
                    None if alt.types().contains(&"any") => open = true,
                    None => {}
                }
                continue;
            }
            if let Some(property) = alt
                .node
                .get("properties")
                .and_then(|properties| properties.get(segment))
            {
                found.extend(self.expand(property));
                declared = true;
                continue;
            }
            if !alt.is_object_like() {
                continue;
            }
            match alt.node.get("additionalProperties") {
                Some(Value::Bool(false)) | None => {
                    if alt.node.get("properties").is_some() {
                        closed = true;
                    } else {
                        open = true;
                    }
                }
                Some(more) => found.extend(self.expand(more)),
            }
        }
        if !found.is_empty() {
            Step::Found(found, declared)
        } else if open {
            Step::Open
        } else if closed {
            Step::Unknown
        } else {
            Step::Scalar
        }
    }

    /// Walks `names` from the root of the manifest.
    pub fn walk<'a, S: AsRef<str>>(&'a self, names: &[S]) -> Walk<'a> {
        let mut alts = self.expand(&self.root);
        let mut declared = false;
        for (index, name) in names.iter().enumerate() {
            match self.child_of(&alts, name.as_ref()) {
                Step::Found(next, is_declared) => {
                    alts = next;
                    declared = is_declared;
                }
                Step::Open => return Walk::Open { index },
                Step::Unknown => return Walk::Unknown { index },
                Step::Scalar => return Walk::Scalar { index },
            }
        }
        Walk::Found { alts, declared }
    }

    /// The names a table at this path may have, in the hyphenated spelling: `dev_dependencies` is an alias that is not offered.
    pub fn keys_of<'a>(alts: &[Alt<'a>]) -> Vec<(&'a str, &'a Value)> {
        let mut seen = HashSet::new();
        let mut keys = Vec::new();
        for alt in alts {
            let Some(properties) = alt
                .node
                .get("properties")
                .and_then(Value::as_object)
            else {
                continue;
            };
            for (name, node) in properties {
                if name.contains('_') && properties.contains_key(&name.replace('_', "-")) {
                    continue;
                }
                if node.get("x-alias").is_some() {
                    continue;
                }
                if seen.insert(name.as_str()) {
                    keys.push((name.as_str(), node));
                }
            }
        }
        keys
    }
}

enum Step<'a> {
    Found(Vec<Alt<'a>>, bool),
    Open,
    Unknown,
    Scalar,
}

/// The types a node says, without `null`.
fn declared_types(node: &Value) -> Vec<&str> {
    match node.get("type") {
        Some(Value::String(one)) => vec![one.as_str()],
        Some(Value::Array(many)) => many.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
    .into_iter()
    .filter(|kind| *kind != "null")
    .collect()
}

impl<'a> Alt<'a> {
    /// The words the schema has for this node.
    pub fn description(&self) -> Option<&'a str> {
        self.node
            .get("description")
            .and_then(Value::as_str)
            .or(self.doc)
    }

    /// The types a value of this node may have: `any` where it says none.
    pub fn types(&self) -> Vec<&'a str> {
        let declared = declared_types(self.node);
        if !declared.is_empty() {
            return declared;
        }
        if self.node.get("properties").is_some() || self.node.get("additionalProperties").is_some()
        {
            return vec!["object"];
        }
        if self.node.get("items").is_some() {
            return vec!["array"];
        }
        if let Some(members) = self.node.get("enum").and_then(Value::as_array) {
            let mut kinds: Vec<&str> = Vec::new();
            for member in members {
                let kind = match member {
                    Value::Number(_) => "integer",
                    Value::Bool(_) => "boolean",
                    Value::String(_) => "string",
                    _ => continue,
                };
                if !kinds.contains(&kind) {
                    kinds.push(kind);
                }
            }
            return kinds;
        }
        vec!["any"]
    }

    pub fn is_object_like(&self) -> bool {
        self.types()
            .iter()
            .any(|kind| *kind == "object" || *kind == "any")
    }

    /// `{ workspace = true }`: the alternative that every field that can be inherited, and every dependency, has.
    pub fn is_inherit(&self) -> bool {
        let required = self
            .node
            .get("required")
            .and_then(Value::as_array)
            .is_some_and(|names| names.iter().any(|name| name == "workspace"));
        required
            && self
                .node
                .get("properties")
                .is_some_and(|properties| properties.get("workspace").is_some())
    }

    /// The default the schema says, if any.
    pub fn default(&self) -> Option<&'a Value> {
        self.node
            .get("default")
            .filter(|value| !value.is_null())
    }

    fn summary(&self, schema: &Schema) -> String {
        if let Some(members) = self.node.get("enum").and_then(Value::as_array) {
            return members
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join(" | ");
        }
        let types = self.types();
        if types.contains(&"array")
            && let Some(items) = self.node.get("items")
        {
            let mut inner: Vec<String> = Vec::new();
            for alt in schema.expand(items) {
                for kind in alt.types() {
                    let name = type_name(kind).to_owned();
                    if !inner.contains(&name) {
                        inner.push(name);
                    }
                }
            }
            let inner = if inner.is_empty() {
                "any".to_owned()
            } else {
                inner.join(" | ")
            };
            return format!("array of {inner}");
        }
        types
            .iter()
            .map(|kind| type_name(kind))
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

/// The name of a schema type as it is shown.
pub fn type_name(kind: &str) -> &str {
    match kind {
        "object" => "table",
        "number" => "float",
        "any" => "any value",
        other => other,
    }
}

/// The types as they are shown: `string | table`, where the node can be either.
pub fn describe(schema: &Schema, alts: &[Alt<'_>]) -> String {
    let own: Vec<&Alt> = alts
        .iter()
        .filter(|alt| !alt.is_inherit())
        .collect();
    let chosen: Vec<&Alt> = if own.is_empty() {
        alts.iter().collect()
    } else {
        own.clone()
    };
    let mut parts: Vec<String> = Vec::new();
    for alt in chosen {
        let part = alt.summary(schema);
        if !part.is_empty() && !parts.contains(&part) {
            parts.push(part);
        }
    }
    if !own.is_empty()
        && own.len() < alts.len()
        && !own
            .iter()
            .any(|alt| alt.types().contains(&"object"))
    {
        parts.push("{ workspace = true }".to_owned());
    }
    parts.join(" | ")
}

/// The descriptions worth showing: the docs of the wrapper of a field that can be inherited only describe the Rust enum.
pub fn docs_of<'a>(alts: &[Alt<'a>]) -> Vec<&'a str> {
    const NOISE: [&str; 3] = [
        "The type that is used when not inheriting",
        "The type when inheriting",
        "An enum that allows for inheriting",
    ];
    let mut docs: Vec<&str> = Vec::new();
    for alt in alts {
        let Some(text) = alt.description() else {
            continue;
        };
        if NOISE
            .iter()
            .any(|noise| text.starts_with(noise))
        {
            continue;
        }
        if !docs.contains(&text) {
            docs.push(text);
        }
    }
    docs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found<'a>(walk: Walk<'a>) -> (Vec<Alt<'a>>, bool) {
        match walk {
            Walk::Found { alts, declared } => (alts, declared),
            _ => panic!("the path is found"),
        }
    }

    #[test]
    fn the_keys_of_the_manifest_and_of_a_table_are_known() {
        let schema = Schema::cargo();
        let (root, _) = found(schema.walk::<&str>(&[]));
        let names: Vec<_> = Schema::keys_of(&root)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        for expected in [
            "package",
            "dependencies",
            "features",
            "workspace",
            "profile",
        ] {
            assert!(names.contains(&expected), "{expected} in {names:?}");
        }
        assert!(
            !names.contains(&"dev_dependencies"),
            "an alias is not offered"
        );
        let (package, declared) = found(schema.walk(&["package"]));
        assert!(declared);
        let names: Vec<_> = Schema::keys_of(&package)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert!(
            names.contains(&"name") && names.contains(&"edition"),
            "{names:?}"
        );
    }

    #[test]
    fn a_crate_is_a_map_entry_and_an_unknown_key_of_a_closed_table_is_unknown() {
        let schema = Schema::cargo();
        let (serde, declared) = found(schema.walk(&["dependencies", "serde"]));
        assert!(!declared, "a crate name is not a property");
        assert!(!serde.is_empty());
        assert!(matches!(
            schema.walk(&["package", "nme"]),
            Walk::Unknown { index: 1 }
        ));
        assert!(matches!(
            schema.walk(&["package", "metadata", "anything", "deep"]),
            Walk::Open { .. }
        ));
    }

    #[test]
    fn types_are_described_with_their_documentation() {
        let schema = Schema::cargo();
        let (level, _) = found(schema.walk(&["profile", "release", "opt-level"]));
        let shown = describe(schema, &level);
        assert!(shown.contains("\"s\"") && shown.contains('3'), "{shown}");
        let (edition, _) = found(schema.walk(&["package", "edition"]));
        let shown = describe(schema, &edition);
        assert!(shown.contains("{ workspace = true }"), "{shown}");
        let (package, _) = found(schema.walk(&["package"]));
        assert!(docs_of(&package)[0].contains("`package`"));
        let (features, _) = found(schema.walk(&["features"]));
        assert!(describe(schema, &features).contains("table"));
        let (elements, _) = found(schema.walk(&["bin", "[]"]));
        assert!(!elements.is_empty(), "an array of tables has elements");
    }
}
