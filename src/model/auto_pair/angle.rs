//! The angle brackets of a generic: `Vec<u8>`, `collect::<Vec<_>>()`, `fn f<T>()`, `impl<T>`, `<T as Trait>::Item`.
//!
//! A `<` is also less-than and shift, and closing one that is one corrupts what is typed, so it is closed only where
//! it cannot be either: after a name of a type, a declaration or a turbofish, or where an expression cannot go on.

use aimer::input::input::TextSnapshot;

use super::AutoPair;
use super::literal::{Literal, Within};

pub(super) struct Angle;

impl Angle {
    /// The longest name that is read back from the caret: a longer one is not a type.
    const MAX_NAME: usize = 64;
    /// The words after which a name is declared, and so is followed by its generics.
    const DECLARATIONS: [&'static [u8]; 6] =
        [b"fn", b"struct", b"enum", b"trait", b"type", b"union"];
    /// The words that are followed by generics themselves: `impl<T>`, `for<'a>`.
    const GENERIC_KEYWORDS: [&'static [u8]; 2] = [b"impl", b"for"];
    /// The marks after which a `<` begins a qualified path, `<T as Trait>::Item`: nothing before it could be compared.
    const PATH_AFTER: [u8; 6] = [b'(', b',', b'=', b'&', b'{', b':'];

    /// Whether a `<` typed at `at` begins a generic, and is closed. `after` is the character after the caret.
    pub fn opens(text: &TextSnapshot, at: usize, after: Option<char>) -> bool {
        let Some(line) = Literal::line(text, at) else {
            return false;
        };
        // A string is not code: what is written in it is not a generic.
        if Literal::within(&line) != Within::Code {
            return false;
        }
        let mut gap = 0;
        while matches!(Self::back(&line, line.len(), gap), Some(b' ' | b'\t')) {
            gap += 1;
        }
        let end = line.len() - gap;
        let Some(last) = Self::back(&line, end, 0) else {
            return false;
        };
        if Literal::is_word(last) {
            let name = Self::name_before(&line, end);
            return match name {
                None => false,
                // A word that is followed by blanks is a generic only when it is one that takes them.
                Some(name) if gap > 0 => Self::GENERIC_KEYWORDS.contains(&name),
                Some(name) => {
                    name[0].is_ascii_uppercase()
                        || Self::GENERIC_KEYWORDS.contains(&name)
                        || Self::declared(&line, end - name.len())
                }
            };
        }
        match last {
            // A nested generic is begun in the one that was closed with it: `Vec<<T as Trait>::Item>`.
            b'<' => gap == 0 && after == Some('>'),
            last => Self::PATH_AFTER.contains(&last),
        }
    }

    /// Whether a `>` typed at `at`, where one is, ends a generic that was begun on its line, and is stepped over.
    pub fn steps_over(text: &TextSnapshot, at: usize) -> bool {
        // The `>` of `->` and `=>` is not the end of anything.
        if at > 0 && matches!(text.byte_at(at - 1), Some(b'-' | b'=')) {
            return false;
        }
        let floor = at.saturating_sub(AutoPair::LOOKBEHIND);
        let (mut depth, mut cursor) = (0usize, at);
        while cursor > floor {
            cursor -= 1;
            match text.byte_at(cursor) {
                Some(b'\n') | None => return false,
                Some(b'>')
                    if !matches!(
                        cursor
                            .checked_sub(1)
                            .and_then(|before| text.byte_at(before)),
                        Some(b'-' | b'=')
                    ) =>
                {
                    depth += 1
                }
                Some(b'<') if depth == 0 => return true,
                Some(b'<') => depth -= 1,
                _ => {}
            }
        }
        false
    }

    /// The byte `back` bytes before `end`.
    fn back(line: &[u8], end: usize, back: usize) -> Option<u8> {
        end.checked_sub(back + 1).map(|at| line[at])
    }

    /// The name that ends at `end`, if it is shorter than [`Self::MAX_NAME`].
    fn name_before(line: &[u8], end: usize) -> Option<&[u8]> {
        let mut start = end;
        while start > 0 && Literal::is_word(line[start - 1]) {
            start -= 1;
            if end - start > Self::MAX_NAME {
                return None;
            }
        }
        Some(&line[start..end])
    }

    /// Whether the name that starts at `start` is declared: the word before it, past the blanks, is one that declares.
    fn declared(line: &[u8], start: usize) -> bool {
        let mut end = start;
        while end > 0 && matches!(line[end - 1], b' ' | b'\t') {
            end -= 1;
        }
        end < start
            && Self::name_before(line, end).is_some_and(|word| Self::DECLARATIONS.contains(&word))
    }
}
