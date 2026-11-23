//! Which of the items fit the word typed, and in what order: the ones that begin with it first, then the ones
//! that begin with it but for the case of the letters, then the ones that have its letters in order. Within each
//! the order is the server's, which knows better than the length of a word which of two is the likelier.

use crate::services::lsp::Item;

/// How well an item fits the word, the best being the least: 0 for the word itself, 1 for what begins with it, 2 for
/// what begins with it but for the case of the letters, and 3 for what has its letters in order, each letter beginning
/// a word of the name unless it follows the letter before it. None for one that does not fit, which for the last is
/// all that does not begin with the word unless `fuzzy` lets it have the letters.
fn tier(filter: &str, word: &str, fuzzy: bool) -> Option<u8> {
    if filter == word {
        return Some(0);
    }
    if filter.starts_with(word) {
        return Some(1);
    }
    if filter.is_ascii() && word.is_ascii() {
        let (filter, word) = (filter.as_bytes(), word.as_bytes());
        if filter.len() >= word.len() && filter[..word.len()].eq_ignore_ascii_case(word) {
            return Some(2);
        }
        return (fuzzy && at_word_starts(filter, word, 0, false)).then_some(3);
    }
    // Text that is not ascii is rarer, and is compared by the letters it has.
    let lower = |text: &str| -> Vec<char> {
        text.chars()
            .flat_map(char::to_lowercase)
            .collect()
    };
    let (lowered_filter, lowered_word) = (lower(filter), lower(word));
    if lowered_filter.starts_with(&lowered_word) {
        return Some(2);
    }
    let starts: Vec<bool> = {
        let mut before = None;
        filter
            .chars()
            .map(|c| {
                let start = starts_a_word(before, c);
                before = Some(c);
                start
            })
            .collect()
    };
    // One letter of the filter for each letter of it, which is what lowering keeps for the letters of text that is
    // not special; any other is not compared by its starts.
    if !fuzzy || starts.len() != lowered_filter.len() {
        return None;
    }
    at_word_starts_of(&lowered_filter, &starts, &lowered_word, 0, false).then_some(3)
}

/// Whether the letter `c` begins a word of a name: it is the first, or follows a separator, or is a capital after a
/// letter that is not, or a digit after a letter.
fn starts_a_word(before: Option<char>, c: char) -> bool {
    match before {
        None => true,
        Some(before) => {
            !before.is_alphanumeric()
                || (c.is_uppercase() && !before.is_uppercase())
                || (c.is_numeric() && !before.is_numeric())
        }
    }
}

/// Whether the letters of `word` are in `filter`, from `from` on, each at the start of a word or, where `follows`, the very next.
fn at_word_starts(filter: &[u8], word: &[u8], from: usize, follows: bool) -> bool {
    let Some((&want, rest)) = word.split_first() else {
        return true;
    };
    (from..filter.len()).any(|at| {
        filter[at].eq_ignore_ascii_case(&want)
            && (follows && at == from
                || starts_a_word(
                    at.checked_sub(1)
                        .map(|before| char::from(filter[before])),
                    char::from(filter[at]),
                ))
            && at_word_starts(filter, rest, at + 1, true)
    })
}

/// [`at_word_starts`] for text that is not ascii, with the starts worked out.
fn at_word_starts_of(
    filter: &[char],
    starts: &[bool],
    word: &[char],
    from: usize,
    follows: bool,
) -> bool {
    let Some((&want, rest)) = word.split_first() else {
        return true;
    };
    (from..filter.len()).any(|at| {
        filter[at] == want
            && (follows && at == from || starts[at])
            && at_word_starts_of(filter, starts, rest, at + 1, true)
    })
}

