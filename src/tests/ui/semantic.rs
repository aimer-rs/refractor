use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use aimer::Color;
use aimer::input::TextEditingController;
use serde_json::{Value, json};

use super::*;
use crate::services::lsp::fake::{Fake, installed, lsp_with, settle};
use crate::services::lsp::{Legend, SemanticToken};

const FILE: &str = "/project/src/main.rs";
const TEXT: &str = "fn main() {\n    let mut count = 1;\n    let total = 2;\n}\n";

fn legend() -> Rc<Legend> {
    Rc::new(Legend {
        types: vec![
            "keyword".into(),
            "variable".into(),
            "struct".into(),
            "string".into(),
        ],
        modifiers: vec!["mutable".into(), "controlFlow".into()],
    })
}

fn token(range: Range<usize>, kind: u32, modifiers: u32) -> SemanticToken {
    SemanticToken {
        range,
        kind,
        modifiers,
    }
}

/// `count` is a mutable variable (bytes 24..29), `total` is a variable (bytes 41..46) and `fn` is a keyword.
fn found(text: &str) -> SemanticTokens {
    SemanticTokens {
        tokens: vec![
            token(0..2, 0, 0),
            token(24..29, 1, 0b1),
            token(41..46, 1, 0),
            token(50..52, 3, 0),
        ],
        legend: legend(),
        text: TextSnapshot::from_text(text),
    }
}

fn red() -> SpanStyle {
    SpanStyle::new().color(Color::Rgb(255, 0, 0))
}

fn summary(runs: &[StyledRange]) -> Vec<(Range<usize>, Option<Color>)> {
    runs.iter()
        .map(|run| (run.range.clone(), run.style.color))
        .collect()
}

const MUTABLE: Option<Color> = Some(Color::Rgb(0xFF, 0x6B, 0x81));
const VARIABLE: Option<Color> = Some(Color::Rgb(0xE5, 0xFB, 0xCD));
const KEYWORD: Option<Color> = Some(Color::Rgb(0xF8, 0x5F, 0xCD));

#[test]
fn only_the_tokens_that_a_rule_is_for_are_kept_and_what_is_alike_has_one_style() {
    let resolved = Resolved::of(&found(TEXT), ThemeMode::Dark);
    // The string has no rule, and the mutable and the plain variable are two styles, as the keyword is a third.
    assert_eq!(resolved.tokens.len(), 3);
    assert_eq!(resolved.styles.len(), 3);
    let again = SemanticTokens {
        tokens: vec![token(0..2, 1, 0), token(3..7, 1, 0), token(8..9, 1, 0)],
        ..found(TEXT)
    };
    assert_eq!(
        Resolved::of(&again, ThemeMode::Dark)
            .styles
            .len(),
        1,
        "three variables, one style"
    );
    assert_eq!(
        resolved.styles[usize::from(resolved.tokens[1].style)].color,
        MUTABLE
    );
    assert_eq!(
        resolved.styles[usize::from(resolved.tokens[2].style)].color,
        VARIABLE
    );
    assert_ne!(
        Resolved::of(&found(TEXT), ThemeMode::Light).styles[0].color,
        resolved.styles[0].color,
        "the light page has its own colors"
    );
}

#[test]
fn a_kind_the_legend_does_not_have_is_left_out() {
    let broken = SemanticTokens {
        tokens: vec![token(0..2, 40, 0), token(3..7, 1, 0)],
        ..found(TEXT)
    };
    assert_eq!(
        Resolved::of(&broken, ThemeMode::Dark)
            .tokens
            .len(),
        1
    );
}

