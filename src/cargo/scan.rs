//! A tolerant reader of `Cargo.toml`: every table header and every `key = value`, with where each is in the text. It never fails: text
//! that is half typed gives what could be read of it, which is what completion, hover and the checks are asked about.
//!
//! Places are bytes of the text. Every place a slice is made at is one that follows an ASCII byte, or the end, so none is inside a
//! character.

use std::ops::Range;

/// One name of a dotted key or of a table header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub name: String,
    /// Where the name is, without quotes; `None` for the `[]` that stands for an element of an array of tables.
    pub span: Option<Range<usize>>,
}

impl Segment {
    /// The name of the element of an array of tables.
    pub const ELEMENT: &'static str = "[]";

    fn element() -> Self {
        Self {
            name: Self::ELEMENT.to_owned(),
            span: None,
        }
    }
}

/// What a value is, as far as the checks need to tell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    String,
    Boolean,
    Integer,
    Float,
    Date,
    Array(Vec<Value>),
    Table,
    /// Text that is none of the others, such as what is half typed. It is not checked.
    Unknown,
}

/// A value: its kind, its text (for a string, what is between the quotes), and where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Value {
    pub kind: Kind,
    pub text: String,
    pub span: Range<usize>,
}

/// Whether an entry is a table header or a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Header,
    Key,
}

/// A table header, or a key with its value, with its whole path from the root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub kind: EntryKind,
    pub segments: Vec<Segment>,
    /// How many of the segments name the table the key is in; the rest are the key itself. Zero for a header.
    pub section_len: usize,
    pub value: Option<Value>,
    pub span: Range<usize>,
}

/// What was read of a manifest.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scan {
    pub entries: Vec<Entry>,
}

impl Scan {
    /// Reads `text`.
    pub fn of(text: &str) -> Self {
        let mut scanner = Scanner {
            text,
            bytes: text.as_bytes(),
            at: 0,
            entries: Vec::new(),
        };
        scanner.run();
        Self {
            entries: scanner.entries,
        }
    }
}

struct Scanner<'a> {
    text: &'a str,
    bytes: &'a [u8],
    at: usize,
    entries: Vec<Entry>,
}

