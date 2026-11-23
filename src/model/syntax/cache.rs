//! What the highlighter found for a stretch of the text, kept between edits.
//!
//! Asking tree-sitter for the captures of a stretch and cutting them into runs costs the length of the
//! stretch, and an editor that shows a short file asks for the whole of it on every keystroke. So the
//! runs are kept: an edit moves the ones after it, and only the lines it touched are asked for again.

use std::ops::Range;

use aimer::input::input::StyledRange;

/// The highlight of the bytes `covered`, as the runs a query of them gives: in order, not overlapping,
/// and each as long as it can be, so two neighbours never have the same style.
pub(super) struct HighlightCache {
    covered: Range<usize>,
    ranges: Vec<StyledRange>,
}

impl HighlightCache {
    /// A cache of `covered`, holding `ranges`, which are the runs of it and lie inside it.
    pub fn new(covered: Range<usize>, ranges: Vec<StyledRange>) -> Self {
        Self { covered, ranges }
    }

    /// The bytes the cache holds the highlight of.
    pub fn covered(&self) -> Range<usize> {
        self.covered.clone()
    }

    /// The runs that have a byte in `range`, whole: a run is not cut where `range` cuts it.
    pub fn ranges_in(&self, range: Range<usize>) -> Vec<StyledRange> {
        if range.is_empty() {
            return Vec::new();
        }
        let first = self
            .ranges
            .partition_point(|run| run.range.end <= range.start);
        let last = self
            .ranges
            .partition_point(|run| run.range.start < range.end)
            .max(first);
        self.ranges[first..last].to_vec()
    }

    /// Moves the cache over an edit that replaced the bytes `old` by `new_len` bytes. What the edit
    /// replaced is left without runs, for [`Self::replace`] to fill.
    ///
    /// The cost is the runs from the edit to the end of what is covered, each moved by an addition:
    /// no run is made again but those the edit cuts.
    pub fn edited(&mut self, old: Range<usize>, new_len: usize) {
        let new_end = old.start + new_len;
        // For a byte that is at or after the end of what was replaced.
        let moved = |byte: usize| byte - old.end + new_end;

        // What is covered. A side that lies inside the edit is drawn in to the edge of the new bytes,
        // which are not known yet, so that nothing is claimed of them.
        let start = if self.covered.start <= old.start {
            self.covered.start
        } else if self.covered.start >= old.end {
            moved(self.covered.start)
        } else {
            new_end
        };
        let end = if self.covered.end <= old.start {
            self.covered.end
        } else if self.covered.end >= old.end {
            moved(self.covered.end)
        } else {
            old.start
        };
        self.covered = start..end.max(start);

        // The runs the edit overlaps keep their parts before it and after it.
        let first = self
            .ranges
            .partition_point(|run| run.range.end <= old.start);
        let after = self
            .ranges
            .partition_point(|run| run.range.start < old.end)
            .max(first);
        let mut kept = Vec::new();
        for run in &self.ranges[first..after] {
            if run.range.start < old.start {
                kept.push(StyledRange::new(
                    run.range.start..old.start,
                    run.style.clone(),
                ));
            }
            if run.range.end > old.end {
                kept.push(StyledRange::new(
                    new_end..moved(run.range.end),
                    run.style.clone(),
                ));
            }
        }
        let moved_from = first + kept.len();
        self.ranges.splice(first..after, kept);
        for run in &mut self.ranges[moved_from..] {
            run.range = moved(run.range.start)..moved(run.range.end);
        }
    }

    /// Makes `fresh`, the runs of `region` and inside it, what the cache holds of `region`, which lies
    /// inside what it covers.
    pub fn replace(&mut self, region: Range<usize>, fresh: Vec<StyledRange>) {
        let first = self
            .ranges
            .partition_point(|run| run.range.end <= region.start);
        let last = self
            .ranges
            .partition_point(|run| run.range.start < region.end)
            .max(first);
        let mut middle = Vec::with_capacity(fresh.len() + 2);
        // The parts of the runs at the two edges that lie outside the region stay.
        let mut left = 0;
        if first < last {
            let run = &self.ranges[first];
            if run.range.start < region.start {
                middle.push(StyledRange::new(
                    run.range.start..region.start,
                    run.style.clone(),
                ));
                left = 1;
            }
        }
        let fresh_len = fresh.len();
        middle.extend(fresh);
        if first < last {
            let run = &self.ranges[last - 1];
            if run.range.end > region.end {
                middle.push(StyledRange::new(
                    region.end..run.range.end,
                    run.style.clone(),
                ));
            }
        }
        let added = middle.len();
        self.ranges.splice(first..last, middle);
        // Where runs meet that were not made together, two of one style are one run: after the new
        // ones, between them and the part kept of a cut run, and before them. From the end, so that
        // a run taken in does not move the places still to be looked at.
        for seam in [first + added, first + left + fresh_len, first + left, first] {
            self.join(seam);
        }
    }

