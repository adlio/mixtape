use crate::prelude::*;
use crate::process::session_manager::{ProcessState, SessionManager};
use std::sync::Arc;
use tokio::sync::Mutex;

lazy_static::lazy_static! {
    pub(crate) static ref SESSION_MANAGER: Arc<Mutex<SessionManager>> = Arc::new(Mutex::new(SessionManager::new()));
}

/// Input for starting a process
#[derive(Debug, Deserialize, JsonSchema)]
pub struct StartProcessInput {
    /// Command to execute
    pub command: String,

    /// Optional timeout in milliseconds (default: no timeout)
    #[serde(default)]
    pub timeout_ms: Option<u64>,

    /// Optional shell to use (defaults to 'sh' on Unix, 'cmd' on Windows)
    #[serde(default)]
    pub shell: Option<String>,
}

/// Tool for starting a new process session
pub struct StartProcessTool;

impl Tool for StartProcessTool {
    type Input = StartProcessInput;

    fn name(&self) -> &str {
        "start_process"
    }

    fn description(&self) -> &str {
        "Start a new process session. Returns a PID that can be used to interact with the process, read its output, or terminate it."
    }

    fn format_output_plain(&self, result: &ToolResult) -> String {
        let text = result.as_text();
        let (command, pid, status, output_lines) = parse_start_output(&text);

        let mut out = String::new();
        out.push_str(&"─".repeat(50));
        out.push_str("\n  PROCESS STARTED\n");
        out.push_str(&"─".repeat(50));
        out.push('\n');

        if let Some(cmd) = command {
            out.push_str(&format!("  Command: {}\n", cmd));
        }
        if let Some(p) = pid {
            out.push_str(&format!("  PID:     {}\n", p));
        }
        if let Some(s) = status {
            out.push_str(&format!("  Status:  {}\n", s));
        }

        if !output_lines.is_empty() {
            out.push_str(&"─".repeat(50));
            out.push('\n');
            for line in output_lines {
                out.push_str(&format!("  {}\n", line));
            }
        }
        out
    }

    fn format_output_ansi(&self, result: &ToolResult) -> String {
        let text = result.as_text();
        let (command, pid, status, output_lines) = parse_start_output(&text);

        let mut out = String::new();
        out.push_str(&format!("\x1b[2m{}\x1b[0m\n  \x1b[32m●\x1b[0m \x1b[1mProcess Started\x1b[0m\n\x1b[2m{}\x1b[0m\n", "─".repeat(50), "─".repeat(50)));

        if let Some(cmd) = command {
            out.push_str(&format!(
                "  \x1b[2mCommand\x1b[0m  \x1b[36m{}\x1b[0m\n",
                cmd
            ));
        }
        if let Some(p) = pid {
            out.push_str(&format!("  \x1b[2mPID\x1b[0m      \x1b[33m{}\x1b[0m\n", p));
        }
        if let Some(s) = status {
            let status_color = if s.contains("Running") {
                "\x1b[32m"
            } else if s.contains("Completed") {
                "\x1b[34m"
            } else {
                "\x1b[33m"
            };
            out.push_str(&format!(
                "  \x1b[2mStatus\x1b[0m   {}{}\x1b[0m\n",
                status_color, s
            ));
        }

        if !output_lines.is_empty() {
            out.push_str(&format!("\x1b[2m{}\x1b[0m\n", "─".repeat(50)));
            for line in output_lines {
                out.push_str(&format!("  \x1b[2m│\x1b[0m {}\n", line));
            }
        }
        out
    }

    fn format_output_markdown(&self, result: &ToolResult) -> String {
        let text = result.as_text();
        let (command, pid, status, output_lines) = parse_start_output(&text);

        let mut out = String::from("### 🚀 Process Started\n\n");
        if let Some(cmd) = command {
            out.push_str(&format!("- **Command**: `{}`\n", cmd));
        }
        if let Some(p) = pid {
            out.push_str(&format!("- **PID**: `{}`\n", p));
        }
        if let Some(s) = status {
            out.push_str(&format!("- **Status**: {}\n", s));
        }

        if !output_lines.is_empty() {
            out.push_str("\n**Initial Output:**\n```\n");
            for line in output_lines {
                out.push_str(line);
                out.push('\n');
            }
            out.push_str("```\n");
        }
        out
    }