impl Scanner<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn is_bare(byte: u8) -> bool {
        byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
    }

    /// Moves past the character at the caret, so that a byte that is not understood is skipped whole.
    fn skip_char(&mut self) {
        self.at += self.text[self.at..]
            .chars()
            .next()
            .map_or(1, char::len_utf8);
    }

    fn skip_blanks(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r')) {
            self.at += 1;
        }
    }

    fn skip_line(&mut self) {
        while self.peek().is_some_and(|byte| byte != b'\n') {
            self.at += 1;
        }
    }

    fn skip_trivia(&mut self) {
        loop {
            self.skip_blanks();
            match self.peek() {
                Some(b'\n') => self.at += 1,
                Some(b'#') => self.skip_line(),
                _ => return,
            }
        }
    }

    /// Whether a table header begins at the caret, at the start of a line: it ends an unfinished `{` or `[`, which must not take the
    /// rest of the file.
    fn header_ahead(&self) -> bool {
        if self.peek() != Some(b'[') || (self.at > 0 && self.bytes[self.at - 1] != b'\n') {
            return false;
        }
        let line = self.text[self.at..]
            .lines()
            .next()
            .unwrap_or_default();
        let inner = line.trim_start_matches('[').trim_start();
        let named = inner
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'"' | b'\''));
        named
            && line
                .trim_end()
                .trim_end_matches(|c: char| c != ']' && c != '#')
                .contains(']')
    }

    /// A quoted string at the caret: its text and where the text is, and where the whole of it is. The caret is left after it.
    fn string(&mut self) -> (String, Range<usize>, Range<usize>) {
        let quote = self.bytes[self.at];
        let start = self.at;
        let triple = self.bytes[self.at..].starts_with(&[quote; 3]);
        let width = if triple { 3 } else { 1 };
        self.at += width;
        let inner_start = self.at;
        let mut value = String::new();
        let mut closed = None;
        while self.at < self.bytes.len() {
            if self.bytes[self.at..].starts_with(&[quote; 3][..width]) {
                closed = Some(self.at);
                self.at += width;
                break;
            }
            let byte = self.bytes[self.at];
            if !triple && byte == b'\n' {
                break;
            }
            if quote == b'"' && byte == b'\\' && self.at + 1 < self.bytes.len() {
                self.at += 1;
                let escaped = self.text[self.at..]
                    .chars()
                    .next()
                    .expect("a byte follows the backslash");
                value.push(match escaped {
                    'b' => '\u{8}',
                    't' => '\t',
                    'n' => '\n',
                    'f' => '\u{c}',
                    'r' => '\r',
                    other => other,
                });
                self.at += escaped.len_utf8();
                continue;
            }
            let ch = self.text[self.at..]
                .chars()
                .next()
                .expect("the caret is inside the text");
            value.push(ch);
            self.at += ch.len_utf8();
        }
        let inner_end = closed.unwrap_or(self.at);
        (value, inner_start..inner_end, start..self.at)
    }

    /// A dotted key at the caret; empty where there is none.
    fn key_path(&mut self) -> Vec<Segment> {
        let mut segments = Vec::new();
        loop {
            self.skip_blanks();
            match self.peek() {
                Some(b'"' | b'\'') => {
                    let (name, inner, _) = self.string();
                    segments.push(Segment {
                        name,
                        span: Some(inner),
                    });
                }
                Some(byte) if Self::is_bare(byte) => {
                    let start = self.at;
                    while self.peek().is_some_and(Self::is_bare) {
                        self.at += 1;
                    }
                    segments.push(Segment {
                        name: self.text[start..self.at].to_owned(),
                        span: Some(start..self.at),
                    });
                }
                _ => return segments,
            }
            self.skip_blanks();
            if self.peek() == Some(b'.') {
                self.at += 1;
            } else {
                return segments;
            }
        }
    }

    /// One value. `base` is the path of the key it belongs to, so that the keys of an inline table become entries of their own.
    fn value(&mut self, base: &[Segment]) -> Value {
        self.skip_blanks();
        let start = self.at;
        match self.peek() {
            Some(b'"' | b'\'') => {
                let (text, _, span) = self.string();
                Value {
                    kind: Kind::String,
                    text,
                    span,
                }
            }
            Some(b'[') => {
                self.at += 1;
                let mut items = Vec::new();
                let mut item_base = base.to_vec();
                item_base.push(Segment::element());
                loop {
                    self.skip_trivia();
                    if self.at >= self.bytes.len() {
                        break;
                    }
                    match self.peek() {
                        Some(b']') => {
                            self.at += 1;
                            break;
                        }
                        _ if self.header_ahead() => break,
                        Some(b',') => self.at += 1,
                        _ => {
                            let before = self.at;
                            items.push(self.value(&item_base));
                            if self.at == before {
                                self.skip_char();
                            }
                        }
                    }
                }
                Value {
                    kind: Kind::Array(items),
                    text: String::new(),
                    span: start..self.at,
                }
            }
            Some(b'{') => {
                self.at += 1;
                loop {
                    self.skip_trivia();
                    match self.peek() {
                        None => break,
                        Some(b'}') => {
                            self.at += 1;
                            break;
                        }
                        Some(b'[') => break,
                        Some(b',') => self.at += 1,
                        _ => {
                            let before = self.at;
                            let keys = self.key_path();
                            self.skip_blanks();
                            if !keys.is_empty() && self.peek() == Some(b'=') {
                                self.at += 1;
                                let mut path = base.to_vec();
                                path.extend(keys.iter().cloned());
                                let found = self.value(&path);
                                self.entries.push(Entry {
                                    kind: EntryKind::Key,
                                    segments: path,
                                    section_len: base.len(),
                                    span: keys[0].span.as_ref().map_or(start, |s| s.start)
                                        ..found.span.end,
                                    value: Some(found),
                                });
                            }
                            if self.at == before {
                                self.skip_char();
                            }
                        }
                    }
                }
                Value {
                    kind: Kind::Table,
                    text: String::new(),
                    span: start..self.at,
                }
            }
            _ => {
                while self.peek().is_some_and(|byte| {
                    !byte.is_ascii_whitespace() && !matches!(byte, b',' | b']' | b'}' | b'#')
                }) {
                    self.at += 1;
                }
                let word = &self.text[start..self.at];
                Value {
                    kind: classify(word),
                    text: word.to_owned(),
                    span: start..self.at,
                }
            }
        }
    }

    fn run(&mut self) {
        let mut section: Vec<Segment> = Vec::new();
        loop {
            self.skip_trivia();
            if self.at >= self.bytes.len() {
                return;
            }
            let before = self.at;
            if self.peek() == Some(b'[') {
                let header_start = self.at;
                let double = self.bytes.get(self.at + 1) == Some(&b'[');
                self.at += if double { 2 } else { 1 };
                self.skip_blanks();
                let mut segments = self.key_path();
                self.skip_blanks();
                while self.peek() == Some(b']') {
                    self.at += 1;
                }
                if double {
                    segments.push(Segment::element());
                }
                section = segments.clone();
                self.entries.push(Entry {
                    kind: EntryKind::Header,
                    segments,
                    section_len: 0,
                    value: None,
                    span: header_start..self.at,
                });
            } else {
                let keys = self.key_path();
                self.skip_blanks();
                if !keys.is_empty() && self.peek() == Some(b'=') {
                    self.at += 1;
                    let mut path = section.clone();
                    path.extend(keys.iter().cloned());
                    let found = self.value(&path);
                    self.entries.push(Entry {
                        kind: EntryKind::Key,
                        segments: path,
                        section_len: section.len(),
                        span: keys[0]
                            .span
                            .as_ref()
                            .map_or(before, |s| s.start)
                            ..found.span.end,
                        value: Some(found),
                    });
                }
            }
            if self.at == before {
                self.skip_char();
            }
            self.skip_blanks();
            if self.peek() == Some(b'#') {
                self.skip_line();
            }
        }
    }
}

