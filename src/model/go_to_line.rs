//! Go to Line / Column prompt state, input parser (`80` or `80:80`), and line/column-to-byte-offset resolver.
//!
//! This module owns only the prompt's state and pure parsing/resolution logic; the modal dialog that draws it lives under
//! `screen::editor::go_to_line_card`.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use aimer::input::input::{TextEditingController, TextSnapshot};
use aimer::{FocusNode, Venus};

use crate::ui::components::popup::Redraw;

/// Result of parsing a Go to Line/Column input string such as `"80"` or `"80:80"`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LineColVerdict {
    /// Blank input; nothing to jump to.
    Empty,
    /// Valid 1-based line and optional 1-based column.
    Valid { line: usize, column: Option<usize> },
    /// Malformed input with a short message for the dialog.
    Invalid(&'static str),
}

/// Parses a Go to Line/Column string (`"80"`, `"80:80"`, `"80:"`).
///
/// Whitespace around the numbers is ignored, and 0 is clamped to 1.
pub fn parse_line_column(input: &str) -> LineColVerdict {
    const HELP: &str = "line:column";
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return LineColVerdict::Empty;
    }
    let parse_num = |part: &str| -> Option<usize> {
        let part = part.trim();
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        part.parse::<usize>().ok().map(|n| n.max(1))
    };
    match trimmed.split_once(':') {
        None => match parse_num(trimmed) {
            Some(line) => LineColVerdict::Valid { line, column: None },
            None => LineColVerdict::Invalid(HELP),
        },
        Some((left, right)) => {
            let Some(line) = parse_num(left) else {
                return LineColVerdict::Invalid(HELP);
            };
            let right = right.trim();
            if right.is_empty() {
                return LineColVerdict::Valid { line, column: None };
            }
            match parse_num(right) {
                Some(column) => LineColVerdict::Valid {
                    line,
                    column: Some(column),
                },
                None => LineColVerdict::Invalid(HELP),
            }
        }
    }
}

/// Resolves a 1-based `line` and optional 1-based `column` within `snapshot` to a byte offset on a valid UTF-8 character
/// boundary.
///
/// Out-of-range lines clamp to the last line; out-of-range columns clamp to the end of that line's content (before any
/// trailing `\r\n` or `\n`). Reads only the target line, never the whole file.
pub fn resolve_byte_offset(snapshot: &TextSnapshot, line: usize, column: Option<usize>) -> usize {
    let lines = snapshot.line_count();
    if lines == 0 {
        return 0;
    }
    let row = line.saturating_sub(1).min(lines - 1);
    let range = snapshot.line_range(row);
    let Some(col) = column else {
        return range.start;
    };
    let target = col.saturating_sub(1);
    if target == 0 {
        return range.start;
    }
    let text = snapshot.slice(range.clone());
    let content = text.strip_suffix('\n').unwrap_or(&text);
    let content = content.strip_suffix('\r').unwrap_or(content);
    let offset_in_line = if content.is_ascii() {
        target.min(content.len())
    } else {
        content
            .char_indices()
            .nth(target)
            .map_or(content.len(), |(idx, _)| idx)
    };
    range.start + offset_in_line
}

/// The open Go to Line/Column prompt: its input controller and focus handle.
pub struct GoToLineAsking {
    pub controller: TextEditingController,
    pub focus: FocusNode,
}

/// Holds the currently open Go to Line/Column prompt (if any) and any validation complaint.
#[derive(Default)]
pub struct GoToLinePrompt {
    asking: Rc<RefCell<Option<Rc<GoToLineAsking>>>>,
    complaint: RefCell<Option<String>>,
    redraw: Redraw,
}

impl GoToLinePrompt {
    /// Delay before focusing the prompt's text field so the modal finishes mounting first.
    const FOCUS_DELAY: Duration = Duration::from_millis(32);

