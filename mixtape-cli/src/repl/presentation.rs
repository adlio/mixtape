//! Tool presentation formatting for CLI output

use super::commands::Verbosity;
use super::formatter::ToolFormatter;
use mixtape_core::{Agent, AgentEvent, AgentHook, Display, ToolApprovalStatus};
use std::sync::{Arc, Mutex};

/// Hook that presents tool calls with rich CLI formatting
///
/// Generic over `F: ToolFormatter` to enable testing with mock formatters.
/// Defaults to `Agent` for normal usage.
pub struct PresentationHook<F: ToolFormatter = Agent> {
    formatter: Arc<F>,
    verbosity: Arc<Mutex<Verbosity>>,
}

impl<F: ToolFormatter> PresentationHook<F> {
    pub fn new(formatter: Arc<F>, verbosity: Arc<Mutex<Verbosity>>) -> Self {
        Self {
            formatter,
            verbosity,
        }
    }
}

impl<F: ToolFormatter + 'static> AgentHook for PresentationHook<F> {
    fn on_event(&self, event: &AgentEvent) {
        match event {
            AgentEvent::ToolStarted {
                name,
                input,
                approval_status,
                ..
            } => {
                let verbosity = *self.verbosity.lock().unwrap();
                if !should_print_start(name, approval_status, verbosity) {
                    return;
                }
                let formatted = self
                    .formatter
                    .format_tool_input(name, input, Display::Cli)
                    .and_then(|formatted| format_tool_input(name, &formatted, verbosity));

                let show_approval = matches!(approval_status, ToolApprovalStatus::UserApproved);
                if formatted.is_none() && !show_approval {
                    return;
                }

                println!("\n🛠️  \x1b[1m{}\x1b[0m", name);
                if let Some(output) = formatted {
                    println!("{}", indent_lines(&output));
                }
                if show_approval {
                    println!("  \x1b[33m(user approved)\x1b[0m");
                }
            }
            AgentEvent::ToolCompleted { name, output, .. } => {
                let verbosity = *self.verbosity.lock().unwrap();
                if verbosity == Verbosity::Quiet {
                    println!("\n\x1b[32m✓\x1b[0m \x1b[1m{}\x1b[0m", name);
                    return;
                }
                println!("\n\x1b[32m✓\x1b[0m \x1b[1m{}\x1b[0m", name);

                // Format tool output on-demand using CLIPresenter
                if let Some(formatted) =
                    self.formatter
                        .format_tool_output(name, output, Display::Cli)
                {
                    if let Some(output) = format_tool_output(name, &formatted, verbosity) {
                        println!("{}", indent_lines(&output));
                    } else {
                        println!("  (completed)");
                    }
                } else {
                    println!("  (completed)");
                }
            }
            AgentEvent::ToolFailed { name, error, .. } => {
                println!("\n\x1b[31m✗\x1b[0m \x1b[1m{}\x1b[0m", name);
                println!("{}", indent_lines(&format!("\x1b[31m{}\x1b[0m", error)));
            }
            _ => {}
        }
    }
}

fn should_print_start(
    tool_name: &str,
    approval_status: &ToolApprovalStatus,
    verbosity: Verbosity,
) -> bool {
    if verbosity == Verbosity::Verbose {
        return true;
    }
    if verbosity == Verbosity::Quiet {
        return false;
    }
    tool_is_long_running(tool_name) || matches!(approval_status, ToolApprovalStatus::UserApproved)
}

fn format_tool_input(tool_name: &str, formatted: &str, verbosity: Verbosity) -> Option<String> {
    if verbosity == Verbosity::Quiet {
        return None;
    }
    if verbosity == Verbosity::Verbose {
        return Some(formatted.to_string());
    }
    if tool_is_noisy(tool_name) {
        return None;
    }
    Some(formatted.to_string())
}

