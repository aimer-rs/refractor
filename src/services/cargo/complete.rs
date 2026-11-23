//! What could be written at the caret of a manifest, as the schema says it: the names of tables inside `[ ]`, the keys at the start of a
//! line or inside `{ }`, and the values a key may have after `key =`.
//!
//! The line the caret is in is read as text and not through the scan, since the line is what is half typed.

use std::collections::HashSet;

use serde_json::Value as Json;

use super::scan::{Entry, EntryKind};
use super::schema::{Alt, Schema, Walk, describe, docs_of};

/// Where in a manifest the caret is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// A name inside the brackets of a table header.
    Table,
    /// A key, at the start of a line or inside `{ }`.
    Key,
    /// A value, after `key =`.
    Value,
}

/// One thing that could be written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub label: String,
    /// What replaces the word before the caret when it is chosen.
    pub insert: String,
    pub detail: String,
    pub doc: Option<String>,
}

/// What could be written at the caret, and where the word being typed begins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    pub place: Place,
    /// The byte where the word before the caret begins.
    pub start: usize,
    pub items: Vec<Candidate>,
}

/// The names of a dotted fragment, without quotes: `a."b c".d` is `a`, `b c` and `d`.
fn pieces(fragment: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = fragment;
    loop {
        rest = rest.trim_start_matches(|c: char| c == '.' || c.is_whitespace());
        let Some(first) = rest.chars().next() else {
            return out;
        };
        let end = if first == '"' || first == '\'' {
            rest[1..]
                .find(first)
                .map_or(rest.len(), |at| at + 2)
        } else {
            rest.find(|c: char| c == '.' || c.is_whitespace())
                .unwrap_or(rest.len())
        };
        out.push(
            rest[..end]
                .trim_matches(|c| c == '"' || c == '\'')
                .to_owned(),
        );
        rest = &rest[end..];
    }
}

