//! Where a byte of the text is in the terms of the protocol, and the other way.
//!
//! A position is a line and a character on it, both counted from zero. What a character is depends on
//! the encoding the client and the server agreed on: a byte of UTF-8, or a unit of UTF-16 (which a
//! character outside the basic plane is two of). A line ends at a line feed, as the editor's lines do,
//! and the line break is not on the line: a position cannot be after the end of the line's text, and
//! one that asks for it is at the end.
//!
//! A byte that is not on a character boundary, a position in the middle of a character, and a line or a
//! character that is past the end are not errors: they are brought to the nearest place at or before
//! them, as the protocol says a server does for a position that is out of range.

use std::ops::Range;

use aimer::input::input::TextSnapshot;

/// What a character of a position is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// A byte of the text, as it is held.
    Utf8,
    /// A unit of UTF-16, which every server understands.
    Utf16,
}

/// A line and a character on it, both from zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

/// Where the byte `byte` of `text` is. A byte past the end is at the end, and one inside a character
/// is at the start of the character.
pub fn position_of(text: &TextSnapshot, byte: usize, encoding: Encoding) -> Position {
    let mut byte = byte.min(text.len());
    while byte > 0 && !text.is_char_boundary(byte) {
        byte -= 1;
    }
    let line = text.byte_to_line(byte);
    let content = line_text(text, line);
    // A byte inside the line break is at the end of the line's text.
    let end = byte.min(content.end);
    let character = match encoding {
        Encoding::Utf8 => end - content.start,
        Encoding::Utf16 => {
            let before = text.slice(content.start..end);
            if before.is_ascii() {
                before.len()
            } else {
                before.chars().map(char::len_utf16).sum()
            }
        }
    };
    Position {
        line: line as u32,
        character: character as u32,
    }
}

/// The byte of `text` that `position` is at.
pub fn byte_of(text: &TextSnapshot, position: Position, encoding: Encoding) -> usize {
    let line = position.line as usize;
    if line >= text.line_count() {
        return text.len();
    }
    let content = line_text(text, line);
    let character = position.character as usize;
    match encoding {
        Encoding::Utf8 => {
            let mut at = content
                .start
                .saturating_add(character)
                .min(content.end);
            while at > content.start && !text.is_char_boundary(at) {
                at -= 1;
            }
            at
        }
        Encoding::Utf16 => {
            let line = text.slice(content.clone());
            let mut units = 0;
            for (offset, character_at) in line.char_indices() {
                // A position that asks for the middle of a character is at its start.
                if units >= character {
                    return content.start + offset;
                }
                units += character_at.len_utf16();
                if units > character {
                    return content.start + offset;
                }
            }
            content.end
        }
    }
}

/// The bytes of `line` of `text` without its line break.
fn line_text(text: &TextSnapshot, line: usize) -> Range<usize> {
    let range = text.line_range(line);
    let mut end = range.end;
    if end > range.start && text.byte_at(end - 1) == Some(b'\n') {
        end -= 1;
        if end > range.start && text.byte_at(end - 1) == Some(b'\r') {
            end -= 1;
        }
    }
    range.start..end
}

#[cfg(test)]
mod tests {
    use super::*;

    const UTF8: Encoding = Encoding::Utf8;
    const UTF16: Encoding = Encoding::Utf16;

    fn snapshot(text: &str) -> TextSnapshot {
        TextSnapshot::from_text(text)
    }