fn classify(word: &str) -> Kind {
    let digits = |text: &str, radix: u32| {
        !text.is_empty()
            && text
                .chars()
                .all(|c| c == '_' || c.is_digit(radix))
            && text.chars().any(|c| c.is_digit(radix))
    };
    let unsigned = word.trim_start_matches(['+', '-']);
    if word == "true" || word == "false" {
        Kind::Boolean
    } else if digits(unsigned, 10)
        || ["0x", "0o", "0b"].iter().any(|prefix| {
            let radix = match *prefix {
                "0x" => 16,
                "0o" => 8,
                _ => 2,
            };
            word.strip_prefix(prefix)
                .is_some_and(|rest| digits(rest, radix))
        })
    {
        Kind::Integer
    } else if word.len() >= 10
        && word.as_bytes()[..4]
            .iter()
            .all(u8::is_ascii_digit)
        && word.as_bytes()[4] == b'-'
    {
        Kind::Date
    } else if matches!(unsigned, "inf" | "nan")
        || word.parse::<f64>().is_ok()
            && unsigned
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_digit)
    {
        Kind::Float
    } else {
        Kind::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(scan: &Scan) -> Vec<String> {
        scan.entries
            .iter()
            .map(|entry| {
                entry
                    .segments
                    .iter()
                    .map(|segment| segment.name.as_str())
                    .collect::<Vec<_>>()
                    .join(".")
            })
            .collect()
    }

    fn key<'a>(scan: &'a Scan, path: &str) -> &'a Entry {
        let at = paths(scan)
            .iter()
            .position(|found| found == path)
            .unwrap_or_else(|| panic!("{path} is read"));
        &scan.entries[at]
    }

    #[test]
    fn nothing_is_read_from_empty_text_and_comments() {
        assert!(Scan::of("").entries.is_empty());
        assert!(
            Scan::of("# only\n\n  # comments\n")
                .entries
                .is_empty()
        );
    }

    #[test]
    fn headers_and_keys_are_read_with_their_paths_and_places() {
        let text = "[package]\nname = \"app\"\nversion = \"0.1.0\" # v\n\n[dependencies]\nserde = { version = \"1\", features = [\"derive\"] }\n";
        let scan = Scan::of(text);
        assert_eq!(
            paths(&scan),
            [
                "package",
                "package.name",
                "package.version",
                "dependencies",
                "dependencies.serde.version",
                "dependencies.serde.features",
                "dependencies.serde",
            ]
        );
        let name = key(&scan, "package.name");
        assert_eq!(name.section_len, 1);
        assert_eq!(
            &text[name.segments[1].span.clone().expect("a place")],
            "name"
        );
        let value = name.value.as_ref().expect("a value");
        assert_eq!(
            (value.kind.clone(), value.text.as_str()),
            (Kind::String, "app")
        );
        assert_eq!(&text[value.span.clone()], "\"app\"");
        let serde = key(&scan, "dependencies.serde.version");
        assert_eq!(
            serde.section_len, 2,
            "the inline table is the table of the key"
        );
        assert_eq!(key(&scan, "package").kind, EntryKind::Header);
    }

    #[test]
    fn an_array_of_tables_has_an_element_segment_and_dotted_keys_are_split() {
        let scan = Scan::of("[[bin]]\nname = \"a\"\n[profile.release]\nlto.mode = true\n");
        assert_eq!(
            paths(&scan),
            [
                "bin.[]",
                "bin.[].name",
                "profile.release",
                "profile.release.lto.mode"
            ]
        );
        assert_eq!(key(&scan, "bin.[]").segments[1].span, None);
        assert_eq!(
            key(&scan, "profile.release.lto.mode")
                .value
                .as_ref()
                .expect("a value")
                .kind,
            Kind::Boolean
        );
    }

    #[test]
    fn the_kinds_of_values_are_told_apart() {
        let scan = Scan::of(
            "a = 1\nb = 1_000\nc = 1.5\nd = true\ne = 2024-01-02\nf = [1, \"x\"]\ng = {}\nh = 0xff\ni = nope\nj = 'lit'\nk = -3\n",
        );
        let kind = |path: &str| {
            key(&scan, path)
                .value
                .as_ref()
                .expect("a value")
                .kind
                .clone()
        };
        assert_eq!(kind("a"), Kind::Integer);
        assert_eq!(kind("b"), Kind::Integer);
        assert_eq!(kind("c"), Kind::Float);
        assert_eq!(kind("d"), Kind::Boolean);
        assert_eq!(kind("e"), Kind::Date);
        assert_eq!(kind("g"), Kind::Table);
        assert_eq!(kind("h"), Kind::Integer);
        assert_eq!(kind("i"), Kind::Unknown);
        assert_eq!(kind("j"), Kind::String);
        assert_eq!(kind("k"), Kind::Integer);
        match kind("f") {
            Kind::Array(items) => assert_eq!(
                items
                    .iter()
                    .map(|item| item.kind.clone())
                    .collect::<Vec<_>>(),
                [Kind::Integer, Kind::String]
            ),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn crlf_and_multi_byte_text_keep_their_places() {
        let text =
            "[package]\r\nname = \"caf\u{e9}\"\r\n[dependencies]\r\n\u{e9}x = 1\r\nok = 2\r\n";
        let scan = Scan::of(text);
        let name = key(&scan, "package.name")
            .value
            .as_ref()
            .expect("a value");
        assert_eq!(name.text, "caf\u{e9}");
        assert_eq!(&text[name.span.clone()], "\"caf\u{e9}\"");
        assert_eq!(&text[key(&scan, "dependencies.ok").span.clone()], "ok = 2");
    }

    #[test]
    fn half_typed_text_gives_what_can_be_read_and_never_panics() {
        for text in [
            "[",
            "[[",
            "[package",
            "name =",
            "name = \"unclosed",
            "a = [1, 2",
            "a = { b = 1,",
            "\u{e9}",
            "= 1",
            "a = \"\u{e9}\\",
            "x = [\n[package]\nname = \"a\"\n",
            "a = {\n[package]\n",
        ] {
            let _ = Scan::of(text);
        }
        let scan =
            Scan::of("[dependencies]\nserde = { version = \"1\",\n[package]\nname = \"a\"\n");
        assert!(
            paths(&scan).contains(&"package.name".to_owned()),
            "a header ends an unfinished table"
        );
        let scan = Scan::of("x = [\n  1,\n[package]\nname = \"a\"\n");
        assert!(
            paths(&scan).contains(&"package.name".to_owned()),
            "a header ends an unfinished array"
        );
    }

    #[test]
    fn a_long_line_is_read() {
        let text = format!("a = \"{}\"\nb = 1\n", "x".repeat(200_000));
        assert_eq!(paths(&Scan::of(&text)), ["a", "b"]);
    }
}
