//! Tool presentation formatting for CLI output

use super::commands::Verbosity;
use super::formatter::ToolFormatter;
use mixtape_core::{Agent, AgentEvent, AgentHook, Display};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

const BOX_WIDTH: usize = 80;

/// Queue for tool events that need to be printed
pub type EventQueue = Arc<Mutex<VecDeque<AgentEvent>>>;

/// Create a new event queue
pub fn new_event_queue() -> EventQueue {
    Arc::new(Mutex::new(VecDeque::new()))
}

/// Hook that queues tool events for later presentation
///
/// Events are queued rather than printed immediately, allowing the caller
/// to control when output appears (e.g., not during permission prompts).
pub struct PresentationHook {
    queue: EventQueue,
}

impl PresentationHook {
    pub fn new(queue: EventQueue) -> Self {
        Self { queue }
    }
}

impl AgentHook for PresentationHook {
    fn on_event(&self, event: &AgentEvent) {
        // Only queue tool-related events
        match event {
            AgentEvent::ToolRequested { .. }
            | AgentEvent::ToolExecuting { .. }
            | AgentEvent::ToolCompleted { .. }
            | AgentEvent::ToolFailed { .. } => {
                self.queue.lock().unwrap().push_back(event.clone());
            }
            _ => {}
        }
    }
}

/// Presenter that formats and prints queued events
pub struct EventPresenter<F: ToolFormatter = Agent> {
    formatter: Arc<F>,
    verbosity: Arc<Mutex<Verbosity>>,
    queue: EventQueue,
}

impl<F: ToolFormatter> EventPresenter<F> {
    pub fn new(formatter: Arc<F>, verbosity: Arc<Mutex<Verbosity>>, queue: EventQueue) -> Self {
        Self {
            formatter,
            verbosity,
            queue,
        }
    }

    /// Drain and print all queued events
    pub fn flush(&self) {
        let mut queue = self.queue.lock().unwrap();
        while let Some(event) = queue.pop_front() {
            self.print_event(&event);
        }
    }

    fn print_event(&self, event: &AgentEvent) {
        match event {
            AgentEvent::ToolRequested { name, input, .. } => {
                let verbosity = *self.verbosity.lock().unwrap();
                let formatted = self
                    .formatter
                    .format_tool_input(name, input, Display::Cli)
                    .and_then(|formatted| format_tool_input(name, &formatted, verbosity));

                print_tool_header(name);
                if let Some(output) = formatted {
                    for line in output.lines() {
                        println!("│  {}", line);
                    }
                }
            }
            AgentEvent::ToolExecuting { .. } => {
                // Optional: could show spinner for long-running tools
            }
            AgentEvent::ToolCompleted { name, output, .. } => {
                let verbosity = *self.verbosity.lock().unwrap();
                if verbosity == Verbosity::Quiet {
                    print_result_separator();
                    println!("│  \x1b[32m✓\x1b[0m");
                    print_tool_footer(name);
                    return;
                }
                print_result_separator();

                if let Some(formatted) =
                    self.formatter
                        .format_tool_output(name, output, Display::Cli)
                {
                    if let Some(output) = format_tool_output(name, &formatted, verbosity) {
                        for line in output.lines() {
                            println!("│  {}", line);
                        }
                    } else {
                        println!("│  \x1b[2m(no output)\x1b[0m");
                    }
                } else {
                    println!("│  \x1b[2m(no output)\x1b[0m");
                }
                print_tool_footer(name);
            }
            AgentEvent::ToolFailed { name, error, .. } => {
                print_result_separator();
                println!("│  \x1b[31m{}\x1b[0m", error);
                print_tool_footer(name);
            }
            _ => {}
        }
    }
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
    if formatted.trim().is_empty() {
        return None;
    }
    let output = if tool_is_dimmed(tool_name) {
        dim_text(formatted)
    } else {
        formatted.to_string()
    };
    Some(output)
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

/// Print tool header: ┌─ 🛠️  name ───...───┐
pub fn print_tool_header(name: &str) {
    let prefix = format!("┌─ 🛠️  {} ", name);
    let prefix_display_len = 6 + name.len() + 1; // ┌─ + space + emoji(2) + 2 spaces + name + space
    let fill = BOX_WIDTH.saturating_sub(prefix_display_len + 1);
    println!("\n{}{}┐", prefix, "─".repeat(fill));
    println!("│");
}

/// Print tool footer: └───...─── name ─┘
pub fn print_tool_footer(name: &str) {
    println!("│");
    let suffix = format!(" {} ─┘", name);
    let fill = BOX_WIDTH.saturating_sub(suffix.len() + 1);
    println!("└{}{}", "─".repeat(fill), suffix);
}

/// Print result separator with blank lines
pub fn print_result_separator() {
    println!("│");
    println!("├─ Result");
    println!("│");
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
            assert_eq!(dim_text("hello"), "\x1b[2mhello\x1b[0m");
        }