    fn at(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    /// The position of the byte where `needle` starts.
    fn position_at(text: &str, needle: &str, encoding: Encoding) -> Position {
        position_of(
            &snapshot(text),
            text.find(needle)
                .expect("the needle is in the text"),
            encoding,
        )
    }

    #[test]
    fn a_byte_of_plain_text_is_its_line_and_the_characters_before_it_on_the_line() {
        let text = "fn main() {\n    let x = 1;\n}\n";
        for encoding in [UTF8, UTF16] {
            assert_eq!(position_at(text, "fn", encoding), at(0, 0));
            assert_eq!(position_at(text, "main", encoding), at(0, 3));
            assert_eq!(position_at(text, "let", encoding), at(1, 4));
            assert_eq!(position_at(text, "}\n", encoding), at(2, 0));
        }
    }

    #[test]
    fn empty_text_and_the_end_of_a_text_are_positions_too() {
        for encoding in [UTF8, UTF16] {
            assert_eq!(position_of(&snapshot(""), 0, encoding), at(0, 0));
            assert_eq!(byte_of(&snapshot(""), at(0, 0), encoding), 0);
            assert_eq!(
                byte_of(&snapshot(""), at(5, 9), encoding),
                0,
                "past the end of nothing"
            );
            // One line that has no line break at its end.
            assert_eq!(position_of(&snapshot("abc"), 3, encoding), at(0, 3));
            assert_eq!(byte_of(&snapshot("abc"), at(0, 3), encoding), 3);
            // A line break at the end makes one more, empty, line.
            assert_eq!(position_of(&snapshot("abc\n"), 4, encoding), at(1, 0));
            assert_eq!(byte_of(&snapshot("abc\n"), at(1, 0), encoding), 4);
            // A byte past the end is at the end.
            assert_eq!(position_of(&snapshot("abc"), 99, encoding), at(0, 3));
        }
    }

    #[test]
    fn a_character_outside_the_basic_plane_is_two_units_of_utf16_and_four_bytes() {
        // é is two bytes and one unit, 你 three bytes and one unit, 😀 four bytes and two units.
        let text = "a\u{e9}\u{4f60}\u{1f600}z";
        assert_eq!(position_at(text, "z", UTF8), at(0, 1 + 2 + 3 + 4));
        assert_eq!(position_at(text, "z", UTF16), at(0, 1 + 1 + 1 + 2));
        assert_eq!(position_at(text, "\u{1f600}", UTF16), at(0, 3));
        let snapshot = snapshot(text);
        assert_eq!(
            byte_of(&snapshot, at(0, 1 + 2 + 3 + 4), UTF8),
            text.find('z').unwrap()
        );
        assert_eq!(
            byte_of(&snapshot, at(0, 1 + 1 + 1 + 2), UTF16),
            text.find('z').unwrap()
        );
    }

    #[test]
    fn a_combining_mark_counts_as_the_character_it_is_and_not_as_part_of_its_base() {
        // "e" and U+0301 are one grapheme and two characters; a position counts the characters.
        let text = "e\u{301}x";
        assert_eq!(position_at(text, "x", UTF16), at(0, 2));
        assert_eq!(position_at(text, "x", UTF8), at(0, 3));
        assert_eq!(
            byte_of(&snapshot(text), at(0, 1), UTF16),
            1,
            "between the base and its mark"
        );
    }

    #[test]
    fn a_line_break_is_not_on_its_line_so_crlf_ends_a_line_before_the_return() {
        let text = "ab\r\ncd\r\n";
        for encoding in [UTF8, UTF16] {
            assert_eq!(
                position_at(text, "cd", encoding),
                at(1, 0),
                "the line after a CRLF"
            );
            // The end of the first line is before the return, and so is anything inside the break.
            assert_eq!(position_of(&snapshot(text), 2, encoding), at(0, 2));
            assert_eq!(
                position_of(&snapshot(text), 3, encoding),
                at(0, 2),
                "between the return and the feed"
            );
            assert_eq!(byte_of(&snapshot(text), at(0, 2), encoding), 2);
            assert_eq!(
                byte_of(&snapshot(text), at(0, 3), encoding),
                2,
                "past the end of the line is its end"
            );
            assert_eq!(byte_of(&snapshot(text), at(0, 100), encoding), 2);
        }
    }

    #[test]
    fn a_line_or_a_character_past_the_end_is_the_end_and_the_middle_of_a_character_is_its_start() {
        let text = "ab\n\u{1f600}c\nlast";
        let snapshot = snapshot(text);
        for encoding in [UTF8, UTF16] {
            assert_eq!(
                byte_of(&snapshot, at(0, 50), encoding),
                2,
                "past the end of a line"
            );
            assert_eq!(
                byte_of(&snapshot, at(2, 50), encoding),
                text.len(),
                "past the end of the last line"
            );
            assert_eq!(
                byte_of(&snapshot, at(40, 0), encoding),
                text.len(),
                "past the last line"
            );
        }
        let emoji = text.find('\u{1f600}').unwrap();
        // Inside the emoji: its second unit of UTF-16, and its third byte.
        assert_eq!(byte_of(&snapshot, at(1, 1), UTF16), emoji);
        assert_eq!(byte_of(&snapshot, at(1, 2), UTF8), emoji);
        assert_eq!(
            position_of(&snapshot, emoji + 2, UTF8),
            at(1, 0),
            "a byte inside a character"
        );
        assert_eq!(position_of(&snapshot, emoji + 2, UTF16), at(1, 0));
    }

    #[test]
    fn a_very_long_line_is_measured_to_its_end_and_back() {
        let line = "x\u{e9}".repeat(60_000) + "\u{1f600}";
        let text = format!("short\n{line}\nafter");
        let snapshot = snapshot(&text);
        let end_of_line = text.find("\nafter").unwrap();
        assert_eq!(
            position_of(&snapshot, end_of_line, UTF8),
            at(1, line.len() as u32)
        );
        assert_eq!(
            position_of(&snapshot, end_of_line, UTF16),
            at(1, (60_000 * 2 + 2) as u32)
        );
        assert_eq!(
            byte_of(&snapshot, at(1, (60_000 * 2 + 2) as u32), UTF16),
            end_of_line
        );
        assert_eq!(position_at(&text, "after", UTF16), at(2, 0));
    }

    #[test]
    fn a_text_held_in_many_chunks_is_measured_the_same() {
        let text: String = (0..3000)
            .map(|n| format!("line {n} \u{e9}\u{1f600}\n"))
            .collect();
        let snapshot = snapshot(&text);
        assert!(snapshot.chunk_count() > 2, "the text is in several chunks");
        for encoding in [UTF8, UTF16] {
            for byte in (0..text.len())
                .step_by(997)
                .filter(|byte| text.is_char_boundary(*byte))
            {
                let position = position_of(&snapshot, byte, encoding);
                assert_eq!(
                    byte_of(&snapshot, position, encoding),
                    byte,
                    "{encoding:?} at byte {byte}"
                );
            }
        }
    }

    #[test]
    fn every_boundary_of_a_varied_text_goes_to_a_position_and_back() {
        let text = "fn \u{e9}t\u{e9}() {}\r\n\r\n\t// \u{4f60}\u{597d} \u{1f600}\u{1f600}\nlet x = \"e\u{301}\";\n\nend";
        let snapshot = snapshot(text);
        for encoding in [UTF8, UTF16] {
            for byte in (0..=text.len()).filter(|byte| text.is_char_boundary(*byte)) {
                let position = position_of(&snapshot, byte, encoding);
                let back = byte_of(&snapshot, position, encoding);
                // A byte inside a line break comes back as the end of its line's text.
                let in_break = text[..byte].ends_with('\r') && text[byte..].starts_with('\n');
                if in_break {
                    assert_eq!(
                        back,
                        byte - 1,
                        "{encoding:?} between a return and a feed at {byte}"
                    );
                } else {
                    assert_eq!(back, byte, "{encoding:?} at byte {byte}: {position:?}");
                }
            }
        }
    }
}
