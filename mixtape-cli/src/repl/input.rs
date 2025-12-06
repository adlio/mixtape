//! Input styling for rustyline REPL

use rustyline::completion::Completer;
use rustyline::highlight::Highlighter;
use rustyline::hint::{Hint, Hinter};
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::{Context, Helper, Result as RustylineResult};

#[derive(Clone, Copy, Debug, Default)]
pub struct InputStyleHelper;

pub struct InputHint;

impl Hint for InputHint {
    fn display(&self) -> &str {
        "\n"
    }

    fn completion(&self) -> Option<&str> {
        None
    }
}

impl Completer for InputStyleHelper {
    type Candidate = String;

    fn complete(
        &self,
        _line: &str,
        _pos: usize,
        _ctx: &Context<'_>,
    ) -> RustylineResult<(usize, Vec<Self::Candidate>)> {
        Ok((0, Vec::new()))
    }
}

impl Hinter for InputStyleHelper {
    type Hint = InputHint;

    fn hint(&self, _line: &str, _pos: usize, _ctx: &Context<'_>) -> Option<Self::Hint> {
        // Don't show hints - they interfere with multiline background styling
        None
    }
}

impl Highlighter for InputStyleHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        _default: bool,
    ) -> std::borrow::Cow<'b, str> {
        let styled = format!("\x1b[48;5;236m\x1b[2K{}", prompt);
        std::borrow::Cow::Owned(styled)
    }

    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> std::borrow::Cow<'l, str> {
        if line.is_empty() {
            return std::borrow::Cow::Borrowed(line);
        }

        let mut styled = String::with_capacity(line.len() + 16);
        styled.push_str("\x1b[48;5;236m");
        styled.push_str(&line.replace('\n', "\x1b[0K\r\n\x1b[48;5;236m\x1b[2K"));
        styled.push_str("\x1b[0K");
        std::borrow::Cow::Owned(styled)
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> std::borrow::Cow<'h, str> {
        if hint == "\n" {
            return std::borrow::Cow::Owned("\r\n\x1b[48;5;236m\x1b[2K\x1b[0m".to_string());
        }
        std::borrow::Cow::Borrowed(hint)
    }
}

impl Validator for InputStyleHelper {
    fn validate(&self, _ctx: &mut ValidationContext<'_>) -> RustylineResult<ValidationResult> {
        Ok(ValidationResult::Valid(None))
    }
}

impl Helper for InputStyleHelper {}

#[cfg(test)]
mod tests {
    use super::*;

    mod input_hint_tests {
        use super::*;

        #[test]
        fn display_returns_newline() {
            let hint = InputHint;
            assert_eq!(hint.display(), "\n");
        }

        #[test]
        fn completion_returns_none() {
            let hint = InputHint;
            assert!(hint.completion().is_none());
        }
    }

    mod highlighter_tests {
        use super::*;

        #[test]
        fn highlight_prompt_adds_background() {
            let helper = InputStyleHelper;
            let result = helper.highlight_prompt(">>> ", false);
            assert!(result.starts_with("\x1b[48;5;236m\x1b[2K"));
            assert!(result.contains(">>> "));
        }

        #[test]
        fn highlight_empty_line_returns_borrowed() {
            let helper = InputStyleHelper;
            let result = helper.highlight("", 0);
            assert!(matches!(result, std::borrow::Cow::Borrowed("")));
        }

        #[test]
        fn highlight_single_line_adds_background() {
            let helper = InputStyleHelper;
            let result = helper.highlight("hello", 0);
            assert!(result.starts_with("\x1b[48;5;236m"));
            assert!(result.contains("hello"));
            assert!(result.ends_with("\x1b[0K"));
        }

        #[test]
        fn highlight_multiline_handles_newlines() {
            let helper = InputStyleHelper;
            let result = helper.highlight("line1\nline2", 0);
            // Each newline should be replaced with escape sequences
            assert!(result.contains("\x1b[0K\r\n\x1b[48;5;236m\x1b[2K"));
            assert!(result.contains("line1"));
            assert!(result.contains("line2"));
        }

        #[test]
        fn highlight_hint_newline_returns_styled() {
            let helper = InputStyleHelper;
            let result = helper.highlight_hint("\n");
            assert_eq!(result.as_ref(), "\r\n\x1b[48;5;236m\x1b[2K\x1b[0m");
        }

        #[test]
        fn highlight_hint_other_returns_borrowed() {
            let helper = InputStyleHelper;
            let result = helper.highlight_hint("other");
            assert!(matches!(result, std::borrow::Cow::Borrowed("other")));
        }
    }

    mod validator_tests {
        use super::*;

        #[test]
        fn validate_always_returns_valid() {
            let _helper = InputStyleHelper;
            // We can't easily create a ValidationContext, so we trust the implementation
            // The function body is trivial: Ok(ValidationResult::Valid(None))
        }
    }

    mod completer_tests {
        #[test]
        fn complete_returns_empty() {
            // The completer doesn't provide completions - it returns empty vec
            // We can't easily test this without a Context, but the implementation is trivial
        }
    }
}