#[test]
fn the_tokens_after_an_edit_are_moved_by_it_and_those_it_touched_are_gone_and_those_before_it_stay()
{
    let resolved = Resolved::of(&found(TEXT), ThemeMode::Dark);
    let spans = |text: &TextSnapshot| {
        resolved
            .moved_to(text)
            .iter()
            .map(|token| token.start..token.end)
            .collect::<Vec<_>>()
    };
    let same = TextSnapshot::from_text(TEXT);
    assert_eq!(spans(&same), [0..2, 24..29, 41..46]);
    // `// hi` and a line break put in at the start: everything is six bytes along.
    let added = same.with_replaced(0..0, "// hi\n");
    assert_eq!(spans(&added), [6..8, 30..35, 47..52]);
    // The `s` of `count` is typed over: that token is gone, and the ones around it are where they were or moved by what was taken.
    let touched = same.with_replaced(27..28, "Z");
    assert_eq!(spans(&touched), [0..2, 41..46]);
    // Text taken out between the two names moves the second back.
    let taken = same.with_replaced(30..35, "");
    assert_eq!(spans(&taken), [0..2, 24..29, 36..41]);
    // Text added right after a name is not in the name, and right before it moves it.
    assert_eq!(
        spans(&same.with_replaced(29..29, "x")),
        [0..2, 24..29, 42..47]
    );
    assert_eq!(
        spans(&same.with_replaced(24..24, "x")),
        [0..2, 25..30, 42..47]
    );
    // An empty text, and a text of one line without a final line break.
    assert!(spans(&TextSnapshot::from_text("")).is_empty());
    // A text that is replaced but for what it begins with keeps what is in that.
    assert_eq!(spans(&same.with_replaced(0..same.len(), "fn")), [0..2]);
    assert!(spans(&same.with_replaced(0..same.len(), "xx")).is_empty());
}

#[test]
fn a_token_is_drawn_over_the_run_of_the_grammar_and_the_run_is_cut_round_it() {
    let styles = [SpanStyle::new().color(Color::Rgb(1, 2, 3))];
    let tokens = [Styled {
        start: 4,
        end: 7,
        style: 0,
    }];
    let found = overlaid(vec![StyledRange::new(0..10, red())], &tokens, &styles);
    assert_eq!(
        summary(&found),
        vec![
            (0..4, red().color),
            (4..7, Some(Color::Rgb(1, 2, 3))),
            (7..10, red().color)
        ]
    );
}

#[test]
fn a_token_over_nothing_is_drawn_and_runs_with_no_token_are_kept_and_the_result_is_in_order() {
    let styles = [SpanStyle::new().color(Color::Rgb(1, 2, 3))];
    let tokens = [
        Styled {
            start: 12,
            end: 14,
            style: 0,
        },
        Styled {
            start: 20,
            end: 22,
            style: 0,
        },
    ];
    let base = vec![
        StyledRange::new(0..5, red()),
        StyledRange::new(18..24, red()),
    ];
    let found = overlaid(base.clone(), &tokens, &styles);
    let tinted = Some(Color::Rgb(1, 2, 3));
    assert_eq!(
        summary(&found),
        vec![
            (0..5, red().color),
            (12..14, tinted),
            (18..20, red().color),
            (20..22, tinted),
            (22..24, red().color)
        ]
    );
    assert_eq!(
        overlaid(base.clone(), &[], &styles),
        base,
        "no tokens, no change"
    );
    assert_eq!(
        summary(&overlaid(Vec::new(), &tokens, &styles)).len(),
        2,
        "no grammar, only the tokens"
    );
}

#[test]
fn a_token_that_is_the_whole_of_a_run_replaces_it_and_one_that_spans_two_cuts_both() {
    let styles = [SpanStyle::new().color(Color::Rgb(1, 2, 3))];
    let tinted = Some(Color::Rgb(1, 2, 3));
    let whole = overlaid(
        vec![StyledRange::new(4..7, red())],
        &[Styled {
            start: 4,
            end: 7,
            style: 0,
        }],
        &styles,
    );
    assert_eq!(summary(&whole), vec![(4..7, tinted)]);
    let base = vec![
        StyledRange::new(0..5, red()),
        StyledRange::new(5..10, red()),
    ];
    let across = overlaid(
        base,
        &[Styled {
            start: 3,
            end: 8,
            style: 0,
        }],
        &styles,
    );
    assert_eq!(
        summary(&across),
        vec![(0..3, red().color), (3..8, tinted), (8..10, red().color)]
    );
}

