//! The problems that the schema finds in a manifest, and the card shown for a key.
//!
//! An unknown key is a warning, as Cargo says it as "unused manifest key"; a value of the wrong type, one outside the values a key may
//! have, and a version that is not a full one are errors. What the schema leaves open is not checked, nor is text that is half typed.

use std::ops::Range;

use serde_json::Value as Json;

use super::scan::{Entry, EntryKind, Kind, Segment, Value};
use super::schema::{Alt, Schema, Walk, describe, docs_of, type_name};

/// How bad a problem is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// Something wrong at a place in the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub span: Range<usize>,
    pub severity: Severity,
    pub message: String,
}

/// The words for a key, and the place they are for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hover {
    pub span: Range<usize>,
    pub markdown: String,
}

/// Why a value does not fit a node.
enum Mismatch<'a> {
    Type,
    Enum(&'a [Json]),
    Version,
    Range(String, String),
    Item(&'a Value),
}

/// The schema name of the kind of a value; `None` for text that is not understood.
fn kind_name(kind: &Kind) -> Option<&'static str> {
    Some(match kind {
        Kind::String => "string",
        Kind::Boolean => "boolean",
        Kind::Integer => "integer",
        Kind::Float => "number",
        Kind::Array(_) => "array",
        Kind::Table => "object",
        Kind::Date => "date",
        Kind::Unknown => return None,
    })
}

/// Whether `text` is a full version, as `1.2.3`, with an optional `-pre` and `+build`: the one pattern the schema has.
pub fn is_version(text: &str) -> bool {
    let (rest, build) = match text.split_once('+') {
        Some((rest, build)) => (rest, Some(build)),
        None => (text, None),
    };
    let (core, pre) = match rest.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (rest, None),
    };
    let number = |part: &str| {
        !part.is_empty()
            && part.bytes().all(|byte| byte.is_ascii_digit())
            && (part == "0" || !part.starts_with('0'))
    };
    let identifiers = |part: &str, numeric_without_zero: bool| {
        part.split('.').all(|id| {
            !id.is_empty()
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && (!numeric_without_zero
                    || !id.bytes().all(|byte| byte.is_ascii_digit())
                    || number(id))
        })
    };
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| number(part))
        && pre.is_none_or(|pre| identifiers(pre, true))
        && build.is_none_or(|build| identifiers(build, false))
}

fn mismatch<'a>(schema: &'a Schema, alt: &Alt<'a>, value: &'a Value) -> Option<Mismatch<'a>> {
    let accepted = alt.types();
    if accepted.contains(&"any") {
        return None;
    }
    let kind = kind_name(&value.kind)?;
    if !(accepted.contains(&kind) || (kind == "integer" && accepted.contains(&"number"))) {
        return Some(Mismatch::Type);
    }
    if let Some(members) = alt.node.get("enum").and_then(Json::as_array) {
        let ignore_case = alt.node.get("x-ignore-case").is_some();
        let found = match &value.kind {
            Kind::String => members.iter().any(|member| {
                member.as_str().is_some_and(|member| {
                    if ignore_case {
                        member.eq_ignore_ascii_case(&value.text)
                    } else {
                        member == value.text
                    }
                })
            }),
            Kind::Boolean => members
                .iter()
                .any(|member| *member == Json::Bool(value.text == "true")),
            Kind::Integer | Kind::Float => {
                let given = value.text.replace('_', "").parse::<f64>().ok();
                given.is_none()
                    || members
                        .iter()
                        .any(|member| member.as_f64() == given)
            }
            _ => true,
        };
        if !found {
            return Some(Mismatch::Enum(members));
        }
    }
    if matches!(value.kind, Kind::String)
        && alt.node.get("pattern").is_some()
        && !is_version(&value.text)
    {
        return Some(Mismatch::Version);
    }
    if matches!(value.kind, Kind::Integer)
        && let Ok(number) = value.text.replace('_', "").parse::<f64>()
    {
        let minimum = alt.node.get("minimum").and_then(Json::as_f64);
        let maximum = alt.node.get("maximum").and_then(Json::as_f64);
        if minimum.is_some_and(|minimum| number < minimum)
            || maximum.is_some_and(|maximum| number > maximum)
        {
            let shown = |bound: Option<f64>| {
                bound.map_or_else(|| "?".to_owned(), |bound| bound.to_string())
            };
            return Some(Mismatch::Range(shown(minimum), shown(maximum)));
        }
    }
    if let (Kind::Array(items), Some(items_node)) = (&value.kind, alt.node.get("items")) {
        let item_alts = schema.expand(items_node);
        if !item_alts.is_empty() {
            for item in items {
                if !matches!(item.kind, Kind::Table)
                    && item_alts
                        .iter()
                        .all(|candidate| mismatch(schema, candidate, item).is_some())
                {
                    return Some(Mismatch::Item(item));
                }
            }
        }
    }
    None
}

