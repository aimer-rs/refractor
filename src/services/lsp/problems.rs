//! The problems of a file as a list, in the order they are to be read: what the panel under the editor
//! shows. The language server says where each is as bytes of the text; here each is given the line and the
//! column a person counts, and the list is put in order.

use aimer::input::TextEditingValue;

use super::protocol::Severity;
use super::session::Diagnostic;

/// How much a problem weighs, which is what the list is sorted by: the worst first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Weight {
    Error,
    Warning,
    /// What the server calls information and hints: things that are worth saying and not worth worrying about.
    Weak,
}

impl From<Severity> for Weight {
    fn from(severity: Severity) -> Self {
        match severity {
            Severity::Error => Self::Error,
            Severity::Warning => Self::Warning,
            Severity::Information | Severity::Hint => Self::Weak,
        }
    }
}

impl super::session::Diagnostic {
    /// How much it weighs.
    pub fn weight(&self) -> Weight {
        self.severity.into()
    }
}

/// One problem of a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub weight: Weight,
    pub message: String,
    /// The server's name for the kind of problem, such as an error number.
    pub code: Option<String>,
    /// Where it is, counted from one as a person counts: the line, and the column in characters as the status bar
    /// has it.
    pub line: usize,
    pub column: usize,
    /// The byte of the text it starts at, which is where the caret goes to see it.
    pub at: usize,
}

impl Problem {
    /// The first line of the message that has anything in it: a message can be many lines, and a row of a list
    /// has room for one.
    pub fn headline(&self) -> &str {
        self.message
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("")
    }
}

/// The problems that `diagnostics` are of the text `value`, worst first, and in the order of the text among
/// those that weigh the same.
pub fn problems_of(diagnostics: &[Diagnostic], value: &TextEditingValue) -> Vec<Problem> {
    let mut problems: Vec<Problem> = diagnostics
        .iter()
        .map(|diagnostic| {
            let at = diagnostic.range.start;
            let (line, column) = value.line_and_column(at);
            Problem {
                weight: diagnostic.severity.into(),
                message: diagnostic.message.clone(),
                code: diagnostic.code.clone(),
                line: line + 1,
                column: column + 1,
                at,
            }
        })
        .collect();
    // The message is the last of the keys, so that the order does not depend on the order the server said them in.
    problems.sort_by(|a, b| (a.weight, a.at, &a.message).cmp(&(b.weight, b.at, &b.message)));
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str) -> TextEditingValue {
        TextEditingValue::new(text, aimer::input::TextSelection::collapsed(0), None)
    }

    fn diagnostic(
        range: std::ops::Range<usize>,
        severity: Severity,
        message: &str,
        code: Option<&str>,
    ) -> Diagnostic {
        Diagnostic {
            range,
            severity,
            message: message.to_owned(),
            code: code.map(str::to_owned),
            unused: false,
        }
    }

    #[test]
    fn what_the_server_calls_information_and_hints_weigh_the_least() {
        assert_eq!(Weight::from(Severity::Error), Weight::Error);
        assert_eq!(Weight::from(Severity::Warning), Weight::Warning);
        assert_eq!(Weight::from(Severity::Information), Weight::Weak);
        assert_eq!(Weight::from(Severity::Hint), Weight::Weak);
        assert!(
            Weight::Error < Weight::Warning && Weight::Warning < Weight::Weak,
            "the worst sorts first"
        );
    }

    #[test]
    fn the_list_is_errors_then_warnings_then_weak_warnings_and_each_in_the_order_of_the_text() {
        let text = "line zero\nline one\nline two\nline three\n";
        let at = |needle: &str| text.find(needle).unwrap();
        let diagnostics = [
            diagnostic(at("two")..at("two") + 3, Severity::Hint, "hint late", None),
            diagnostic(
                at("one")..at("one") + 3,
                Severity::Warning,
                "warning middle",
                Some("W1"),
            ),
            diagnostic(
                at("three")..at("three") + 5,
                Severity::Error,
                "error last",
                Some("E0425"),
            ),
            diagnostic(0..4, Severity::Information, "info first", None),
            diagnostic(
                at("zero")..at("zero") + 4,
                Severity::Error,
                "error first",
                None,
            ),
            diagnostic(0..4, Severity::Warning, "warning first", None),
        ];
        let problems = problems_of(&diagnostics, &value(text));
        let order: Vec<(Weight, &str)> = problems
            .iter()
            .map(|problem| (problem.weight, problem.message.as_str()))
            .collect();
        assert_eq!(
            order,
            vec![
                (Weight::Error, "error first"),
                (Weight::Error, "error last"),
                (Weight::Warning, "warning first"),
                (Weight::Warning, "warning middle"),
                (Weight::Weak, "info first"),
                (Weight::Weak, "hint late"),
            ]
        );
    }

    #[test]
    fn problems_at_the_same_place_and_of_the_same_weight_keep_to_a_fixed_order_by_their_message() {
        let diagnostics = [
            diagnostic(2..3, Severity::Error, "b", None),
            diagnostic(2..3, Severity::Error, "a", None),
            diagnostic(2..3, Severity::Error, "c", None),
        ];
        let messages: Vec<String> = problems_of(&diagnostics, &value("abcdef"))
            .into_iter()
            .map(|problem| problem.message)
            .collect();
        assert_eq!(messages, ["a", "b", "c"]);
    }

    #[test]
    fn a_problem_is_at_the_line_and_the_column_a_person_counts_from_one() {
        // The second line begins with "é" (two bytes), a letter with a combining mark (two characters, one grapheme) and an emoji.
        let text = "first\n\u{e9}e\u{301}\u{1f600}X\r\nthird";
        let x = text.find('X').unwrap();
        let problems = problems_of(
            &[diagnostic(x..x + 1, Severity::Error, "here", None)],
            &value(text),
        );
        assert_eq!(
            (problems[0].line, problems[0].column),
            (2, 4),
            "after three graphemes on the second line"
        );
        assert_eq!(problems[0].at, x);
        let first = problems_of(
            &[diagnostic(0..1, Severity::Warning, "start", None)],
            &value(text),
        );
        assert_eq!((first[0].line, first[0].column, first[0].at), (1, 1, 0));
        let third = text.find("third").unwrap();
        let last = problems_of(
            &[diagnostic(
                third + 5..third + 5,
                Severity::Warning,
                "end of the text",
                None,
            )],
            &value(text),
        );
        assert_eq!(
            (last[0].line, last[0].column),
            (3, 6),
            "a problem that is a point at the very end"
        );
    }

    #[test]
    fn a_problem_in_empty_text_is_at_the_start() {
        let problems = problems_of(
            &[diagnostic(0..0, Severity::Error, "empty", None)],
            &value(""),
        );
        assert_eq!(
            (problems[0].line, problems[0].column, problems[0].at),
            (1, 1, 0)
        );
        assert!(problems_of(&[], &value("anything")).is_empty());
    }

    #[test]
    fn the_headline_is_the_first_line_that_says_something() {
        let make = |message: &str| Problem {
            weight: Weight::Error,
            message: message.to_owned(),
            code: None,
            line: 1,
            column: 1,
            at: 0,
        };
        assert_eq!(make("one line").headline(), "one line");
        assert_eq!(make("first\nsecond\nthird").headline(), "first");
        assert_eq!(
            make("\n  \n  indented first  \nnext").headline(),
            "indented first"
        );
        assert_eq!(make("").headline(), "");
        assert_eq!(make("\n\n").headline(), "");
    }
}