/// The numbers of the items of `items` that fit `word`, best first. `within` limits the search to those
/// numbers: when the word has only grown, what fitted it before is all that can fit it now. `fuzzy` lets an item that
/// has the letters of the word in order fit it.
pub fn rank(items: &[Item], within: Option<&[u32]>, word: &str, fuzzy: bool) -> Vec<u32> {
    let mut fitting: Vec<(u8, u32)> = match within {
        Some(numbers) => numbers
            .iter()
            .filter_map(|&at| tier(&items[at as usize].filter, word, fuzzy).map(|tier| (tier, at)))
            .collect(),
        None => items
            .iter()
            .enumerate()
            .filter_map(|(at, item)| tier(&item.filter, word, fuzzy).map(|tier| (tier, at as u32)))
            .collect(),
    };
    // Stable, so that items of the same tier and sort text stay in the order they were given.
    fitting.sort_by(|a, b| {
        a.0.cmp(&b.0).then_with(|| {
            items[a.1 as usize]
                .sort
                .cmp(&items[b.1 as usize].sort)
        })
    });
    fitting.into_iter().map(|(_, at)| at).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::lsp::{Edit, ItemKind};

    fn item(filter: &str, sort: &str) -> Item {
        Item {
            label: filter.to_owned(),
            kind: ItemKind::Function,
            detail: None,
            label_detail: None,
            sort: sort.to_owned(),
            filter: filter.to_owned(),
            edit: Edit {
                range: None,
                text: filter.to_owned(),
                extra: vec![],
            },
            resolve: None,
            deprecated: false,
        }
    }

    fn labels(items: &[Item], found: &[u32]) -> Vec<String> {
        found
            .iter()
            .map(|&at| items[at as usize].label.clone())
            .collect()
    }

    fn ranked(items: &[Item], word: &str) -> Vec<String> {
        ranked_with(items, word, true)
    }

    fn ranked_with(items: &[Item], word: &str, fuzzy: bool) -> Vec<String> {
        labels(items, &rank(items, None, word, fuzzy))
    }

    #[test]
    fn the_word_itself_comes_first_then_what_begins_with_it_then_what_has_its_letters_at_the_starts_of_words()
     {
        let items = [
            item("HashMap", "a"),
            item("letm", "a"),
            item("let", "z"),
            item("Letter", "b"),
            item("l_e_t", "c"),
            item("AnyElement", "d"),
        ];
        // The word as it is, though the server likes the others better; then case that is the same; then case that is not; then
        // the letters of the word, each beginning a word of the name. AnyElement has an l, an e and a t but none begins a word.
        assert_eq!(ranked(&items, "let"), ["let", "letm", "Letter", "l_e_t"]);
    }

    #[test]
    fn items_of_the_same_tier_are_in_the_order_of_their_sort_text() {
        let items = [item("beta", "2"), item("bravo", "1"), item("bingo", "3")];
        assert_eq!(ranked(&items, "b"), ["bravo", "beta", "bingo"]);
        // Where the sort text is the same, the order given.
        let same = [item("b2", "x"), item("b1", "x"), item("b3", "x")];
        assert_eq!(ranked(&same, "b"), ["b2", "b1", "b3"]);
    }

    #[test]
    fn the_empty_word_fits_everything_in_the_order_of_the_sort_text() {
        let items = [item("c", "3"), item("a", "1"), item("b", "2")];
        assert_eq!(ranked(&items, ""), ["a", "b", "c"]);
    }

    #[test]
    fn letters_find_the_words_of_a_name_that_they_begin() {
        let items = [
            item("HashMap", "a"),
            item("HashSet", "b"),
            item("hasher", "c"),
            item("split_off", "d"),
            item("SplitOff", "e"),
            item("escape_debug", "f"),
        ];
        assert_eq!(ranked(&items, "hm"), ["HashMap"], "the capitals of a name");
        assert_eq!(ranked(&items, "hset"), ["HashSet"]);
        assert_eq!(
            ranked(&items, "hs"),
            ["HashSet"],
            "H of Hash and S of Set, and not the s in the middle of the others"
        );
        assert_eq!(
            ranked(&items, "so"),
            ["split_off", "SplitOff"],
            "each word of a name with underscores or capitals"
        );
        assert_eq!(ranked(&items, "ed"), ["escape_debug"]);
        assert!(ranked(&items, "zz").is_empty());
        assert!(
            ranked(&items, "hashmapp").is_empty(),
            "a word that is longer than the item is not in it"
        );
    }

    #[test]
    fn a_letter_that_follows_one_that_matched_may_continue_the_word_and_is_not_taken_from_the_middle_of_one()
     {
        let items = [
            item("HashMap", "a"),
            item("push_str", "b"),
            item("split_inclusive", "c"),
            item("strip_suffix", "d"),
        ];
        assert_eq!(
            ranked(&items, "hashm"),
            ["HashMap"],
            "hash, then the word that begins at M"
        );
        assert_eq!(ranked(&items, "pus"), ["push_str"]);
        assert_eq!(
            ranked(&items, "pu"),
            ["push_str"],
            "the u of split_inclusive and the p of strip_suffix are not at the start of a word"
        );
        assert_eq!(ranked(&items, "ps"), ["push_str"], "p, then the s of str");
    }

    #[test]
    fn a_name_with_a_path_or_a_signature_after_it_is_found_by_its_name_and_its_path_words() {
        let items = [item("Err()", "a"), item("std::collections::HashMap", "b")];
        assert_eq!(ranked(&items, "er"), ["Err()"]);
        assert_eq!(ranked(&items, "hm"), ["std::collections::HashMap"]);
        assert_eq!(
            ranked(&items, "stc"),
            ["std::collections::HashMap"],
            "the start of std and of collections"
        );
    }

    #[test]
    fn without_fuzzy_only_what_begins_with_the_word_fits_it() {
        let items = [
            item("static", "1"),
            item("SomeThing", "2"),
            item("const", "3"),
        ];
        assert_eq!(ranked_with(&items, "st", false), ["static"]);
        assert_eq!(
            ranked_with(&items, "st", true),
            ["static", "SomeThing"],
            "S and T of the two words; const has no s that begins a word"
        );
    }

    #[test]
    fn a_word_that_has_grown_is_looked_for_only_among_what_fitted_it() {
        let items = [
            item("push", "1"),
            item("pull", "2"),
            item("pop", "3"),
            item("peek", "4"),
        ];
        let first = rank(&items, None, "p", true);
        assert_eq!(first.len(), 4);
        let narrowed = rank(&items, Some(&first), "pu", true);
        assert_eq!(labels(&items, &narrowed), ["push", "pull"]);
        assert_eq!(
            rank(&items, Some(&narrowed), "pus", true),
            rank(&items, None, "pus", true),
            "the same as looking at all of them"
        );
        assert!(rank(&items, Some(&[]), "p", true).is_empty());
    }

    #[test]
    fn letters_that_are_not_ascii_are_compared_without_their_case_too() {
        let items = [
            item("\u{c9}t\u{e9}", "a"),
            item("ete", "b"),
            item("sous_\u{e9}t\u{e9}", "c"),
        ];
        assert_eq!(
            ranked(&items, "\u{e9}t"),
            ["\u{c9}t\u{e9}", "sous_\u{e9}t\u{e9}"],
            "the second has the word at the start of its last word"
        );
        assert_eq!(
            ranked(&items, "\u{c9}"),
            ["\u{c9}t\u{e9}", "sous_\u{e9}t\u{e9}"]
        );
        assert_eq!(
            ranked(&items, "s\u{e9}"),
            ["sous_\u{e9}t\u{e9}"],
            "the accent at the start of a word after an underscore"
        );
    }
}