    /// Opens a fresh Go to Line/Column prompt with an empty input field.
    pub fn open(&self) {
        let asking = Rc::new(GoToLineAsking {
            controller: TextEditingController::new(),
            focus: FocusNode::new(),
        });
        *self.asking.borrow_mut() = Some(Rc::clone(&asking));
        *self.complaint.borrow_mut() = None;
        self.redraw.fire();
        if let Some(venus) = Venus::current() {
            let (waited, open) = (venus.sleep(Self::FOCUS_DELAY), Rc::clone(&self.asking));
            venus.spawn(async move {
                let _ = waited.await;
                if open
                    .borrow()
                    .as_ref()
                    .is_some_and(|now| Rc::ptr_eq(now, &asking))
                {
                    asking.focus.request_focus();
                }
            });
        }
    }

    /// Closes the prompt if open and clears any complaint.
    pub fn close(&self) {
        let was_open = self.asking.borrow_mut().take().is_some();
        *self.complaint.borrow_mut() = None;
        if was_open {
            self.redraw.fire();
        }
    }

    /// Closes the prompt only if `target` is still the active asking instance.
    pub fn close_if_same(&self, target: &Rc<GoToLineAsking>) {
        let same = self
            .asking
            .borrow()
            .as_ref()
            .is_some_and(|now| Rc::ptr_eq(now, target));
        if same {
            self.close();
        }
    }

    /// The currently open prompt, if any.
    pub fn asking(&self) -> Option<Rc<GoToLineAsking>> {
        self.asking.borrow().clone()
    }

    /// Shows a validation error message beneath the input field.
    pub fn complain(&self, message: impl Into<String>) {
        *self.complaint.borrow_mut() = Some(message.into());
        self.redraw.fire();
    }

    /// The current error message, if any.
    pub fn complaint(&self) -> Option<String> {
        self.complaint.borrow().clone()
    }

    /// Clears any complaint when the user edits the input again.
    pub fn typed(&self) {
        if self.complaint.borrow_mut().take().is_some() {
            self.redraw.fire();
        }
    }

