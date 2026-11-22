//! Closing what is opened as it is typed: brackets and quotes.
//!
//! Typing `(` makes `()` with the caret between; typing the `)` that is already there steps over it; typing a
//! bracket or a quote over a selection wraps it; and Backspace in an empty pair takes both. The rules read only
//! the few bytes around the caret, and the line before it as far as [`AutoPair::LOOKBEHIND`], so a keystroke
//! costs the same in a file of any size. Where the rules cannot tell what is meant, the character is typed as
//! it is, which does no harm: an editor that closes too little is a nuisance and one that closes too much
//! corrupts what is typed.
//!
//! What is done is one edit through the controller, which one undo takes back.

use std::ops::Range;

use aimer::input::input::{TextEditingController, TextSnapshot};

use self::angle::Angle;
use self::literal::{Literal, Start, Within};

mod angle;
mod literal;

/// What typing a character does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Typed {
    /// Types `open` and `close` with the caret between them.
    Pair { open: char, close: char },
    /// Puts `open` before the selection and `close` after it, and keeps it selected.
    Wrap { open: char, close: char },
    /// The character that closes is already there: the caret moves over its `len` bytes, and nothing is typed.
    Over { len: usize },
    /// Types the quote that opens a raw string and the quote that closes it with its `hashes` marks, with the caret between
    /// the quotes: `r#"` becomes `r#""#`.
    Raw { hashes: usize },
}

/// The rules of closing brackets and quotes as they are typed.
pub struct AutoPair;

impl AutoPair {
    /// How many bytes of the line before the caret are read to tell whether a quote is inside a string. A longer
    /// line is not read: it is minified or generated, and a quote in it is typed as it is.
    pub const LOOKBEHIND: usize = 1024;
    /// What may follow the caret for an opener to be closed: the end of the text, a line break, a space, or a mark
    /// that ends or separates things. An opener before a word opens what is there, and is not closed.
    const CLOSES_BEFORE: &'static str = ";:.,=}])> \t\r\n";
    /// The marks before which a single quote begins a character, and not a lifetime or a label.
    const CHARACTER_AFTER: &'static str = "=([";

    /// What typing `typed` does with `selection` selected in `text`, or `None` where it is for the field to type.
    pub fn typed(text: &TextSnapshot, selection: Range<usize>, typed: &str) -> Option<Typed> {
        let mut chars = typed.chars();
        let ch = chars.next()?;
        // A phrase, or a dead key that made two characters, is not a bracket.
        if chars.next().is_some() {
            return None;
        }
        if !selection.is_empty() {
            return Self::closer_of(ch).map(|close| Typed::Wrap { open: ch, close });
        }
        let at = selection.start;
        let after = Self::char_after(text, at);
        match ch {
            ')' | ']' | '}' => (after == Some(ch)).then_some(Typed::Over { len: 1 }),
            '<' => (Self::closes_before(after) && Angle::opens(text, at, after)).then_some(
                Typed::Pair {
                    open: '<',
                    close: '>',
                },
            ),
            '>' => (after == Some('>') && Angle::steps_over(text, at))
                .then_some(Typed::Over { len: 1 }),
            '#' => Self::over_mark(text, at, after),
            '(' | '[' | '{' => {
                let close = Self::closer_of(ch).expect("an opener has a closer");
                Self::closes_before(after).then_some(Typed::Pair { open: ch, close })
            }
            '"' => Self::double_quote(text, at, after),
            '\'' => Self::single_quote(text, at, after),
            _ => None,
        }
    }

    /// The bytes that Backspace takes with the pair it is in: the opener before the caret and the closer after it,
    /// when they are a pair with nothing between. `None` for anything else, which is the field's to delete.
    pub fn backspace(text: &TextSnapshot, selection: Range<usize>) -> Option<Range<usize>> {
        if !selection.is_empty() {
            return None;
        }
        let at = selection.start;
        let before = Self::char_before(text, at)?;
        let after = Self::char_after(text, at)?;
        let closer = Self::closer_of(before).or((before == '<').then_some('>'));
        (closer == Some(after)).then(|| at - before.len_utf8()..at + after.len_utf8())
    }

