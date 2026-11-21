use super::*;

fn matched(query: &str, name: &str) -> Match {
    Match::of(query, name).unwrap_or_else(|| panic!("{query:?} should match {name:?}"))
}

fn text_of(name: &str, m: &Match) -> Vec<String> {
    m.ranges.iter().map(|r| name[r.clone()].to_owned()).collect()
}

#[test]
fn the_empty_query_matches_everything_and_marks_nothing() {
    let m = matched("", "anything");
    assert!(m.ranges.is_empty());
}

#[test]
fn a_name_without_the_letters_in_order_does_not_match() {
    assert!(Match::of("xyz", "request").is_none());
    assert!(Match::of("tseuqer", "request").is_none());
    assert!(Match::of("requests", "request").is_none());
    assert!(Match::of("a", "").is_none());
}

#[test]
fn letters_match_without_regard_to_case_and_the_range_is_in_the_name() {
    let m = matched("REQ", "RequestBuilder");
    assert_eq!(text_of("RequestBuilder", &m), ["Req"]);
}

#[test]
fn the_marks_are_the_tightest_place_the_letters_fit() {
    // `req` is found at the start of `request_ext`, not scattered from a `r` early in `fry_request`.
    let m = matched("req", "fry_request");
    assert_eq!(text_of("fry_request", &m), ["req"]);
}

#[test]
fn letters_that_touch_are_one_mark() {
    let m = matched("quest", "RequestBuilder");
    assert_eq!(text_of("RequestBuilder", &m), ["quest"]);
}

#[test]
fn letters_that_do_not_touch_are_marked_apart() {
    let m = matched("rb", "RequestBuilder");
    assert_eq!(text_of("RequestBuilder", &m), ["R", "B"]);
}

#[test]
fn a_name_that_begins_with_the_query_is_better_than_one_that_has_it_inside() {
    let prefix = matched("req", "request").score;
    let inside = matched("req", "a_request").score;
    assert!(prefix > inside, "{prefix} should beat {inside}");
}

#[test]
fn a_whole_name_is_better_than_a_longer_one_that_begins_with_it() {
    assert!(matched("request", "request").score > matched("request", "request_ext").score);
}

#[test]
fn letters_that_begin_words_beat_letters_that_are_scattered() {
    let words = matched("rb", "request_builder").score;
    let scattered = matched("rb", "arbitrary").score;
    assert!(words > scattered, "{words} should beat {scattered}");
}

#[test]
fn the_case_of_the_letters_breaks_a_tie() {
    assert!(matched("Req", "Request").score > matched("Req", "request").score);
}

#[test]
fn a_camel_case_hump_begins_a_word() {
    let m = matched("rb", "RequestBuilder");
    let plain = matched("rb", "requestbuilder");
    assert!(m.score > plain.score);
}

#[test]
fn text_that_is_not_ascii_is_matched_by_its_letters_and_marked_in_bytes() {
    let name = "naïve_café";
    let m = matched("caf", name);
    assert_eq!(text_of(name, &m), ["caf"]);
    let m = matched("ÉÉ", "été");
    assert_eq!(text_of("été", &m), ["é", "é"]);
}

#[test]
fn a_very_long_name_is_matched_without_cost_that_grows_with_the_square() {
    let name = "a".repeat(100_000) + "b";
    assert!(Match::of("ab", &name).is_some());
    assert!(Match::of("ba", &name).is_none());
}