/// A server that has tokens and is ready, with the file open, and a cell that is attached to the file.
fn ready() -> (Rc<aimer::Venus>, Fake, Rc<Semantic>, TextEditingController) {
    let venus = installed();
    let fake = Fake::new();
    let lsp = lsp_with(&fake);
    let controller = TextEditingController::with_text(TEXT);
    lsp.open(Path::new(FILE), &controller, Rc::new(|| {}));
    settle(&venus);
    let _ = fake.requests();
    fake.answer(
        1,
        json!({"capabilities": {
            "textDocumentSync": 2,
            "positionEncoding": "utf-8",
            "semanticTokensProvider": {
                "legend": {"tokenTypes": ["keyword", "variable", "struct", "string"], "tokenModifiers": ["mutable", "controlFlow"]},
                "full": true,
            },
        }}),
    );
    settle(&venus);
    let _ = fake.requests();
    let semantic = Rc::new(Semantic::default());
    semantic.attach(
        lsp,
        PathBuf::from(FILE),
        controller.clone(),
        ThemeMode::Dark,
    );
    (venus, fake, semantic, controller)
}

/// `fn`, `count` (mutable) and `total`: the tokens of `TEXT`.
fn answer() -> Value {
    json!({"data": [0, 0, 2, 0, 0, 1, 12, 5, 1, 1, 1, 8, 5, 1, 0]})
}

fn snapshot() -> TextSnapshot {
    TextSnapshot::from_text(TEXT)
}

#[test]
fn nothing_is_drawn_over_the_grammar_until_the_server_has_said_something() {
    let (_venus, _fake, semantic, _controller) = ready();
    let base = vec![StyledRange::new(0..2, red())];
    assert_eq!(
        semantic.overlay(base.clone(), &snapshot(), 0..TEXT.len()),
        base
    );
}

#[test]
fn the_tokens_the_server_says_are_drawn_over_the_grammar_and_the_pane_is_drawn_again() {
    let (venus, fake, semantic, _controller) = ready();
    let drawn = Rc::new(Cell::new(0));
    let counted = Rc::clone(&drawn);
    semantic.on_change(Rc::new(move || {
        counted.set(counted.get() + 1);
        true
    }));
    semantic.refresh();
    let told = fake.requests();
    assert_eq!(told[0].0, "textDocument/semanticTokens/full");
    fake.answer(told[0].2.unwrap(), answer());
    settle(&venus);
    assert_eq!(drawn.get(), 1);

    let base = vec![StyledRange::new(0..TEXT.len(), red())];
    let found = semantic.overlay(base, &snapshot(), 0..TEXT.len());
    let tokens: Vec<_> = summary(&found)
        .into_iter()
        .filter(|(_, color)| *color != red().color)
        .collect();
    assert_eq!(
        tokens,
        vec![(0..2, KEYWORD), (24..29, MUTABLE), (43..48, VARIABLE)]
    );
}

#[test]
fn only_the_tokens_of_the_part_asked_about_are_drawn() {
    let (venus, fake, semantic, _controller) = ready();
    semantic.refresh();
    let id = fake.requests()[0].2.unwrap();
    fake.answer(id, answer());
    settle(&venus);
    let found = semantic.overlay(Vec::new(), &snapshot(), 20..35);
    assert_eq!(summary(&found), vec![(24..29, MUTABLE)]);
    assert!(
        semantic
            .overlay(Vec::new(), &snapshot(), 0..0)
            .is_empty(),
        "no part, no tokens"
    );
}

#[test]
fn the_tokens_follow_the_text_as_it_is_typed_in_and_a_name_that_was_typed_in_is_the_grammars() {
    let (venus, fake, semantic, _controller) = ready();
    semantic.refresh();
    let id = fake.requests()[0].2.unwrap();
    fake.answer(id, answer());
    settle(&venus);
    let typed = snapshot().with_replaced(0..0, "// hi\n");
    let moved = semantic.overlay(Vec::new(), &typed, 0..typed.len());
    assert_eq!(
        moved
            .iter()
            .map(|run| run.range.clone())
            .collect::<Vec<_>>(),
        [6..8, 30..35, 49..54]
    );
    let inside = snapshot().with_replaced(26..27, "X");
    let left = semantic.overlay(Vec::new(), &inside, 0..inside.len());
    assert_eq!(
        left.iter()
            .map(|run| run.range.clone())
            .collect::<Vec<_>>(),
        [0..2, 43..48],
        "`count` was typed in"
    );
}