fn shown(value: &Value) -> String {
    if matches!(value.kind, Kind::String) {
        format!("\"{}\"", value.text)
    } else {
        value.text.clone()
    }
}

fn problem_for(schema: &Schema, alts: &[Alt<'_>], value: &Value) -> Option<String> {
    if alts.is_empty() || matches!(value.kind, Kind::Unknown) {
        return None;
    }
    let reasons: Vec<Option<Mismatch>> = alts
        .iter()
        .map(|alt| mismatch(schema, alt, value))
        .collect();
    if reasons.iter().any(Option::is_none) {
        return None;
    }
    let reasons: Vec<Mismatch> = reasons.into_iter().flatten().collect();
    let near = reasons.iter().find(|reason| {
        matches!(
            reason,
            Mismatch::Enum(_) | Mismatch::Version | Mismatch::Range(..)
        )
    });
    match near {
        Some(Mismatch::Enum(members)) => {
            // The other shapes that the key takes (`jobs` is an integer or "default") are part of what is expected.
            let others: Vec<Alt> = alts
                .iter()
                .filter(|alt| {
                    alt.node.get("enum").is_none()
                        && !alt.is_inherit()
                        && !alt.types().contains(&"object")
                })
                .copied()
                .collect();
            let list = members
                .iter()
                .map(Json::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            let more = if others.is_empty() {
                String::new()
            } else {
                format!(", or {}", describe(schema, &others))
            };
            return Some(format!(
                "Invalid value {}. Expected one of: {list}{more}.",
                shown(value)
            ));
        }
        Some(Mismatch::Version) => {
            return Some(format!(
                "Invalid version {}. Expected a full version such as 1.2.3.",
                shown(value)
            ));
        }
        Some(Mismatch::Range(minimum, maximum)) => {
            return Some(format!(
                "Value {} is out of range ({minimum} to {maximum}).",
                value.text
            ));
        }
        _ => {}
    }
    if let Some(Mismatch::Item(item)) = reasons
        .iter()
        .find(|reason| matches!(reason, Mismatch::Item(_)))
    {
        let array = alts
            .iter()
            .find(|alt| alt.types().contains(&"array"))
            .and_then(|alt| alt.node.get("items"));
        let wanted =
            array.map_or_else(String::new, |items| describe(schema, &schema.expand(items)));
        let found = kind_name(&item.kind).map_or("value", type_name);
        return Some(format!("Array items must be {wanted}, found {found}."));
    }
    let found = kind_name(&value.kind).map_or("value", type_name);
    Some(format!(
        "Expected {}, found {found}.",
        describe(schema, alts).replace(" | ", " or ")
    ))
}

/// How many edits make `a` into `b`.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (x, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = x + 1;
        for (y, cb) in b.iter().enumerate() {
            let above = row[y + 1];
            row[y + 1] = (row[y + 1] + 1)
                .min(row[y] + 1)
                .min(diagonal + usize::from(ca != *cb));
            diagonal = above;
        }
    }
    row[b.len()]
}

/// The key of the table that `name` is nearest to, if it is near.
fn suggestion<'a>(alts: &[Alt<'a>], name: &str) -> Option<&'a str> {
    let limit = 1.max(2.min(name.chars().count() / 3));
    Schema::keys_of(alts)
        .into_iter()
        .map(|(key, _)| (distance(name, key), key))
        .filter(|(score, _)| *score <= limit)
        .min_by_key(|(score, _)| *score)
        .map(|(_, key)| key)
}