    /// Makes the cache cover `region` too, which touches or overlaps what it covers, with `fresh`, the
    /// runs of the part of `region` it did not cover. Only one side: a region that reaches past both
    /// ends is added in two calls.
    pub fn extend(&mut self, region: Range<usize>, fresh: Vec<StyledRange>) {
        let before = region.start < self.covered.start && region.end <= self.covered.end;
        self.covered = self.covered.start.min(region.start)..self.covered.end.max(region.end);
        let added = fresh.len();
        if before {
            self.ranges.splice(0..0, fresh);
            self.join(added);
        } else {
            let at = self.ranges.len();
            self.ranges.extend(fresh);
            self.join(at);
        }
    }

    /// Makes the runs on the two sides of the index `at` one if they touch and have the same style.
    fn join(&mut self, at: usize) {
        if at == 0 || at >= self.ranges.len() {
            return;
        }
        let (previous, next) = (&self.ranges[at - 1], &self.ranges[at]);
        if previous.range.end == next.range.start && previous.style == next.style {
            let end = next.range.end;
            self.ranges[at - 1].range.end = end;
            self.ranges.remove(at);
        }
    }
}

/// The parts of `ranges`, which are runs, that lie inside `region`: a run that crosses its edge is cut.
pub(super) fn clip(ranges: Vec<StyledRange>, region: &Range<usize>) -> Vec<StyledRange> {
    ranges
        .into_iter()
        .filter_map(|mut run| {
            run.range = run.range.start.max(region.start)..run.range.end.min(region.end);
            (run.range.start < run.range.end).then_some(run)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aimer::input::input::SpanStyle;

    /// A style that is told from the others by its size.
    fn style(size: u32) -> SpanStyle {
        SpanStyle::new().font_size(size)
    }

    fn run(range: Range<usize>, size: u32) -> StyledRange {
        StyledRange::new(range, style(size))
    }

    fn cache() -> HighlightCache {
        // 0..10 a, 10..20 b, 25..30 a (20..25 has no style), 30..40 c.
        HighlightCache::new(
            0..40,
            vec![
                run(0..10, 1),
                run(10..20, 2),
                run(25..30, 1),
                run(30..40, 3),
            ],
        )
    }

    fn held(cache: &HighlightCache) -> Vec<(Range<usize>, u32)> {
        cache
            .ranges_in(0..usize::MAX)
            .into_iter()
            .map(|range| {
                let size = (0..8)
                    .find(|size| range.style == style(*size))
                    .expect("one of the styles of the tests");
                (range.range, size)
            })
            .collect()
    }

    #[test]
    fn the_runs_of_a_range_are_those_with_a_byte_in_it_and_are_not_cut() {
        let cache = cache();
        let ranges = |range: Range<usize>| {
            cache
                .ranges_in(range)
                .into_iter()
                .map(|run| run.range)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ranges(12..14),
            vec![10..20],
            "a run that holds the range is given whole"
        );
        assert_eq!(ranges(15..27), vec![10..20, 25..30]);
        assert_eq!(
            ranges(20..25),
            Vec::<Range<usize>>::new(),
            "a gap between runs has none"
        );
        assert_eq!(
            ranges(10..10),
            Vec::<Range<usize>>::new(),
            "nor does a range of no bytes"
        );
        assert_eq!(ranges(0..40).len(), 4);
        assert_eq!(cache.covered(), 0..40);
    }

    #[test]
    fn an_edit_moves_the_runs_after_it_and_leaves_what_it_replaced_without_runs() {
        let mut cache = cache();
        // Three bytes replace the four in 12..16.
        cache.edited(12..16, 3);
        assert_eq!(
            held(&cache),
            vec![
                (0..10, 1),
                (10..12, 2),
                (15..19, 2),
                (24..29, 1),
                (29..39, 3)
            ],
            "the run through the edit is cut at it, and the rest is moved by one byte"
        );
        assert_eq!(cache.covered(), 0..39);

        let mut cache = self::cache();
        cache.edited(22..22, 5);
        assert_eq!(
            held(&cache),
            vec![(0..10, 1), (10..20, 2), (30..35, 1), (35..45, 3)],
            "an insertion in a gap pushes what is after it"
        );
        assert_eq!(cache.covered(), 0..45);

        let mut cache = self::cache();
        cache.edited(0..40, 0);
        assert!(held(&cache).is_empty(), "everything replaced");
        assert_eq!(cache.covered(), 0..0);
    }

    #[test]
    fn an_edit_past_what_is_covered_changes_nothing_and_one_before_it_moves_it() {
        let mut cache = HighlightCache::new(10..30, vec![run(10..20, 1), run(20..30, 2)]);
        cache.edited(40..41, 9);
        assert_eq!(
            (cache.covered(), held(&cache)),
            (10..30, vec![(10..20, 1), (20..30, 2)])
        );
        cache.edited(0..2, 5);
        assert_eq!(
            (cache.covered(), held(&cache)),
            (13..33, vec![(13..23, 1), (23..33, 2)])
        );
    }

    #[test]
    fn what_replaces_a_region_is_joined_to_its_neighbours_of_the_same_style() {
        let mut cache = cache();
        // 10..20 is b; make 12..18 a, which leaves b on both sides, and then make it b again.
        cache.replace(12..18, vec![run(12..18, 1)]);
        assert_eq!(
            held(&cache),
            vec![
                (0..10, 1),
                (10..12, 2),
                (12..18, 1),
                (18..20, 2),
                (25..30, 1),
                (30..40, 3)
            ]
        );
        cache.replace(12..18, vec![run(12..18, 2)]);
        assert_eq!(
            held(&cache),
            vec![(0..10, 1), (10..20, 2), (25..30, 1), (30..40, 3)],
            "one run again, as a query of it would give"
        );
        // A region that starts a run: the one before it, of the same style, takes it in.
        cache.replace(20..25, vec![run(20..25, 2)]);
        assert_eq!(
            held(&cache),
            vec![(0..10, 1), (10..25, 2), (25..30, 1), (30..40, 3)]
        );
        cache.replace(20..30, vec![run(20..30, 1)]);
        assert_eq!(
            held(&cache),
            vec![(0..10, 1), (10..20, 2), (20..30, 1), (30..40, 3)]
        );
        cache.replace(0..40, vec![]);
        assert!(
            held(&cache).is_empty(),
            "a region with nothing in it empties what was there"
        );
    }

    #[test]
    fn the_part_of_a_run_that_a_region_cuts_is_joined_to_the_fresh_run_of_its_style() {
        // A run of one style through the start of the region, and fresh runs of that style at it.
        let mut cache =
            HighlightCache::new(0..40, vec![run(0..10, 1), run(10..30, 2), run(30..40, 3)]);
        cache.replace(20..35, vec![run(20..27, 2), run(27..35, 4)]);
        assert_eq!(
            held(&cache),
            vec![(0..10, 1), (10..27, 2), (27..35, 4), (35..40, 3)],
            "left of the region, the old run goes on into the fresh one"
        );
        // The same at the end of the region.
        let mut cache =
            HighlightCache::new(0..40, vec![run(0..10, 1), run(10..30, 2), run(30..40, 3)]);
        cache.replace(5..20, vec![run(5..8, 4), run(8..20, 2)]);
        assert_eq!(
            held(&cache),
            vec![(0..5, 1), (5..8, 4), (8..30, 2), (30..40, 3)],
            "right of the region, the fresh run goes on into the old one"
        );
        // A run that holds the whole region, replaced by the same style.
        let mut cache = HighlightCache::new(0..40, vec![run(0..40, 2)]);
        cache.replace(10..20, vec![run(10..20, 2)]);
        assert_eq!(held(&cache), vec![(0..40, 2)]);
    }

    #[test]
    fn a_region_is_replaced_across_several_runs_and_the_edges_of_the_ones_it_cuts_stay() {
        let mut cache = cache();
        cache.replace(5..32, vec![run(5..32, 4)]);
        assert_eq!(held(&cache), vec![(0..5, 1), (5..32, 4), (32..40, 3)]);
    }

    #[test]
    fn the_cache_grows_at_either_end_and_joins_what_it_adds() {
        let mut cache = HighlightCache::new(10..20, vec![run(10..20, 1)]);
        cache.extend(20..30, vec![run(20..25, 1), run(25..30, 2)]);
        assert_eq!(
            (cache.covered(), held(&cache)),
            (10..30, vec![(10..25, 1), (25..30, 2)])
        );
        cache.extend(0..10, vec![run(2..10, 1)]);
        assert_eq!(
            (cache.covered(), held(&cache)),
            (0..30, vec![(2..25, 1), (25..30, 2)])
        );
        cache.extend(5..35, vec![run(30..35, 2)]);
        assert_eq!(
            (cache.covered(), held(&cache)),
            (0..35, vec![(2..25, 1), (25..35, 2)]),
            "a region that overlaps what is covered adds only what is not"
        );
    }

    #[test]
    fn clipping_cuts_the_runs_at_the_edges_of_a_region() {
        let clipped = clip(
            vec![run(0..10, 1), run(10..20, 2), run(25..30, 1)],
            &(5..27),
        );
        assert_eq!(
            clipped
                .into_iter()
                .map(|run| run.range)
                .collect::<Vec<_>>(),
            vec![5..10, 10..20, 25..27]
        );
        assert!(
            clip(vec![run(0..4, 1)], &(4..9)).is_empty(),
            "a run that ends where the region starts is outside it"
        );
    }
}