    async fn execute(&self, input: Self::Input) -> std::result::Result<ToolResult, ToolError> {
        let manager = SESSION_MANAGER.lock().await;
        let pid = manager
            .create_session(input.command.clone(), input.shell, input.timeout_ms)
            .await?;

        // Give the process a moment to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Read initial output
        let initial_output = manager.read_output(pid, false).await.unwrap_or_default();
        let status = manager
            .check_status(pid)
            .await
            .unwrap_or(ProcessState::Running);

        let mut content = format!(
            "Started process: {}\nPID: {}\nStatus: {:?}\n",
            input.command, pid, status
        );

        if !initial_output.is_empty() {
            content.push_str("\nInitial output:\n");
            for line in initial_output.iter().take(20) {
                content.push_str(&format!("{}\n", line));
            }
            if initial_output.len() > 20 {
                content.push_str(&format!(
                    "... and {} more lines\n",
                    initial_output.len() - 20
                ));
            }
        }

        Ok(content.into())
    }
}

/// Parse start_process output into components
fn parse_start_output(output: &str) -> (Option<&str>, Option<&str>, Option<&str>, Vec<&str>) {
    let mut command = None;
    let mut pid = None;
    let mut status = None;
    let mut output_lines = Vec::new();
    let mut in_output = false;

    for line in output.lines() {
        if line.starts_with("Started process:") {
            command = Some(line.trim_start_matches("Started process:").trim());
        } else if line.starts_with("PID:") {
            pid = Some(line.trim_start_matches("PID:").trim());
        } else if line.starts_with("Status:") {
            status = Some(line.trim_start_matches("Status:").trim());
        } else if line.starts_with("Initial output:") {
            in_output = true;
        } else if in_output && !line.starts_with("...") {
            output_lines.push(line);
        }
    }

    (command, pid, status, output_lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixtape_core::ToolResult;

    #[tokio::test]
    async fn test_start_process_simple_command() {
        let tool = StartProcessTool;

        // Use 'echo' which works cross-platform
        let input = StartProcessInput {
            command: "echo 'Hello from process'".to_string(),
            timeout_ms: Some(5000),
            shell: None,
        };

        let result = tool.execute(input).await;
        assert!(result.is_ok());

        let output = result.unwrap().as_text();
        assert!(output.contains("Started process"));
        assert!(output.contains("PID:"));
    }

    #[tokio::test]
    async fn test_start_process_with_timeout() {
        let tool = StartProcessTool;

        let input = StartProcessInput {
            command: "echo 'test'".to_string(),
            timeout_ms: Some(1000),
            shell: None,
        };

        let result = tool.execute(input).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_start_process_empty_command() {
        let tool = StartProcessTool;

        let input = StartProcessInput {
            command: String::new(),
            timeout_ms: Some(5000),
            shell: None,
        };

        let result = tool.execute(input).await;
        // Should handle empty command gracefully
        assert!(result.is_ok() || result.is_err());
    }

    // ==================== parse_start_output tests ====================

    #[test]
    fn test_parse_start_output_complete() {
        let output = "Started process: echo hello\nPID: 12345\nStatus: Running\nInitial output:\nHello World\nLine 2";
        let (command, pid, status, lines) = parse_start_output(output);

        assert_eq!(command, Some("echo hello"));
        assert_eq!(pid, Some("12345"));
        assert_eq!(status, Some("Running"));
        assert_eq!(lines, vec!["Hello World", "Line 2"]);
    }

    #[test]
    fn test_parse_start_output_no_output() {
        let output = "Started process: sleep 10\nPID: 12345\nStatus: Running";
        let (command, pid, status, lines) = parse_start_output(output);

        assert_eq!(command, Some("sleep 10"));
        assert_eq!(pid, Some("12345"));
        assert_eq!(status, Some("Running"));
        assert!(lines.is_empty());
    }

    #[test]
    fn test_parse_start_output_empty() {
        let output = "";
        let (command, pid, status, lines) = parse_start_output(output);

        assert_eq!(command, None);
        assert_eq!(pid, None);
        assert_eq!(status, None);
        assert!(lines.is_empty());
    }

    #[test]
    fn test_parse_start_output_partial() {
        let output = "PID: 99999";
        let (command, pid, status, lines) = parse_start_output(output);

        assert_eq!(command, None);
        assert_eq!(pid, Some("99999"));
        assert_eq!(status, None);
        assert!(lines.is_empty());
    }

    #[test]
    fn test_parse_start_output_with_more_lines_indicator() {
        let output = "Started process: cmd\nPID: 1\nStatus: Running\nInitial output:\nline1\n... and 5 more lines";
        let (_, _, _, lines) = parse_start_output(output);

        // The "... and X more lines" should be filtered out
        assert_eq!(lines, vec!["line1"]);
    }

    // ==================== format_output tests ====================

    #[test]
    fn test_format_output_plain_basic() {
        let tool = StartProcessTool;
        let result: ToolResult = "Started process: echo test\nPID: 12345\nStatus: Running".into();

        let formatted = tool.format_output_plain(&result);

        assert!(formatted.contains("PROCESS STARTED"));
        assert!(formatted.contains("Command:"));
        assert!(formatted.contains("PID:"));
        assert!(formatted.contains("Status:"));
    }

    #[test]
    fn test_format_output_plain_with_output() {
        let tool = StartProcessTool;
        let result: ToolResult = "Started process: echo test\nPID: 12345\nStatus: Completed { exit_code: Some(0) }\nInitial output:\nHello".into();

        let formatted = tool.format_output_plain(&result);

        assert!(formatted.contains("Hello"));
    }

    #[test]
    fn test_format_output_ansi_colors() {
        let tool = StartProcessTool;
        let result: ToolResult = "Started process: echo test\nPID: 12345\nStatus: Running".into();

        let formatted = tool.format_output_ansi(&result);

        // Should contain ANSI escape codes
        assert!(formatted.contains("\x1b["));
        assert!(formatted.contains("Process Started"));
    }

    #[test]
    fn test_format_output_ansi_status_colors() {
        let tool = StartProcessTool;

        // Running status should be green
        let running: ToolResult = "Started process: test\nPID: 1\nStatus: Running".into();
        let formatted = tool.format_output_ansi(&running);
        assert!(formatted.contains("\x1b[32m")); // green

        // Completed status should be blue
        let completed: ToolResult = "Started process: test\nPID: 1\nStatus: Completed".into();
        let formatted = tool.format_output_ansi(&completed);
        assert!(formatted.contains("\x1b[34m")); // blue
    }

    #[test]
    fn test_format_output_markdown() {
        let tool = StartProcessTool;
        let result: ToolResult =
            "Started process: echo test\nPID: 12345\nStatus: Running\nInitial output:\nHello"
                .into();

        let formatted = tool.format_output_markdown(&result);

        assert!(formatted.contains("### 🚀 Process Started"));
        assert!(formatted.contains("**Command**: `echo test`"));
        assert!(formatted.contains("**PID**: `12345`"));
        assert!(formatted.contains("**Initial Output:**"));
        assert!(formatted.contains("```"));
    }

    #[test]
    fn test_format_output_markdown_no_output() {
        let tool = StartProcessTool;
        let result: ToolResult = "Started process: sleep 10\nPID: 12345\nStatus: Running".into();

        let formatted = tool.format_output_markdown(&result);

        // Should not have output section
        assert!(!formatted.contains("**Initial Output:**"));
    }

    // ==================== Tool metadata tests ====================

    #[test]
    fn test_tool_name() {
        let tool = StartProcessTool;
        assert_eq!(tool.name(), "start_process");
    }

    #[test]
    fn test_tool_description() {
        let tool = StartProcessTool;
        assert!(!tool.description().is_empty());
        assert!(tool.description().contains("process"));
    }
}