    /// Does what typing `typed` does in the text of `controller`, as one edit. Whether it did: `false` leaves the
    /// character to be typed by the field.
    pub fn apply_typed(controller: &TextEditingController, typed: &str) -> bool {
        let selection = controller.value().selection().range();
        let Some(action) = Self::typed(&controller.snapshot(), selection.clone(), typed) else {
            return false;
        };
        match action {
            Typed::Pair { open, close } | Typed::Wrap { open, close } => {
                let (mut open_text, mut close_text) = ([0; 4], [0; 4]);
                let (open_text, close_text): (&str, &str) = (
                    open.encode_utf8(&mut open_text),
                    close.encode_utf8(&mut close_text),
                );
                // The two marks are two ranges of one edit, so one undo takes both: where nothing is selected they
                // are at the same place, and touch.
                let edited = controller.replace_ranges(
                    &[
                        (selection.start..selection.start, open_text),
                        (selection.end..selection.end, close_text),
                    ],
                    0,
                );
                if edited && !selection.is_empty() {
                    // Still selected, so that typing another opener wraps the wrapped.
                    let shift = open.len_utf8();
                    controller.select_bytes(selection.start + shift, selection.end + shift);
                }
                edited
            }
            Typed::Raw { hashes } => {
                let closer = format!("\"{}", "#".repeat(hashes));
                controller.replace_ranges(
                    &[
                        (selection.start..selection.start, "\""),
                        (selection.start..selection.start, closer.as_str()),
                    ],
                    0,
                )
            }
            Typed::Over { len } => {
                controller.select_bytes(selection.start + len, selection.start + len)
            }
        }
    }

    /// Does what Backspace does in an empty pair, as one edit. Whether it did: `false` leaves the key to the field.
    pub fn apply_backspace(controller: &TextEditingController) -> bool {
        let selection = controller.value().selection().range();
        match Self::backspace(&controller.snapshot(), selection) {
            Some(pair) => controller.replace_range(pair, ""),
            None => false,
        }
    }

    /// The character that closes what `open` opens: a bracket's own, and a quote's the same.
    fn closer_of(open: char) -> Option<char> {
        match open {
            '(' => Some(')'),
            '[' => Some(']'),
            '{' => Some('}'),
            '"' | '\'' => Some(open),
            _ => None,
        }
    }

    /// Whether what follows the caret, `after`, lets an opener be closed.
    fn closes_before(after: Option<char>) -> bool {
        after.is_none_or(|after| Self::CLOSES_BEFORE.contains(after))
    }

    /// A `"` at `at`: the end of a string when it is inside one, which is stepped over if it is the one that is there;
    /// otherwise the start of one, closed where a word is not being written. A raw string is closed with its own marks.
    fn double_quote(text: &TextSnapshot, at: usize, after: Option<char>) -> Option<Typed> {
        let line = Literal::line(text, at)?;
        match Literal::within(&line) {
            Within::Text => return (after == Some('"')).then_some(Typed::Over { len: 1 }),
            Within::Raw {
                hashes,
                closing: None,
            } => {
                // The quote that ends it, and the marks it ends with.
                let ends = after == Some('"')
                    && (0..hashes).all(|mark| text.byte_at(at + 1 + mark) == Some(b'#'));
                return ends.then_some(Typed::Over { len: 1 });
            }
            Within::Raw { .. } => return None,
            Within::Code => {}
        }
        let word = match Literal::start(&line) {
            Start::Raw { hashes } => {
                return Self::closes_before(after).then_some(Typed::Raw { hashes });
            }
            Start::Prefixed => false,
            // A quote before it is an empty string, or the start of a longer one; a `#` is the marks of what is not a raw
            // string.
            Start::Other => Self::char_before(text, at).is_some_and(|before| {
                before.is_alphanumeric() || matches!(before, '_' | '"' | '\'' | '\\' | '#')
            }),
        };
        (!word && Self::closes_before(after)).then_some(Typed::Pair {
            open: '"',
            close: '"',
        })
    }

    /// A `#` at `at`: one of the marks that end a raw string, when the caret is in the end of one that has begun, and the
    /// mark is there.
    fn over_mark(text: &TextSnapshot, at: usize, after: Option<char>) -> Option<Typed> {
        if after != Some('#') {
            return None;
        }
        match Literal::within(&Literal::line(text, at)?) {
            Within::Raw {
                hashes,
                closing: Some(marks),
            } if marks < hashes => Some(Typed::Over { len: 1 }),
            _ => None,
        }
    }