fn format_tool_output(tool_name: &str, formatted: &str, verbosity: Verbosity) -> Option<String> {
    if verbosity == Verbosity::Quiet {
        return None;
    }
    if verbosity == Verbosity::Verbose {
        return Some(formatted.to_string());
    }
    // Check for empty content before applying any formatting
    if formatted.trim().is_empty() {
        return None;
    }
    // Truncation is now handled by Tool::present_output_cli()
    let output = if tool_is_dimmed(tool_name) {
        dim_text(formatted)
    } else {
        formatted.to_string()
    };
    Some(output)
}

fn tool_is_long_running(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "start_process"
            | "read_process_output"
            | "interact_with_process"
            | "list_processes"
            | "list_sessions"
            | "search"
            | "fetch"
            | "list_directory"
    )
}

fn tool_is_dimmed(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "start_process" | "read_process_output" | "interact_with_process"
    )
}

fn tool_is_noisy(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "list_directory" | "search" | "list_processes" | "list_sessions"
    )
}

fn dim_text(text: &str) -> String {
    format!("\x1b[2m{}\x1b[0m", text)
}

pub fn indent_lines(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut lines = text.lines();
    let Some(first) = lines.next() else {
        return String::new();
    };
    let mut output = format!("  └ {}", first);
    for line in lines {
        output.push_str(&format!("\n    {}", line));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    mod indent_lines_tests {
        use super::*;

        #[test]
        fn empty_string_returns_empty() {
            assert_eq!(indent_lines(""), "");
        }

        #[test]
        fn single_line_gets_prefix() {
            assert_eq!(indent_lines("hello"), "  └ hello");
        }

        #[test]
        fn multiline_indents_continuation() {
            let input = "line1\nline2\nline3";
            let expected = "  └ line1\n    line2\n    line3";
            assert_eq!(indent_lines(input), expected);
        }

        #[test]
        fn handles_empty_lines_in_middle() {
            let input = "line1\n\nline3";
            let expected = "  └ line1\n    \n    line3";
            assert_eq!(indent_lines(input), expected);
        }

        #[test]
        fn preserves_existing_indentation() {
            let input = "func() {\n    body\n}";
            let expected = "  └ func() {\n        body\n    }";
            assert_eq!(indent_lines(input), expected);
        }
    }

    mod tool_classification_tests {
        use super::*;

        #[test]
        fn long_running_tools_identified() {
            assert!(tool_is_long_running("start_process"));
            assert!(tool_is_long_running("read_process_output"));
            assert!(tool_is_long_running("interact_with_process"));
            assert!(tool_is_long_running("list_processes"));
            assert!(tool_is_long_running("list_sessions"));
            assert!(tool_is_long_running("search"));
            assert!(tool_is_long_running("fetch"));
            assert!(tool_is_long_running("list_directory"));
        }

        #[test]
        fn non_long_running_tools_not_flagged() {
            assert!(!tool_is_long_running("read_file"));
            assert!(!tool_is_long_running("write_file"));
            assert!(!tool_is_long_running("unknown_tool"));
        }

        #[test]
        fn dimmed_tools_identified() {
            assert!(tool_is_dimmed("start_process"));
            assert!(tool_is_dimmed("read_process_output"));
            assert!(tool_is_dimmed("interact_with_process"));
        }

        #[test]
        fn non_dimmed_tools_not_flagged() {
            assert!(!tool_is_dimmed("read_file"));
            assert!(!tool_is_dimmed("search"));
            assert!(!tool_is_dimmed("fetch"));
        }

        #[test]
        fn noisy_tools_identified() {
            assert!(tool_is_noisy("list_directory"));
            assert!(tool_is_noisy("search"));
            assert!(tool_is_noisy("list_processes"));
            assert!(tool_is_noisy("list_sessions"));
        }

        #[test]
        fn non_noisy_tools_not_flagged() {
            assert!(!tool_is_noisy("read_file"));
            assert!(!tool_is_noisy("fetch"));
            assert!(!tool_is_noisy("start_process"));
        }
    }

    mod dim_text_tests {
        use super::*;

        #[test]
        fn wraps_text_with_ansi_codes() {
            let result = dim_text("hello");
            assert_eq!(result, "\x1b[2mhello\x1b[0m");
        }

        #[test]
        fn handles_empty_string() {
            let result = dim_text("");
            assert_eq!(result, "\x1b[2m\x1b[0m");
        }

        #[test]
        fn handles_multiline_text() {
            let result = dim_text("line1\nline2");
            assert_eq!(result, "\x1b[2mline1\nline2\x1b[0m");
        }
    }

    mod should_print_start_tests {
        use super::*;

        #[test]
        fn verbose_always_prints() {
            assert!(should_print_start(
                "any_tool",
                &ToolApprovalStatus::AutoApproved,
                Verbosity::Verbose
            ));
            assert!(should_print_start(
                "read_file",
                &ToolApprovalStatus::AutoApproved,
                Verbosity::Verbose
            ));
        }

        #[test]
        fn quiet_never_prints() {
            assert!(!should_print_start(
                "start_process",
                &ToolApprovalStatus::UserApproved,
                Verbosity::Quiet
            ));
            assert!(!should_print_start(
                "search",
                &ToolApprovalStatus::AutoApproved,
                Verbosity::Quiet
            ));
        }

        #[test]
        fn normal_prints_long_running() {
            assert!(should_print_start(
                "start_process",
                &ToolApprovalStatus::AutoApproved,
                Verbosity::Normal
            ));
            assert!(should_print_start(
                "search",
                &ToolApprovalStatus::AutoApproved,
                Verbosity::Normal
            ));
        }

        #[test]
        fn normal_prints_user_approved() {
            assert!(should_print_start(
                "read_file",
                &ToolApprovalStatus::UserApproved,
                Verbosity::Normal
            ));
        }

        #[test]
        fn normal_skips_auto_approved_short_tools() {
            assert!(!should_print_start(
                "read_file",
                &ToolApprovalStatus::AutoApproved,
                Verbosity::Normal
            ));
        }
    }

    mod format_tool_input_tests {
        use super::*;

        #[test]
        fn quiet_returns_none() {
            assert!(format_tool_input("any_tool", "content", Verbosity::Quiet).is_none());
        }

        #[test]
        fn verbose_always_returns_content() {
            let result = format_tool_input("list_directory", "content", Verbosity::Verbose);
            assert_eq!(result, Some("content".to_string()));
        }

        #[test]
        fn normal_filters_noisy_tools() {
            assert!(format_tool_input("list_directory", "content", Verbosity::Normal).is_none());
            assert!(format_tool_input("search", "content", Verbosity::Normal).is_none());
        }

        #[test]
        fn normal_shows_non_noisy_tools() {
            let result = format_tool_input("read_file", "content", Verbosity::Normal);
            assert_eq!(result, Some("content".to_string()));
        }
    }

    mod format_tool_output_tests {
        use super::*;

        #[test]
        fn quiet_returns_none() {
            assert!(format_tool_output("any_tool", "content", Verbosity::Quiet).is_none());
        }

        #[test]
        fn verbose_returns_content_as_is() {
            let result = format_tool_output("start_process", "output", Verbosity::Verbose);
            assert_eq!(result, Some("output".to_string()));
        }

        #[test]
        fn normal_dims_dimmed_tools() {
            let result = format_tool_output("start_process", "output", Verbosity::Normal);
            assert_eq!(result, Some("\x1b[2moutput\x1b[0m".to_string()));
        }

        #[test]
        fn normal_does_not_dim_other_tools() {
            let result = format_tool_output("read_file", "output", Verbosity::Normal);
            assert_eq!(result, Some("output".to_string()));
        }

        #[test]
        fn empty_output_returns_none() {
            assert!(format_tool_output("read_file", "", Verbosity::Normal).is_none());
            assert!(format_tool_output("read_file", "   ", Verbosity::Normal).is_none());
            assert!(format_tool_output("read_file", "\n\t  ", Verbosity::Normal).is_none());
        }

        #[test]
        fn whitespace_only_dimmed_returns_none() {
            // Even dimmed whitespace should return None
            assert!(format_tool_output("start_process", "  ", Verbosity::Normal).is_none());
        }
    }

    mod presentation_hook_tests {
        use super::*;
        use crate::repl::formatter::ToolFormatter;
        use mixtape_core::ToolResult;
        use serde_json::{json, Value};
        use std::time::Instant;

        /// Mock formatter for testing PresentationHook
        struct MockFormatter {
            input_result: Option<String>,
            output_result: Option<String>,
        }

        impl MockFormatter {
            fn new() -> Self {
                Self {
                    input_result: None,
                    output_result: None,
                }
            }

            fn with_input(mut self, result: Option<&str>) -> Self {
                self.input_result = result.map(String::from);
                self
            }

            fn with_output(mut self, result: Option<&str>) -> Self {
                self.output_result = result.map(String::from);
                self
            }
        }

        impl ToolFormatter for MockFormatter {
            fn format_tool_input(
                &self,
                _name: &str,
                _input: &Value,
                _display: Display,
            ) -> Option<String> {
                self.input_result.clone()
            }

            fn format_tool_output(
                &self,
                _name: &str,
                _output: &ToolResult,
                _display: Display,
            ) -> Option<String> {
                self.output_result.clone()
            }
        }

        fn create_hook(
            formatter: MockFormatter,
            verbosity: Verbosity,
        ) -> PresentationHook<MockFormatter> {
            PresentationHook::new(Arc::new(formatter), Arc::new(Mutex::new(verbosity)))
        }

        fn tool_started_event(name: &str, approval: ToolApprovalStatus) -> AgentEvent {
            AgentEvent::ToolStarted {
                id: "test-id".to_string(),
                name: name.to_string(),
                input: json!({"query": "test"}),
                approval_status: approval,
                timestamp: Instant::now(),
            }
        }

        fn tool_completed_event(name: &str) -> AgentEvent {
            AgentEvent::ToolCompleted {
                id: "test-id".to_string(),
                name: name.to_string(),
                output: ToolResult::Text("result".to_string()),
                approval_status: ToolApprovalStatus::AutoApproved,
                duration: std::time::Duration::from_millis(100),
            }
        }

        fn tool_failed_event(name: &str, error: &str) -> AgentEvent {
            AgentEvent::ToolFailed {
                id: "test-id".to_string(),
                name: name.to_string(),
                error: error.to_string(),
                duration: std::time::Duration::from_millis(50),
            }
        }

        // Tests for PresentationHook construction
        #[test]
        fn hook_can_be_created_with_mock_formatter() {
            let hook = create_hook(MockFormatter::new(), Verbosity::Normal);
            // Just verify it compiles and creates successfully
            assert!(Arc::strong_count(&hook.formatter) >= 1);
        }

        // Tests for ToolStarted event handling
        #[test]
        fn tool_started_quiet_mode_does_not_panic() {
            let hook = create_hook(
                MockFormatter::new().with_input(Some("formatted input")),
                Verbosity::Quiet,
            );
            // In quiet mode, should return early without printing
            hook.on_event(&tool_started_event(
                "search",
                ToolApprovalStatus::AutoApproved,
            ));
        }

        #[test]
        fn tool_started_long_running_tool_processes_event() {
            let hook = create_hook(
                MockFormatter::new().with_input(Some("query: test")),
                Verbosity::Normal,
            );
            // Long-running tools should be processed in Normal mode
            hook.on_event(&tool_started_event(
                "search",
                ToolApprovalStatus::AutoApproved,
            ));
        }

        #[test]
        fn tool_started_user_approved_processes_event() {
            let hook = create_hook(
                MockFormatter::new().with_input(Some("file: test.txt")),
                Verbosity::Normal,
            );
            // User approved tools show the approval badge
            hook.on_event(&tool_started_event(
                "read_file",
                ToolApprovalStatus::UserApproved,
            ));
        }

        #[test]
        fn tool_started_short_tool_auto_approved_skipped() {
            let hook = create_hook(
                MockFormatter::new().with_input(Some("input")),
                Verbosity::Normal,
            );
            // Short tools that are auto-approved should be skipped
            hook.on_event(&tool_started_event(
                "read_file",
                ToolApprovalStatus::AutoApproved,
            ));
        }

        #[test]
        fn tool_started_verbose_always_processes() {
            let hook = create_hook(
                MockFormatter::new().with_input(Some("any input")),
                Verbosity::Verbose,
            );
            // Verbose mode processes everything
            hook.on_event(&tool_started_event(
                "any_tool",
                ToolApprovalStatus::AutoApproved,
            ));
        }

        #[test]
        fn tool_started_with_none_formatted_and_not_approved_skips() {
            let hook = create_hook(MockFormatter::new().with_input(None), Verbosity::Verbose);
            // If formatter returns None AND not user approved, should skip printing body
            hook.on_event(&tool_started_event(
                "search",
                ToolApprovalStatus::AutoApproved,
            ));
        }

        // Tests for ToolCompleted event handling
        #[test]
        fn tool_completed_quiet_mode_prints_minimal() {
            let hook = create_hook(
                MockFormatter::new().with_output(Some("output")),
                Verbosity::Quiet,
            );
            // Quiet mode prints just the checkmark
            hook.on_event(&tool_completed_event("read_file"));
        }

        #[test]
        fn tool_completed_normal_mode_with_output() {
            let hook = create_hook(
                MockFormatter::new().with_output(Some("file contents here")),
                Verbosity::Normal,
            );
            hook.on_event(&tool_completed_event("read_file"));
        }

        #[test]
        fn tool_completed_normal_mode_no_output() {
            let hook = create_hook(MockFormatter::new().with_output(None), Verbosity::Normal);
            // Should print "(completed)" when no output
            hook.on_event(&tool_completed_event("read_file"));
        }

        #[test]
        fn tool_completed_verbose_mode() {
            let hook = create_hook(
                MockFormatter::new().with_output(Some("detailed output")),
                Verbosity::Verbose,
            );
            hook.on_event(&tool_completed_event("any_tool"));
        }

        #[test]
        fn tool_completed_dimmed_tool_output() {
            let hook = create_hook(
                MockFormatter::new().with_output(Some("process output")),
                Verbosity::Normal,
            );
            // start_process is a dimmed tool
            hook.on_event(&tool_completed_event("start_process"));
        }

        // Tests for ToolFailed event handling
        #[test]
        fn tool_failed_prints_error() {
            let hook = create_hook(MockFormatter::new(), Verbosity::Normal);
            hook.on_event(&tool_failed_event("read_file", "File not found"));
        }

        #[test]
        fn tool_failed_quiet_mode_still_prints() {
            let hook = create_hook(MockFormatter::new(), Verbosity::Quiet);
            // Errors should always be visible, even in quiet mode
            hook.on_event(&tool_failed_event("read_file", "Permission denied"));
        }

        // Tests for other event types
        #[test]
        fn other_events_are_ignored() {
            let hook = create_hook(MockFormatter::new(), Verbosity::Normal);
            // These events should be silently ignored (not Tool* events)
            hook.on_event(&AgentEvent::RunStarted {
                input: "test".to_string(),
                timestamp: Instant::now(),
            });
            hook.on_event(&AgentEvent::RunCompleted {
                output: "done".to_string(),
                duration: std::time::Duration::from_secs(1),
            });
        }

        // Test that hook implements AgentHook trait
        #[test]
        fn hook_implements_agent_hook() {
            let hook = create_hook(MockFormatter::new(), Verbosity::Normal);
            // This compiles because PresentationHook<MockFormatter> implements AgentHook
            let _: &dyn AgentHook = &hook;
        }
    }
}
