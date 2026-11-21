//! How the line of code of a usage is drawn: in the colors of the syntax, with the name that is used in bold.

use std::ops::Range;

use aimer::SpanStyle;
use aimer::input::input::StyledRange;
use aimer::style::FontWeight;

/// The pieces `text` is drawn in, which cover it from its start to its end, with no gaps: each piece in the style of the syntax
/// that covers it, if one does, and a piece that is inside `mark` in bold too. Pieces that have nothing to say of them are drawn in the
/// style that the text has, so a style with nothing in it is not a gap.
pub fn spans(
    text: &str,
    styled: &[StyledRange],
    mark: Range<usize>,
) -> Vec<(Range<usize>, SpanStyle)> {
    let len = text.len();
    let mut edges = vec![0, len];
    for range in styled
        .iter()
        .map(|styled| &styled.range)
        .chain(std::iter::once(&mark))
    {
        edges.extend([range.start.min(len), range.end.min(len)]);
    }
    edges.retain(|edge| text.is_char_boundary(*edge));
    edges.sort_unstable();
    edges.dedup();
    edges
        .windows(2)
        .map(|edge| edge[0]..edge[1])
        .filter(|piece| !piece.is_empty())
        .map(|piece| {
            // The style that is last of those that cover the piece wins, as it does where the field draws them.
            let mut style = styled
                .iter()
                .rev()
                .find(|styled| styled.range.start <= piece.start && piece.end <= styled.range.end)
                .map_or_else(SpanStyle::new, |styled| styled.style.clone());
            if mark.start <= piece.start && piece.end <= mark.end {
                style.font_weight = Some(FontWeight::Bold);
            }
            (piece, style)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aimer::Color;

    fn colored(range: Range<usize>, color: Color) -> StyledRange {
        StyledRange::new(range, SpanStyle::new().color(color))
    }

    fn bold(style: &SpanStyle) -> bool {
        style.font_weight == Some(FontWeight::Bold)
    }

    const RED: Color = Color::Rgba(255, 0, 0, 255);
    const BLUE: Color = Color::Rgba(0, 0, 255, 255);

    #[test]
    fn the_pieces_cover_the_text_with_the_colors_of_the_syntax_and_the_name_in_bold() {
        // "let r = Rules;": the keyword is red, the name is blue, and it is what is used.
        let text = "let r = Rules;";
        let pieces = spans(text, &[colored(0..3, RED), colored(8..13, BLUE)], 8..13);
        let ranges: Vec<_> = pieces
            .iter()
            .map(|(range, _)| range.clone())
            .collect();
        assert_eq!(
            ranges,
            [0..3, 3..8, 8..13, 13..14],
            "from the start to the end, with no gaps"
        );
        assert_eq!(pieces[0].1.color, Some(RED));
        assert_eq!(pieces[1].1.color, None, "nothing says what the rest is");
        assert_eq!(
            (pieces[2].1.color, bold(&pieces[2].1)),
            (Some(BLUE), true),
            "the name keeps its color and is bold"
        );
        assert!(!bold(&pieces[0].1) && !bold(&pieces[3].1));
    }

    #[test]
    fn a_name_that_the_syntax_says_nothing_of_is_bold_and_a_style_that_covers_more_is_cut_at_the_name()
     {
        let text = "use_it(1);";
        let pieces = spans(text, &[], 0..6);
        assert_eq!(pieces.len(), 2);
        assert!(bold(&pieces[0].1) && !bold(&pieces[1].1));
        // One style over all of it is split where the name is, and every piece has it.
        let all = spans(text, &[colored(0..10, RED)], 4..6);
        assert_eq!(
            all.iter()
                .map(|(range, _)| range.clone())
                .collect::<Vec<_>>(),
            [0..4, 4..6, 6..10]
        );
        assert!(
            all.iter()
                .all(|(_, style)| style.color == Some(RED))
        );
        assert_eq!(
            all.iter()
                .map(|(_, style)| bold(style))
                .collect::<Vec<_>>(),
            [false, true, false]
        );
    }

    #[test]
    fn nothing_to_mark_and_nothing_to_style_is_one_piece_and_an_empty_text_has_none() {
        assert_eq!(spans("abc", &[], 0..0).len(), 1);
        assert!(!bold(&spans("abc", &[], 0..0)[0].1));
        assert!(spans("", &[], 0..0).is_empty());
    }

    #[test]
    fn ranges_past_the_end_or_inside_a_character_are_kept_to_the_text() {
        let text = "\u{e9}x";
        // The style begins inside the accent, and the mark runs past the end.
        let pieces = spans(text, &[colored(1..3, RED)], 2..99);
        let ranges: Vec<_> = pieces
            .iter()
            .map(|(range, _)| range.clone())
            .collect();
        assert_eq!(
            ranges,
            [0..2, 2..3],
            "no piece begins or ends inside a character"
        );
        assert!(bold(&pieces[1].1));
    }
}