    /// A `'` at `at`: the end of a character when one is begun, which is stepped over if it is the one that is there;
    /// otherwise the start of one, closed only where it cannot be a lifetime or a label.
    fn single_quote(text: &TextSnapshot, at: usize, after: Option<char>) -> Option<Typed> {
        if after == Some('\'') && Self::ends_character(text, at) {
            return Some(Typed::Over { len: 1 });
        }
        let begins = Self::byte_before_blank(text, at)
            .is_some_and(|before| Self::CHARACTER_AFTER.contains(before))
            || Self::is_byte_prefix(text, at);
        (begins && Self::closes_before(after)).then_some(Typed::Pair {
            open: '\'',
            close: '\'',
        })
    }

    /// Whether the `b` before `at` is the prefix of a byte character, `b'x'`, and not the end of a longer word.
    fn is_byte_prefix(text: &TextSnapshot, at: usize) -> bool {
        at > 0
            && text.byte_at(at - 1) == Some(b'b')
            && (at < 2
                || text
                    .byte_at(at - 2)
                    .is_some_and(|before| !Literal::is_word(before)))
    }

    /// Whether what is before `at` is a character literal that lacks only its closing quote: `'`, `'x`, or `'\x`.
    fn ends_character(text: &TextSnapshot, at: usize) -> bool {
        let Some(last) = Self::char_before(text, at) else {
            return false;
        };
        if last == '\'' {
            return true;
        }
        let before_last = at - last.len_utf8();
        match Self::char_before(text, before_last) {
            Some('\'') => true,
            Some('\\') => Self::char_before(text, before_last - 1) == Some('\''),
            _ => false,
        }
    }

    /// Whether `at` is inside a string of its line: after a `"` that nothing closed. `None` where that cannot be told
    /// because the line is longer than [`Self::LOOKBEHIND`] before `at`.
    fn inside_string(text: &TextSnapshot, at: usize) -> Option<bool> {
        let floor = at.saturating_sub(Self::LOOKBEHIND);
        let mut start = at;
        while start > floor && text.byte_at(start - 1)? != b'\n' {
            start -= 1;
        }
        if start == floor && floor > 0 {
            return None;
        }
        let line: Vec<u8> = (start..at)
            .map(|byte| text.byte_at(byte))
            .collect::<Option<_>>()?;
        let (mut inside, mut escaped, mut at) = (false, false, 0);
        while at < line.len() {
            let byte = line[at];
            if escaped {
                escaped = false;
            } else if !inside && line[at..].starts_with(b"'\"'") {
                // The character `'"'` is not the start of a string.
                at += 2;
            } else if byte == b'\\' && inside {
                escaped = true;
            } else if byte == b'"' {
                inside = !inside;
            }
            at += 1;
        }
        Some(inside)
    }

    /// The first byte before `at` that is not a space or a tab, on the line of `at`, if it is ASCII.
    fn byte_before_blank(text: &TextSnapshot, at: usize) -> Option<char> {
        let floor = at.saturating_sub(Self::LOOKBEHIND);
        let mut at = at;
        while at > floor {
            at -= 1;
            match text.byte_at(at)? {
                b' ' | b'\t' => {}
                byte if byte.is_ascii() && byte != b'\n' => return Some(byte as char),
                _ => return None,
            }
        }
        None
    }

    /// The character that ends at `at`.
    pub(crate) fn char_before(text: &TextSnapshot, at: usize) -> Option<char> {
        let mut start = at.checked_sub(1)?;
        // A continuation byte belongs to the character that starts before it, at most three bytes back.
        while start > 0 && at - start < 4 && text.byte_at(start)? & 0xC0 == 0x80 {
            start -= 1;
        }
        Self::decode(text, start..at)
    }

    /// The character that starts at `at`.
    pub(crate) fn char_after(text: &TextSnapshot, at: usize) -> Option<char> {
        let len = match text.byte_at(at)? {
            0x00..=0x7F => 1,
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            _ => return None,
        };
        Self::decode(text, at..at + len)
    }

    /// The character the bytes in `range`, at most four, are.
    fn decode(text: &TextSnapshot, range: Range<usize>) -> Option<char> {
        let mut bytes = [0; 4];
        let len = range.len();
        for (slot, at) in bytes.iter_mut().zip(range) {
            *slot = text.byte_at(at)?;
        }
        std::str::from_utf8(bytes.get(..len)?)
            .ok()?
            .chars()
            .next()
    }
}

#[cfg(test)]
mod tests;