#[test]
fn the_same_tokens_asked_again_draw_nothing_again_and_other_ones_do() {
    let (venus, fake, semantic, _controller) = ready();
    let drawn = Rc::new(Cell::new(0));
    let counted = Rc::clone(&drawn);
    semantic.on_change(Rc::new(move || {
        counted.set(counted.get() + 1);
        true
    }));
    let ask = |reply: Value| {
        semantic.refresh();
        let id = fake.requests()[0].2.unwrap();
        fake.answer(id, reply);
        settle(&venus);
    };
    ask(answer());
    ask(answer());
    assert_eq!(drawn.get(), 1, "the same");
    ask(json!({"data": [0, 0, 2, 0, 0]}));
    assert_eq!(drawn.get(), 2, "fewer");
}

#[test]
fn what_was_said_is_kept_when_the_server_cannot_say_it_again() {
    let (venus, fake, semantic, _controller) = ready();
    semantic.refresh();
    let id = fake.requests()[0].2.unwrap();
    fake.answer(id, answer());
    settle(&venus);
    semantic.refresh();
    let id = fake.requests()[0].2.unwrap();
    fake.fail(id);
    settle(&venus);
    assert_eq!(
        semantic
            .overlay(Vec::new(), &snapshot(), 0..TEXT.len())
            .len(),
        3
    );
}

#[test]
fn a_request_that_is_made_again_makes_the_one_before_unwanted_and_its_answer_is_not_taken() {
    let (venus, fake, semantic, _controller) = ready();
    semantic.refresh();
    let first = fake.requests()[0].2.unwrap();
    semantic.refresh();
    let told = fake.requests();
    assert_eq!(told[0].0, "$/cancelRequest");
    fake.answer(first, answer());
    settle(&venus);
    assert!(
        semantic
            .overlay(Vec::new(), &snapshot(), 0..TEXT.len())
            .is_empty()
    );
}

#[test]
fn a_new_mode_draws_the_tokens_in_its_own_colors() {
    let (venus, fake, semantic, controller) = ready();
    semantic.refresh();
    let id = fake.requests()[0].2.unwrap();
    fake.answer(id, answer());
    settle(&venus);
    let dark = semantic.overlay(Vec::new(), &snapshot(), 0..TEXT.len());
    let lsp = semantic
        .context
        .borrow()
        .as_ref()
        .unwrap()
        .lsp
        .clone();
    semantic.attach(lsp, PathBuf::from(FILE), controller, ThemeMode::Light);
    let light = semantic.overlay(Vec::new(), &snapshot(), 0..TEXT.len());
    assert_eq!(dark.len(), light.len());
    assert_ne!(summary(&dark), summary(&light));
}

#[test]
fn the_server_saying_that_its_tokens_are_not_as_they_were_asks_for_them_again() {
    let (venus, fake, semantic, _controller) = ready();
    // It is said once, and there is a request for the tokens when it is.
    fake.request(json!(5), "workspace/semanticTokens/refresh", Value::Null);
    settle(&venus);
    let sent = fake.requests();
    assert!(
        sent.iter()
            .any(|(method, _, _)| method == "textDocument/semanticTokens/full"),
        "{sent:?}"
    );
    assert!(Rc::strong_count(&semantic) >= 1);
}

#[test]
fn the_retries_go_on_while_the_server_is_not_done_or_could_not_say_and_stop_at_last() {
    assert_eq!(
        Semantic::retry_delay(true, true, 0),
        Some(Semantic::RETRY_DELAY)
    );
    assert_eq!(
        Semantic::retry_delay(false, false, 5),
        Some(Semantic::RETRY_DELAY)
    );
    assert_eq!(Semantic::retry_delay(false, true, 0), None);
    assert_eq!(
        Semantic::retry_delay(true, false, Semantic::MOST_RETRIES),
        None
    );
}

#[test]
fn a_cell_that_has_no_pane_asks_for_nothing() {
    let semantic = Rc::new(Semantic::default());
    semantic.refresh();
    assert!(
        semantic
            .overlay(Vec::new(), &snapshot(), 0..TEXT.len())
            .is_empty()
    );
    assert!(!semantic.server_idle());
}