fn key_items(schema: &Schema, alts: &[Alt<'_>], skip: &HashSet<String>) -> Vec<Candidate> {
    Schema::keys_of(alts)
        .into_iter()
        .filter(|(label, _)| !skip.contains(*label))
        .map(|(label, node)| {
            let expanded = schema.expand(node);
            Candidate {
                label: label.to_owned(),
                insert: label.to_owned(),
                detail: describe(schema, &expanded),
                doc: docs_of(&expanded)
                    .first()
                    .map(|doc| (*doc).to_owned()),
            }
        })
        .collect()
}

fn value_items(alts: &[Alt<'_>], quoted: bool) -> Vec<Candidate> {
    let mut seen: Vec<Json> = Vec::new();
    let mut items = Vec::new();
    let mut add = |value: Json, detail: &str| {
        if seen.contains(&value) {
            return;
        }
        let (label, insert) = match &value {
            Json::String(text) if quoted => (text.clone(), text.clone()),
            Json::String(text) => (text.clone(), format!("\"{text}\"")),
            other => (other.to_string(), other.to_string()),
        };
        items.push(Candidate {
            label,
            insert,
            detail: detail.to_owned(),
            doc: None,
        });
        seen.push(value);
    };
    for alt in alts {
        let members = alt.node.get("enum").and_then(Json::as_array);
        for member in members.into_iter().flatten() {
            add(member.clone(), "value");
        }
        if members.is_none() && alt.types().contains(&"boolean") {
            add(Json::Bool(true), "boolean");
            add(Json::Bool(false), "boolean");
        }
    }
    items
}

fn is_key_text(text: &str, extra: &[char]) -> bool {
    text.chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || extra.contains(&c))
}

/// What could be written at `offset` of `text`, whose scan is `entries`; `None` where nothing is to be offered.
pub fn complete(
    schema: &Schema,
    text: &str,
    entries: &[Entry],
    offset: usize,
) -> Option<Completion> {
    let offset = offset.min(text.len());
    if !text.is_char_boundary(offset) {
        return None;
    }
    let line_start = text[..offset]
        .rfind('\n')
        .map_or(0, |at| at + 1);
    let current = &text[line_start..offset];
    let trimmed = current.trim_start();

    // A name in a table header.
    if let Some(after) = trimmed.strip_prefix('[') {
        let typed = after
            .strip_prefix('[')
            .unwrap_or(after)
            .trim_start();
        if !typed.contains(']') {
            let cut = typed.rfind('.').map_or(0, |at| at + 1);
            let names = pieces(&typed[..cut.saturating_sub(1)]);
            let Walk::Found { alts, .. } = schema.walk(&names) else {
                return None;
            };
            let items = key_items(schema, &alts, &HashSet::new());
            return (!items.is_empty()).then(|| Completion {
                place: Place::Table,
                start: offset - (typed.len() - cut),
                items,
            });
        }
    }

    // In the middle of a value that spans lines, such as a long array: not a place for a key.
    let inside = entries.iter().any(|entry| {
        entry.kind == EntryKind::Key
            && entry.value.as_ref().is_some_and(|value| {
                value.span.start < offset
                    && offset <= value.span.end
                    && text[value.span.start..offset].contains('\n')
            })
    });
    if inside {
        return None;
    }

    // The table the caret is in: the closest header above.
    let section: Vec<&str> = entries
        .iter()
        .rev()
        .find(|entry| entry.kind == EntryKind::Header && entry.span.start < line_start)
        .map(|entry| {
            entry
                .segments
                .iter()
                .map(|segment| segment.name.as_str())
                .collect()
        })
        .unwrap_or_default();
    let walk_from = |extra: &[String]| {
        let mut path: Vec<&str> = section.clone();
        path.extend(extra.iter().map(String::as_str));
        match schema.walk(&path) {
            Walk::Found { alts, .. } => Some(alts),
            _ => None,
        }
    };

    if let Some((key, after)) = trimmed.split_once('=')
        && is_key_text(key.trim_end(), &['.', '"', '\''])
        && !key.trim().is_empty()
    {
        let after = after.trim_start();
        // `key = ` then the caret: a value.
        let quoted = after.starts_with('"');
        let rest = if quoted { &after[1..] } else { after };
        if !rest.contains('"') && !rest.contains(['{', '[']) {
            let alts = walk_from(&pieces(key))?;
            let items = value_items(&alts, quoted);
            return (!items.is_empty()).then(|| Completion {
                place: Place::Value,
                start: offset - rest.len(),
                items,
            });
        }
        // Inside `{ }` on this line: a key of the table.
        if let Some(inner) = after.strip_prefix('{')
            && !inner.contains('}')
        {
            let tail = inner
                .rsplit(',')
                .next()
                .unwrap_or_default()
                .trim_start();
            if !is_key_text(tail, &[]) {
                return None;
            }
            let alts = walk_from(&pieces(key))?;
            let taken: HashSet<String> = inner
                .split(',')
                .filter_map(|part| part.split_once('='))
                .map(|(name, _)| name.trim().to_owned())
                .collect();
            let items = key_items(schema, &alts, &taken);
            return (!items.is_empty()).then(|| Completion {
                place: Place::Key,
                start: offset - tail.len(),
                items,
            });
        }
        return None;
    }

    // A key at the start of a line.
    if is_key_text(trimmed, &['.']) {
        let cut = trimmed.rfind('.').map_or(0, |at| at + 1);
        let base = pieces(&trimmed[..cut.saturating_sub(1)]);
        let alts = walk_from(&base)?;
        let taken: HashSet<String> = entries
            .iter()
            .filter(|entry| {
                entry.kind == EntryKind::Key
                    && section.len() + base.len() < entry.segments.len()
                    && section
                        .iter()
                        .map(|name| name.to_string())
                        .chain(base.iter().cloned())
                        .zip(&entry.segments)
                        .all(|(name, segment)| name == segment.name)
            })
            .map(|entry| {
                entry.segments[section.len() + base.len()]
                    .name
                    .clone()
            })
            .collect();
        let items = key_items(schema, &alts, &taken);
        return (!items.is_empty()).then(|| Completion {
            place: Place::Key,
            start: offset - (trimmed.len() - cut),
            items,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::cargo::scan::Scan;

    /// The completion at the `|` of `marked`, which is not part of the text.
    fn at(marked: &str) -> Option<(Completion, String)> {
        let offset = marked.find('|').expect("a caret");
        let text = marked.replacen('|', "", 1);
        let found = complete(Schema::cargo(), &text, &Scan::of(&text).entries, offset)?;
        let word = text[found.start..offset].to_owned();
        Some((found, word))
    }

    fn labels(found: &Completion) -> Vec<&str> {
        found
            .items
            .iter()
            .map(|item| item.label.as_str())
            .collect()
    }

    #[test]
    fn table_names_are_offered_inside_the_brackets_and_after_a_dot() {
        let (found, word) = at("[pa|").expect("offered");
        assert_eq!((found.place, word.as_str()), (Place::Table, "pa"));
        assert!(labels(&found).contains(&"package"));
        let (found, word) = at("[[b|").expect("offered");
        assert_eq!(word, "b");
        assert!(labels(&found).contains(&"bin"));
        let (found, word) = at("[workspace.|").expect("offered");
        assert_eq!(word, "");
        assert!(labels(&found).contains(&"members"), "{:?}", labels(&found));
        assert!(
            at("[profile.|").is_none(),
            "a map of any names has none to offer"
        );
    }

    #[test]
    fn keys_are_offered_at_the_start_of_a_line_without_those_already_there() {
        let (found, word) = at("[package]\nname = \"a\"\ned|").expect("offered");
        assert_eq!((found.place, word.as_str()), (Place::Key, "ed"));
        let names = labels(&found);
        assert!(names.contains(&"edition") && names.contains(&"version"));
        assert!(
            !names.contains(&"name"),
            "a key that is there is not offered"
        );
        let (found, _) = at("|").expect("offered at the top");
        assert!(labels(&found).contains(&"package"));
        let (found, word) = at("[workspace]\npackage.|").expect("dotted");
        assert_eq!(word, "");
        assert!(labels(&found).contains(&"edition"), "{:?}", labels(&found));
        assert!(
            at("[profile.release]\nlto.|").is_none(),
            "lto is not a table"
        );
    }

    #[test]
    fn values_are_offered_after_the_equal_sign() {
        let (found, word) = at("[profile.release]\nopt-level = |").expect("offered");
        assert_eq!((found.place, word.as_str()), (Place::Value, ""));
        let inserts: Vec<_> = found
            .items
            .iter()
            .map(|item| item.insert.as_str())
            .collect();
        assert!(
            inserts.contains(&"\"s\"") && inserts.contains(&"3"),
            "{inserts:?}"
        );
        let (found, word) = at("[profile.release]\nopt-level = \"z|").expect("quoted");
        assert_eq!(word, "z");
        assert!(
            found
                .items
                .iter()
                .any(|item| item.insert == "z"),
            "no quote is added inside quotes"
        );
        let (found, _) = at("[profile.release]\nlto = |").expect("a boolean");
        assert!(labels(&found).contains(&"true") && labels(&found).contains(&"false"));
        assert!(
            at("[package]\nname = |").is_none(),
            "a free string has no values"
        );
    }

    #[test]
    fn keys_of_an_inline_table_are_offered_without_those_already_there() {
        let (found, word) = at("[dependencies]\nserde = { version = \"1\", fea|").expect("offered");
        assert_eq!((found.place, word.as_str()), (Place::Key, "fea"));
        let names = labels(&found);
        assert!(names.contains(&"features") && names.contains(&"path"));
        assert!(!names.contains(&"version"));
    }

    #[test]
    fn nothing_is_offered_inside_a_value_that_spans_lines_or_past_the_end() {
        assert!(at("[package]\nkeywords = [\n  \"a\",\n  |\n]").is_none());
        let text = "caf\u{e9}";
        assert!(
            complete(Schema::cargo(), text, &[], 4).is_none(),
            "inside a character"
        );
        assert!(
            complete(Schema::cargo(), "", &[], 99).is_some(),
            "past the end is the end"
        );
    }
}
