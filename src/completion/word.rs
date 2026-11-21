//! The word being typed at the caret: where it starts and what it is, read from the few bytes before the
//! caret so that it costs the same on a file of any size.

use aimer::input::input::TextSnapshot;

/// How far back from the caret the word is looked for. A longer run of word characters is not one that is
/// completed, and reading further would make a keystroke cost the length of the line.
const LOOKBACK: usize = 32;

/// The word before the caret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Word {
    /// The byte where the word starts, which is where the caret is when there is none.
    pub start: usize,
    /// The letters, digits and underscores before the caret, none if the caret is not after one.
    pub text: String,
    /// Whether what comes before the word is a `.` or a `::`, after which there is something to complete
    /// before a letter is typed.
    pub triggered: bool,
}

impl Word {
    /// Whether this is a place to offer completions: a word that is begun and is not a number, or the place after
    /// a `.` or a `::`.
    pub fn is_completable(&self) -> bool {
        if self.text.is_empty() {
            return self.triggered;
        }
        !self
            .text
            .starts_with(|c: char| c.is_ascii_digit())
    }
}

/// The word before `caret`, a byte offset into `text`. None where nothing is completed: in a comment.
///
/// Only the part of the line the lookback reached is known; a comment that began before it is not seen,
/// which at worst offers completions in a long comment.
pub fn word_before(text: &TextSnapshot, caret: usize) -> Option<Word> {
    let caret = caret.min(text.len());
    let line = text.line_range(text.byte_to_line(caret));
    let mut from = caret.saturating_sub(LOOKBACK).max(line.start);
    while !text.is_char_boundary(from) {
        from += 1;
    }
    let before = text.slice(from..caret);
    let at = word_start(&before);
    let (head, word) = before.split_at(at);
    if head.contains("//") {
        return None;
    }
    let triggered = head.ends_with("::") || (head.ends_with('.') && !head.ends_with(".."));
    Some(Word {
        start: from + at,
        text: word.to_owned(),
        triggered,
    })
}

/// Where the word at the end of `before` starts: the run of letters, digits and underscores.
fn word_start(before: &str) -> usize {
    before
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
        .last()
        .map_or(before.len(), |(at, _)| at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at_end(text: &str) -> Option<Word> {
        word_before(&TextSnapshot::from_text(text), text.len())
    }

    fn word(text: &str) -> (usize, String, bool) {
        let found = at_end(text).expect("not in a comment");
        (found.start, found.text, found.triggered)
    }

    #[test]
    fn the_word_is_the_run_of_letters_digits_and_underscores_before_the_caret() {
        assert_eq!(word("    pu"), (4, "pu".into(), false));
        assert_eq!(
            word("let x = snake_case_9"),
            (8, "snake_case_9".into(), false)
        );
        assert_eq!(
            word("let x = "),
            (8, String::new(), false),
            "after a space there is no word"
        );
        assert_eq!(word(""), (0, String::new(), false), "the empty text");
    }

    #[test]
    fn the_word_ends_at_the_caret_and_not_at_the_end_of_the_line() {
        let text = TextSnapshot::from_text("fn main() { pub_thing }");
        let found = word_before(&text, "fn main() { pu".len()).expect("pu");
        assert_eq!((found.start, found.text.as_str()), (12, "pu"));
    }

    #[test]
    fn a_dot_or_a_double_colon_before_the_word_is_a_trigger_and_two_dots_are_not() {
        assert!(word("value.").2);
        assert!(word("value.fi").2, "and the word after it");
        assert!(word("std::").2);
        assert!(word("std::col").2);
        assert!(!word("a..").2, "a range is not a member");
        assert!(!word("a:").2, "one colon is not a path");
        assert!(!word("a, ").2);
        assert_eq!(
            word("value."),
            (6, String::new(), true),
            "the word is where the caret is"
        );
    }

    #[test]
    fn what_can_be_completed_is_a_begun_word_that_is_not_a_number_or_the_place_after_a_trigger() {
        let completable = |text: &str| at_end(text).is_some_and(|word| word.is_completable());
        assert!(completable("pu"));
        assert!(completable("_x"));
        assert!(completable("a."), "after a dot");
        assert!(completable("a::"), "after the colons");
        assert!(!completable(""), "nothing is typed");
        assert!(!completable("a "), "a space");
        assert!(!completable("1st"), "a number");
        assert!(
            !completable("x.1st"),
            "a number after a dot is a field of a tuple"
        );
    }

    #[test]
    fn a_comment_has_no_word() {
        assert_eq!(at_end("let a = 1; // pu"), None);
        assert!(
            at_end("let a = 1;\npu").is_some(),
            "the line before is another line"
        );
    }

    #[test]
    fn crlf_multi_byte_text_and_long_lines_are_handled() {
        let found = at_end("let caf\u{e9} = 1;\r\nlet x = ret").expect("ret");
        assert_eq!(
            (found.text.as_str(), found.start),
            ("ret", "let caf\u{e9} = 1;\r\nlet x = ".len())
        );
        // A word after multi-byte text, where the lookback could start inside a character.
        let text = format!("{} pu", "\u{e9}".repeat(40));
        assert_eq!(at_end(&text).expect("pu after accents").start, 81);
        // A very long line costs no more than a short one, and a long word is cut at the lookback.
        let long = format!("{} pu", "a".repeat(100_000));
        assert_eq!(at_end(&long).expect("pu").start, 100_001);
        assert!(
            at_end(&"a".repeat(100_000))
                .expect("a word")
                .text
                .len()
                <= LOOKBACK
        );
    }

    #[test]
    fn a_combining_mark_does_not_end_up_in_the_word() {
        let found = at_end("pu\u{301}").expect("a place");
        assert_eq!(
            found.text, "",
            "pu followed by a mark is not a word that ends at the caret"
        );
    }
}