fn names(segments: &[Segment]) -> Vec<&str> {
    segments
        .iter()
        .map(|segment| segment.name.as_str())
        .collect()
}

/// The problems in a manifest that was read.
pub fn check(schema: &Schema, entries: &[Entry]) -> Vec<Problem> {
    let mut problems = Vec::new();
    for entry in entries {
        let path = names(&entry.segments);
        match schema.walk(&path) {
            Walk::Unknown { index } => {
                let segment = &entry.segments[index];
                let Some(span) = segment
                    .span
                    .clone()
                    .filter(|_| index >= entry.section_len)
                else {
                    continue;
                };
                let parent = match schema.walk(&path[..index]) {
                    Walk::Found { alts, .. } => alts,
                    _ => Vec::new(),
                };
                let hint = suggestion(&parent, &segment.name);
                let place = path[..index]
                    .iter()
                    .filter(|name| **name != Segment::ELEMENT)
                    .copied()
                    .collect::<Vec<_>>()
                    .join(".");
                let mut message = format!("Unknown key `{}`", segment.name);
                if !place.is_empty() {
                    message.push_str(&format!(" in `{place}`"));
                }
                message.push('.');
                if let Some(hint) = hint {
                    message.push_str(&format!(" Did you mean `{hint}`?"));
                }
                problems.push(Problem {
                    span,
                    severity: Severity::Warning,
                    message,
                });
            }
            Walk::Found { alts, .. } => {
                let table;
                let value = match (&entry.kind, &entry.value) {
                    (EntryKind::Key, Some(value)) => value,
                    _ => {
                        table = Value {
                            kind: Kind::Table,
                            text: String::new(),
                            span: 0..0,
                        };
                        &table
                    }
                };
                let Some(message) = problem_for(schema, &alts, value) else {
                    continue;
                };
                let span = if entry.kind == EntryKind::Header {
                    entry
                        .segments
                        .iter()
                        .rev()
                        .find_map(|segment| segment.span.clone())
                } else {
                    Some(value.span.clone()).filter(|span| !span.is_empty())
                };
                if let Some(span) = span {
                    problems.push(Problem {
                        span,
                        severity: Severity::Error,
                        message,
                    });
                }
            }
            Walk::Open { .. } | Walk::Scalar { .. } => {}
        }
    }
    problems
}