        #[test]
        fn handles_empty_string() {
            assert_eq!(dim_text(""), "\x1b[2m\x1b[0m");
        }

        #[test]
        fn handles_multiline_text() {
            assert_eq!(dim_text("line1\nline2"), "\x1b[2mline1\nline2\x1b[0m");
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
            assert_eq!(
                format_tool_input("list_directory", "content", Verbosity::Verbose),
                Some("content".to_string())
            );
        }

        #[test]
        fn normal_filters_noisy_tools() {
            assert!(format_tool_input("list_directory", "content", Verbosity::Normal).is_none());
        }

        #[test]
        fn normal_shows_non_noisy_tools() {
            assert_eq!(
                format_tool_input("read_file", "content", Verbosity::Normal),
                Some("content".to_string())
            );
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
            assert_eq!(
                format_tool_output("start_process", "output", Verbosity::Verbose),
                Some("output".to_string())
            );
        }

        #[test]
        fn normal_dims_dimmed_tools() {
            assert_eq!(
                format_tool_output("start_process", "output", Verbosity::Normal),
                Some("\x1b[2moutput\x1b[0m".to_string())
            );
        }

        #[test]
        fn normal_does_not_dim_other_tools() {
            assert_eq!(
                format_tool_output("read_file", "output", Verbosity::Normal),
                Some("output".to_string())
            );
        }

        #[test]
        fn empty_output_returns_none() {
            assert!(format_tool_output("read_file", "", Verbosity::Normal).is_none());
            assert!(format_tool_output("read_file", "   ", Verbosity::Normal).is_none());
        }

        #[test]
        fn whitespace_only_dimmed_returns_none() {
            assert!(format_tool_output("start_process", "  ", Verbosity::Normal).is_none());
        }
    }

    mod presentation_hook_tests {
        use super::*;
        use mixtape_core::ToolResult;
        use serde_json::json;
        use std::time::Instant;

        fn tool_requested_event(name: &str) -> AgentEvent {
            AgentEvent::ToolRequested {
                tool_use_id: "test-id".to_string(),
                name: name.to_string(),
                input: json!({"query": "test"}),
            }
        }

        fn tool_completed_event(name: &str) -> AgentEvent {
            AgentEvent::ToolCompleted {
                tool_use_id: "test-id".to_string(),
                name: name.to_string(),
                output: ToolResult::Text("result".to_string()),
                duration: std::time::Duration::from_millis(100),
            }
        }

        #[test]
        fn hook_queues_tool_events() {
            let queue = new_event_queue();
            let hook = PresentationHook::new(Arc::clone(&queue));

            hook.on_event(&tool_requested_event("test_tool"));
            hook.on_event(&tool_completed_event("test_tool"));

            assert_eq!(queue.lock().unwrap().len(), 2);
        }

        #[test]
        fn hook_ignores_non_tool_events() {
            let queue = new_event_queue();
            let hook = PresentationHook::new(Arc::clone(&queue));

            hook.on_event(&AgentEvent::RunStarted {
                input: "test".to_string(),
                timestamp: Instant::now(),
            });

            assert_eq!(queue.lock().unwrap().len(), 0);
        }

        #[test]
        fn hook_implements_agent_hook() {
            let queue = new_event_queue();
            let hook = PresentationHook::new(queue);
            let _: &dyn AgentHook = &hook;
        }
    }
}
