//! How well a name fits what was typed: the letters of the query, in order, anywhere in the name.
//!
//! A list of names is matched on every key typed, so a name that does not fit is turned away without a single
//! allocation: one pass over it, which stops where the query is all found. Only a name that fits is walked back to find
//! the tightest place the letters are and what that is worth.

use std::ops::Range;

/// A name that fits a query: how well, and where in the name the letters of the query are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    /// How well the name fits, the best being the greatest. Only meant to be compared with that of another name for the
    /// same query.
    pub score: i32,
    /// The runs of the name that are the letters of the query, as byte ranges in the order of the name, with the runs that
    /// touch joined. Empty for the empty query.
    pub ranges: Vec<Range<usize>>,
}

/// What is typed, made ready to be matched against many names.
#[derive(Clone, Debug, Default)]
pub struct Pattern {
    /// The letters as typed.
    typed: Vec<char>,
    /// The same, lowercased: what they are compared by.
    lowered: Vec<char>,
}

/// The lowercase of a character, which for the few that become more than one is the first of them.
#[inline]
fn lower(c: char) -> char {
    if c.is_ascii() {
        c.to_ascii_lowercase()
    } else {
        c.to_lowercase().next().unwrap_or(c)
    }
}

impl Pattern {
    /// The pattern for `query`. Whitespace is not a letter of a name, so it is left out.
    pub fn new(query: &str) -> Self {
        let typed: Vec<char> = query
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let lowered = typed.iter().copied().map(lower).collect();
        Self { typed, lowered }
    }

    /// Whether the letter `c` was typed.
    pub fn has(&self, c: char) -> bool {
        self.typed.contains(&c)
    }

    /// Whether nothing was typed.
    pub fn is_empty(&self) -> bool {
        self.typed.is_empty()
    }

    // The points a letter of the name is worth, and what is taken off for what lies between and around the letters.
    const LETTER: i32 = 10;
    const SAME_CASE: i32 = 2;
    const STARTS_WORD: i32 = 12;
    const FOLLOWS: i32 = 8;
    const AT_START: i32 = 10;
    const WHOLE: i32 = 50;
    const GAP: i32 = 1;
    const MOST_GAP: i32 = 15;
    const EXTRA: i32 = 1;
    const MOST_EXTRA: i32 = 40;

    /// How `name` fits the pattern, or none when it has not the letters in order.
    pub fn matches(&self, name: &str) -> Option<Match> {
        if self.is_empty() {
            return Some(Match {
                score: 0,
                ranges: Vec::new(),
            });
        }
        // Forward: the end of the first place the letters are found, which is the nearest end there is.
        let mut next = 0;
        let mut end = None;
        for (at, c) in name.char_indices() {
            if lower(c) == self.lowered[next] {
                next += 1;
                if next == self.lowered.len() {
                    end = Some(at + c.len_utf8());
                    break;
                }
            }
        }
        let end = end?;
        // Backward from there: the latest start, which is the tightest place the letters fit.
        let mut found: Vec<usize> = Vec::with_capacity(self.lowered.len());
        let mut want = self.lowered.len();
        for (at, c) in name[..end].char_indices().rev() {
            if lower(c) == self.lowered[want - 1] {
                found.push(at);
                want -= 1;
                if want == 0 {
                    break;
                }
            }
        }
        found.reverse();
        Some(Match {
            score: self.score(name, &found),
            ranges: ranges_of(name, &found),
        })
    }

    /// What the letters found at the byte offsets `found` of `name` are worth.
    fn score(&self, name: &str, found: &[usize]) -> i32 {
        let mut score = 0;
        let mut before: Option<char> = None;
        let mut previous_end = None;
        let mut gap = 0;
        let mut at_of_before = 0;
        for (&at, &typed) in found.iter().zip(&self.typed) {
            let c = name[at..]
                .chars()
                .next()
                .expect("an offset that was found is a character");
            // The character before is the one before the offset, found from the text unless it is the one just matched.
            if previous_end != Some(at) {
                before = name[..at].chars().next_back();
            }
            score += Self::LETTER;
            if c == typed {
                score += Self::SAME_CASE;
            }
            if starts_a_word(before, c) {
                score += Self::STARTS_WORD;
            }
            match previous_end {
                Some(end) if end == at => score += Self::FOLLOWS,
                Some(end) => gap += (at - end).min(Self::MOST_GAP as usize),
                None => at_of_before = at,
            }
            before = Some(c);
            previous_end = Some(at + c.len_utf8());
        }
        score -= (gap as i32 * Self::GAP).min(Self::MOST_GAP);
        if at_of_before == 0 {
            score += Self::AT_START;
        } else {
            // Letters that come late in the name are worth less than those at its start.
            score -= (at_of_before as i32).min(Self::MOST_GAP / 3);
        }
        let extra = name
            .len()
            .saturating_sub(found.len())
            .min(Self::MOST_EXTRA as usize) as i32;
        score -= extra * Self::EXTRA;
        if name.chars().count() == self.typed.len()
            && name
                .chars()
                .map(lower)
                .eq(self.lowered.iter().copied())
        {
            score += Self::WHOLE;
        }
        score
    }
}

/// Whether `c` begins a word of a name: the first character, one after a character that is no letter or digit, and a
/// capital after a lowercase letter or a digit (a camel-case hump).
fn starts_a_word(before: Option<char>, c: char) -> bool {
    match before {
        None => true,
        Some(b) if !b.is_alphanumeric() => true,
        Some(b) => c.is_uppercase() && (b.is_lowercase() || b.is_ascii_digit()),
    }
}

/// The byte ranges of the characters of `name` at the offsets `found`, the ones that touch joined.
fn ranges_of(name: &str, found: &[usize]) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for &at in found {
        let end = at
            + name[at..]
                .chars()
                .next()
                .map_or(0, char::len_utf8);
        match ranges.last_mut() {
            Some(last) if last.end == at => last.end = end,
            _ => ranges.push(at..end),
        }
    }
    ranges
}

impl Match {
    /// How `name` fits `query`, or none where it does not. For a list of names make a [`Pattern`] once.
    #[cfg(test)]
    pub fn of(query: &str, name: &str) -> Option<Self> {
        Pattern::new(query).matches(name)
    }
}

#[cfg(test)]
#[path = "fuzzy_tests.rs"]
mod fuzzy_tests;