    /// Registers a watcher notified whenever the prompt opens, closes, or changes its complaint.
    pub fn on_change(&self, hook: Rc<dyn Fn() -> bool>) {
        self.redraw.add(hook);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn snap(text: &str) -> TextSnapshot {
        TextEditingController::with_text(text).snapshot()
    }

    #[test]
    fn parses_line_only_and_line_colon_column() {
        assert_eq!(
            parse_line_column("80"),
            LineColVerdict::Valid {
                line: 80,
                column: None,
            }
        );
        assert_eq!(
            parse_line_column("80:80"),
            LineColVerdict::Valid {
                line: 80,
                column: Some(80),
            }
        );
        assert_eq!(
            parse_line_column("  12 : 34  "),
            LineColVerdict::Valid {
                line: 12,
                column: Some(34),
            }
        );
        assert_eq!(
            parse_line_column("42:"),
            LineColVerdict::Valid {
                line: 42,
                column: None,
            }
        );
        assert_eq!(
            parse_line_column("0"),
            LineColVerdict::Valid {
                line: 1,
                column: None,
            }
        );
        assert_eq!(
            parse_line_column("10:0"),
            LineColVerdict::Valid {
                line: 10,
                column: Some(1),
            }
        );
    }

    #[test]
    fn rejects_invalid_and_detects_empty_inputs() {
        assert_eq!(parse_line_column(""), LineColVerdict::Empty);
        assert_eq!(parse_line_column("   "), LineColVerdict::Empty);
        assert!(matches!(
            parse_line_column(":80"),
            LineColVerdict::Invalid(_)
        ));
        assert!(matches!(
            parse_line_column("abc"),
            LineColVerdict::Invalid(_)
        ));
        assert!(matches!(
            parse_line_column("80:abc"),
            LineColVerdict::Invalid(_)
        ));
        assert!(matches!(
            parse_line_column("1:2:3"),
            LineColVerdict::Invalid(_)
        ));
        assert!(matches!(
            parse_line_column("-5"),
            LineColVerdict::Invalid(_)
        ));
        assert!(matches!(
            parse_line_column("80:-1"),
            LineColVerdict::Invalid(_)
        ));
        assert!(matches!(
            parse_line_column("999999999999999999999999999999"),
            LineColVerdict::Invalid(_)
        ));
    }

    #[test]
    fn resolves_byte_offsets_for_empty_single_line_lf_crlf_multibyte_combining_and_long_lines() {
        // Empty text.
        let empty = snap("");
        assert_eq!(resolve_byte_offset(&empty, 1, None), 0);
        assert_eq!(resolve_byte_offset(&empty, 10, Some(5)), 0);

        // Single line without trailing newline.
        let single = snap("hello");
        assert_eq!(resolve_byte_offset(&single, 1, None), 0);
        assert_eq!(resolve_byte_offset(&single, 1, Some(1)), 0);
        assert_eq!(resolve_byte_offset(&single, 1, Some(3)), 2);
        assert_eq!(resolve_byte_offset(&single, 1, Some(6)), 5);
        assert_eq!(resolve_byte_offset(&single, 1, Some(99)), 5);
        assert_eq!(resolve_byte_offset(&single, 5, None), 0);

        // Multi-line LF: clamps column before `\n`.
        let lf = snap("abc\ndef\nghi");
        assert_eq!(resolve_byte_offset(&lf, 2, None), 4);
        assert_eq!(resolve_byte_offset(&lf, 2, Some(2)), 5);
        assert_eq!(resolve_byte_offset(&lf, 2, Some(99)), 7);
        assert_eq!(resolve_byte_offset(&lf, 99, Some(2)), 9);

        // Multi-line CRLF: clamps column before `\r\n`.
        let crlf = snap("abc\r\ndef\r\n");
        assert_eq!(resolve_byte_offset(&crlf, 1, Some(99)), 3);
        assert_eq!(resolve_byte_offset(&crlf, 2, Some(2)), 6);
        assert_eq!(resolve_byte_offset(&crlf, 2, Some(99)), 8);

        // Multi-byte UTF-8.
        let utf8 = snap("café\n日本語\n");
        assert_eq!(resolve_byte_offset(&utf8, 1, Some(4)), 3);
        assert_eq!(resolve_byte_offset(&utf8, 1, Some(5)), 5);
        assert_eq!(resolve_byte_offset(&utf8, 2, Some(2)), 9);

        // Combining marks stay on valid UTF-8 char boundaries.
        let combining = snap("e\u{0301}x");
        let off = resolve_byte_offset(&combining, 1, Some(2));
        assert!(combining.is_char_boundary(off));
        assert_eq!(
            resolve_byte_offset(&combining, 1, Some(99)),
            "e\u{0301}x".len()
        );

        // Very long line.
        let long_line = "a".repeat(10_000);
        let long = snap(&long_line);
        assert_eq!(resolve_byte_offset(&long, 1, Some(5_001)), 5_000);
        assert_eq!(resolve_byte_offset(&long, 1, Some(99_999)), 10_000);
    }

    #[test]
    fn prompt_lifecycle_notifies_watchers_and_close_if_same_ignores_stale_instances() {
        let prompt = GoToLinePrompt::default();
        let changes = Rc::new(Cell::new(0));
        let counted = Rc::clone(&changes);
        prompt.on_change(Rc::new(move || {
            counted.set(counted.get() + 1);
            true
        }));

        prompt.open();
        let first = prompt.asking().expect("open");
        prompt.open();
        let second = prompt.asking().expect("reopened");
        prompt.complain("Bad input");
        let before = changes.get();

        prompt.close_if_same(&first);
        assert!(
            prompt
                .asking()
                .is_some_and(|now| Rc::ptr_eq(&now, &second))
        );
        assert_eq!(prompt.complaint().as_deref(), Some("Bad input"));
        assert_eq!(changes.get(), before);

        prompt.typed();
        assert_eq!(prompt.complaint(), None);

        prompt.close_if_same(&second);
        assert!(prompt.asking().is_none());
    }
}