/// The card for the key or table name at `offset`, or `None` where there is none or the schema does not name it.
pub fn hover_at(schema: &Schema, entries: &[Entry], offset: usize) -> Option<Hover> {
    for entry in entries {
        let first = if entry.kind == EntryKind::Header {
            0
        } else {
            entry.section_len
        };
        let Some((index, span)) = entry
            .segments
            .iter()
            .enumerate()
            .skip(first)
            .find_map(|(index, segment)| {
                segment
                    .span
                    .clone()
                    .filter(|span| span.start <= offset && offset <= span.end)
                    .map(|span| (index, span))
            })
        else {
            continue;
        };
        let path = names(&entry.segments[..=index]);
        let Walk::Found {
            alts,
            declared: true,
        } = schema.walk(&path)
        else {
            return None;
        };
        let title = path
            .iter()
            .filter(|name| **name != Segment::ELEMENT)
            .copied()
            .collect::<Vec<_>>()
            .join(".");
        let mut markdown = format!("**{title}**\n\n");
        let docs = docs_of(&alts);
        if !docs.is_empty() {
            markdown.push_str(&docs.join("\n\n"));
            markdown.push_str("\n\n");
        }
        markdown.push_str(&format!("Type: `{}`", describe(schema, &alts)));
        if alts.iter().any(Alt::is_inherit) {
            markdown.push_str("\n\nCan be inherited from the workspace with `workspace = true`.");
        }
        if let Some(default) = alts.iter().find_map(Alt::default) {
            markdown.push_str(&format!("\n\nDefault: `{default}`"));
        }
        return Some(Hover { span, markdown });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::cargo::scan::Scan;

    fn problems(text: &str) -> Vec<(String, Severity, String)> {
        check(Schema::cargo(), &Scan::of(text).entries)
            .into_iter()
            .map(|problem| {
                (
                    text[problem.span].to_owned(),
                    problem.severity,
                    problem.message,
                )
            })
            .collect()
    }

    #[test]
    fn a_manifest_that_is_right_has_no_problem() {
        let text = "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nserde = { version = \"1\", features = [\"derive\"] }\nrand = \"0.8\"\n\n[features]\ndefault = [\"a\"]\na = []\n\n[[bin]]\nname = \"x\"\npath = \"src/x.rs\"\n\n[profile.release]\nopt-level = 3\nlto = true\n";
        assert_eq!(problems(text), []);
        assert_eq!(problems(""), []);
    }

    #[test]
    fn an_unknown_key_is_a_warning_that_says_what_was_meant() {
        let found = problems("[package]\nnme = \"x\"\n\n[pakage]\n");
        assert_eq!(found.len(), 2, "the key and the header: {found:?}");
        assert_eq!(found[1].0, "pakage");
        assert_eq!(found[0].0, "nme");
        assert_eq!(found[0].1, Severity::Warning);
        assert_eq!(
            found[0].2,
            "Unknown key `nme` in `package`. Did you mean `name`?"
        );
        let top = problems("pakage = 1\n");
        assert_eq!(top[0].2, "Unknown key `pakage`. Did you mean `package`?");
    }

    #[test]
    fn a_value_of_the_wrong_type_or_outside_the_values_is_an_error() {
        let found = problems("[package]\nname = 3\n");
        assert_eq!(found[0].0, "3");
        assert_eq!(found[0].1, Severity::Error);
        assert!(
            found[0]
                .2
                .starts_with("Expected string, found integer"),
            "{}",
            found[0].2
        );
        let found = problems("[profile.release]\nopt-level = \"fast\"\n");
        assert!(
            found[0]
                .2
                .starts_with("Invalid value \"fast\". Expected one of:"),
            "{}",
            found[0].2
        );
        let found = problems("[lints.rust]\nunused = \"loud\"\n");
        assert!(
            found
                .iter()
                .any(|f| f.2.contains("Invalid value")),
            "{found:?}"
        );
    }

    #[test]
    fn a_version_must_be_a_full_one() {
        let found = problems("[package]\nname = \"a\"\nversion = \"1.0\"\n");
        assert_eq!(
            found[0].2,
            "Invalid version \"1.0\". Expected a full version such as 1.2.3."
        );
        for good in [
            "1.2.3",
            "0.0.0",
            "1.2.3-alpha.1",
            "1.2.3+build.5",
            "1.2.3-rc.1+x",
        ] {
            assert!(is_version(good), "{good}");
        }
        for bad in [
            "1.2", "01.2.3", "1.2.3.4", "", "a.b.c", "1.2.3-", "1.2.3-01", "1.2.3+",
        ] {
            assert!(!is_version(bad), "{bad}");
        }
    }

    #[test]
    fn half_typed_text_and_open_tables_are_not_reported() {
        assert_eq!(problems("[package]\nname =\n"), []);
        assert_eq!(
            problems("[package.metadata.docs]\nanything = { a = 1 }\n"),
            []
        );
        assert_eq!(
            problems("[dependencies]\nserde = \"1\"\nx = { path = \"../x\" }\n"),
            []
        );
    }

    #[test]
    fn a_key_has_a_card_with_its_type_and_what_the_schema_says() {
        let text = "[package]\nname = \"a\"\n";
        let entries = Scan::of(text).entries;
        let at = text.find("package").expect("in text") + 2;
        let card = hover_at(Schema::cargo(), &entries, at).expect("a card");
        assert_eq!(&text[card.span.clone()], "package");
        assert!(
            card.markdown.starts_with("**package**"),
            "{}",
            card.markdown
        );
        assert!(card.markdown.contains("Type: `"), "{}", card.markdown);
        let at = text.find("name").expect("in text");
        let card = hover_at(Schema::cargo(), &entries, at).expect("a card");
        assert!(card.markdown.starts_with("**package.name**"));
        assert_eq!(hover_at(Schema::cargo(), &entries, text.len()), None);
        let text = "[dependencies]\nserde = \"1\"\n";
        let entries = Scan::of(text).entries;
        let at = text.find("serde").expect("in text");
        assert_eq!(
            hover_at(Schema::cargo(), &entries, at),
            None,
            "a crate is not documented by the schema"
        );
    }
}
