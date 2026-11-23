//! Strings as the rules of pairing read them: whether the caret is inside one, of which kind, and what a `"` typed
//! at the caret would begin.
//!
//! A string is `"..."` with a backslash that escapes, or raw: `r"..."`, `r#"..."#`, with as many marks as were used
//! to open it, and no escapes at all. A `b`, a `c` or a `br` before either says what it holds, and is not a word.
//! Everything here reads the line before the caret, and nothing else.

use aimer::input::input::TextSnapshot;

use super::AutoPair;

/// The caret's place in its line, as far as strings go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Within {
    /// Not in a string.
    Code,
    /// In a string whose quote is escaped by a backslash.
    Text,
    /// In a raw string, which ends with a quote and `hashes` marks. `closing` is how many of the marks are before the
    /// caret, when it is after a quote that began to end it.
    Raw {
        hashes: usize,
        closing: Option<usize>,
    },
}

/// What a `"` typed at the caret begins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Start {
    /// A raw string that is opened with `hashes` marks, which are at least one.
    Raw { hashes: usize },
    /// A string that has a prefix: `r"`, `b"`, `c"`.
    Prefixed,
    /// Neither: the quote is judged by what is before it.
    Other,
}

pub(super) struct Literal;

impl Literal {
    /// The line of `at` before it, or `None` where it is longer than [`AutoPair::LOOKBEHIND`], so that it is not known
    /// whether a string is open in it.
    pub fn line(text: &TextSnapshot, at: usize) -> Option<Vec<u8>> {
        let floor = at.saturating_sub(AutoPair::LOOKBEHIND);
        let mut start = at;
        while start > floor && text.byte_at(start - 1)? != b'\n' {
            start -= 1;
        }
        if start == floor && floor > 0 {
            return None;
        }
        (start..at)
            .map(|byte| text.byte_at(byte))
            .collect()
    }

    /// Where the end of `line` is: in code or in which kind of string.
    pub fn within(line: &[u8]) -> Within {
        let (mut state, mut at) = (Within::Code, 0);
        while at < line.len() {
            let byte = line[at];
            match state {
                Within::Code => {
                    if line[at..].starts_with(b"'\"'") {
                        // The character `'"'` is not the start of a string.
                        at += 3;
                        continue;
                    }
                    if byte == b'"' {
                        state = match Self::raw_marks(&line[..at]) {
                            Some(hashes) => Within::Raw {
                                hashes,
                                closing: None,
                            },
                            None => Within::Text,
                        };
                    }
                }
                Within::Text => {
                    if byte == b'\\' {
                        at += 2;
                        continue;
                    }
                    if byte == b'"' {
                        state = Within::Code;
                    }
                }
                Within::Raw { hashes, .. } => {
                    if byte == b'"' {
                        let marks = line[at + 1..]
                            .iter()
                            .take(hashes)
                            .take_while(|&&byte| byte == b'#')
                            .count();
                        if marks == hashes {
                            state = Within::Code;
                            at += hashes;
                        } else if at + 1 + marks == line.len() {
                            // The line ends with a quote that has fewer marks than it needs: it has begun to end.
                            state = Within::Raw {
                                hashes,
                                closing: Some(marks),
                            };
                        }
                    }
                }
            }
            at += 1;
        }
        state
    }

    /// What a `"` typed after `line` begins.
    pub fn start(line: &[u8]) -> Start {
        let hashes = line
            .iter()
            .rev()
            .take_while(|&&byte| byte == b'#')
            .count();
        let end = line.len() - hashes;
        match Self::prefix_before(line, end) {
            Some((_, true)) if hashes > 0 => Start::Raw { hashes },
            Some(_) if hashes == 0 => Start::Prefixed,
            _ => Start::Other,
        }
    }

    /// The marks of the raw string that a `"` after `line` opens, if it is one.
    fn raw_marks(line: &[u8]) -> Option<usize> {
        let hashes = line
            .iter()
            .rev()
            .take_while(|&&byte| byte == b'#')
            .count();
        match Self::prefix_before(line, line.len() - hashes) {
            Some((_, true)) => Some(hashes),
            _ => None,
        }
    }

    /// The prefix of a string that ends at `end`: its length, and whether it is a raw string's. `r`, `br` and `cr`
    /// are raw; `b` and `c` are not. A prefix is not one where it ends a longer word.
    fn prefix_before(line: &[u8], end: usize) -> Option<(usize, bool)> {
        let back = |back: usize| end.checked_sub(back + 1).map(|at| line[at]);
        let (len, raw) = match back(0)? {
            b'r' => match back(1) {
                Some(b'b' | b'c') => (2, true),
                _ => (1, true),
            },
            b'b' | b'c' => (1, false),
            _ => return None,
        };
        back(len)
            .is_none_or(|before| !Self::is_word(before))
            .then_some((len, raw))
    }

    /// Whether `byte` can be in a name. A byte of a character that is more than one is, as the name may be in any
    /// script.
    pub fn is_word(byte: u8) -> bool {
        byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
    }
}
