use crate::prelude::*;
use crate::process::start_process::SESSION_MANAGER;

/// Input for interacting with a process
#[derive(Debug, Deserialize, JsonSchema)]
pub struct InteractWithProcessInput {
    /// Process ID to interact with
    pub pid: u32,

    /// Input to send to the process (will be followed by a newline)
    pub input: String,

    /// Wait for response after sending input (default: true)
    #[serde(default = "default_wait")]
    pub wait_for_response: bool,

    /// Maximum time to wait for response in milliseconds (default: 5000)
    #[serde(default = "default_response_timeout")]
    pub response_timeout_ms: u64,
}

fn default_wait() -> bool {
    true
}

fn default_response_timeout() -> u64 {
    5000
}

/// Tool for sending input to a running process
pub struct InteractWithProcessTool;

impl Tool for InteractWithProcessTool {
    type Input = InteractWithProcessInput;

    fn name(&self) -> &str {
        "interact_with_process"
    }

    fn description(&self) -> &str {
        "Send input to a running process and optionally wait for its response. Useful for interactive programs."
    }

    fn format_output_plain(&self, result: &ToolResult) -> String {
        let text = result.as_text();
        let (pid, input_sent, status, response) = parse_interact_output(&text);

        let mut out = String::new();
        out.push_str(&"─".repeat(50));
        out.push('\n');
        if let Some(p) = pid {
            out.push_str(&format!("  Process {} ", p));
        }
        if let Some(s) = status {
            out.push_str(&format!("[{}]", s));
        }
        out.push_str(&format!("\n{}\n", "─".repeat(50)));
        if let Some(cmd) = input_sent {
            out.push_str(&format!("  >>> {}\n", cmd));
        }
        if !response.is_empty() {
            out.push_str(&"─".repeat(50));
            out.push('\n');
            for line in response {
                out.push_str(&format!("  {}\n", line));
            }
        }
        out
    }

    fn format_output_ansi(&self, result: &ToolResult) -> String {
        let text = result.as_text();
        let (pid, input_sent, status, response) = parse_interact_output(&text);

        let mut out = String::new();
        out.push_str(&format!("\x1b[2m{}\x1b[0m\n", "─".repeat(50)));

        let (icon, status_color) = match status {
            Some(s) if s.contains("Running") => ("\x1b[32m●\x1b[0m", "\x1b[32m"),
            Some(s) if s.contains("Completed") => ("\x1b[34m●\x1b[0m", "\x1b[34m"),
            Some(s) if s.contains("Waiting") => ("\x1b[33m●\x1b[0m", "\x1b[33m"),
            _ => ("\x1b[2m●\x1b[0m", "\x1b[2m"),
        };

        out.push_str(&format!("  {} ", icon));
        if let Some(p) = pid {
            out.push_str(&format!("\x1b[1mProcess {}\x1b[0m ", p));
        }
        if let Some(s) = status {
            out.push_str(&format!("{}{}\x1b[0m", status_color, s));
        }
        out.push_str(&format!("\n\x1b[2m{}\x1b[0m\n", "─".repeat(50)));
        if let Some(cmd) = input_sent {
            out.push_str(&format!("  \x1b[33m>>>\x1b[0m \x1b[36m{}\x1b[0m\n", cmd));
        }
        if !response.is_empty() {
            out.push_str(&format!("\x1b[2m{}\x1b[0m\n", "─".repeat(50)));
            for line in response {
                out.push_str(&format!("  \x1b[2m│\x1b[0m {}\n", line));
            }
        }
        out
    }

    fn format_output_markdown(&self, result: &ToolResult) -> String {
        let text = result.as_text();
        let (pid, input_sent, status, response) = parse_interact_output(&text);

        let mut out = String::new();
        let status_emoji = match status {
            Some(s) if s.contains("Running") => "🟢",
            Some(s) if s.contains("Completed") => "🔵",
            Some(s) if s.contains("Waiting") => "🟡",
            _ => "⚪",
        };

        if let Some(p) = pid {
            out.push_str(&format!("### {} Process {}", status_emoji, p));
        }
        if let Some(s) = status {
            out.push_str(&format!(" - {}", s));
        }
        out.push_str("\n\n");
        if let Some(cmd) = input_sent {
            out.push_str(&format!("**Input:** `{}`\n\n", cmd));
        }
        if !response.is_empty() {
            out.push_str("**Response:**\n```\n");
            for line in response {
                out.push_str(line);
                out.push('\n');
            }
            out.push_str("```\n");
        }
        out
    }

    async fn execute(&self, input: Self::Input) -> std::result::Result<ToolResult, ToolError> {
        use crate::process::session_manager::ProcessState;

        let manager = SESSION_MANAGER.lock().await;

        // Send input
        manager.send_input(input.pid, &input.input).await?;

        if !input.wait_for_response {
            return Ok(format!("Sent input to process {}: {}", input.pid, input.input).into());
        }

        // Clear buffer before waiting
        let _ = manager.read_output(input.pid, true).await;

        drop(manager);

        // Poll for response with early exit on prompt detection
        let timeout_ms = input.response_timeout_ms.min(10000);
        let poll_interval_ms = 50;
        let max_polls = timeout_ms / poll_interval_ms;
        let mut exit_reason = "timeout";

        for _ in 0..max_polls {
            tokio::time::sleep(tokio::time::Duration::from_millis(poll_interval_ms)).await;

            let manager = SESSION_MANAGER.lock().await;
            let status = manager.check_status(input.pid).await?;

            match status {
                ProcessState::WaitingForInput => {
                    exit_reason = "prompt_detected";
                    break;
                }
                ProcessState::Completed { .. } => {
                    exit_reason = "process_exited";
                    break;
                }
                ProcessState::TimedOut => {
                    exit_reason = "process_timeout";
                    break;
                }
                ProcessState::Running => {
                    // Continue polling
                }
            }
        }

        let manager = SESSION_MANAGER.lock().await;
        let output = manager.read_output(input.pid, false).await?;
        let status = manager.check_status(input.pid).await?;

        let content = format!(
            "Sent to process {}: {}\nStatus: {:?} ({})\n\nResponse ({} lines):\n{}",
            input.pid,
            input.input,
            status,
            exit_reason,
            output.len(),
            output.join("\n")
        );

        Ok(content.into())
    }
}

/// Parse interact output
fn parse_interact_output(output: &str) -> (Option<&str>, Option<&str>, Option<&str>, Vec<&str>) {
    let mut pid = None;
    let mut input_sent = None;
    let mut status = None;
    let mut response_lines = Vec::new();
    let mut in_response = false;

    for line in output.lines() {
        if line.starts_with("Sent to process ") {
            // "Sent to process 1234: command"
            let rest = line.trim_start_matches("Sent to process ");
            if let Some(colon_idx) = rest.find(':') {
                pid = Some(&rest[..colon_idx]);
                input_sent = Some(rest[colon_idx + 1..].trim());
            }
        } else if line.starts_with("Sent input to process ") {
            // Short form: "Sent input to process 1234: command"
            let rest = line.trim_start_matches("Sent input to process ");
            if let Some(colon_idx) = rest.find(':') {
                pid = Some(&rest[..colon_idx]);
                input_sent = Some(rest[colon_idx + 1..].trim());
            }
        } else if line.starts_with("Status:") {
            status = Some(line.trim_start_matches("Status:").trim());
        } else if line.starts_with("Response (") {
            in_response = true;
        } else if in_response {
            response_lines.push(line);
        }
    }

    (pid, input_sent, status, response_lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::start_process::{StartProcessInput, StartProcessTool};
    use mixtape_core::ToolResult;

    #[tokio::test]
    async fn test_interact_with_process_nonexistent() {
        let tool = InteractWithProcessTool;

        let input = InteractWithProcessInput {
            pid: 99999999,
            input: "test".to_string(),
            wait_for_response: false,
            response_timeout_ms: 100,
        };

        let result = tool.execute(input).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_interact_with_process_no_wait() {
        // Start a process
        let start_tool = StartProcessTool;
        let start_input = StartProcessInput {
            command: "cat".to_string(), // cat reads stdin
            timeout_ms: Some(5000),
            shell: None,
        };

        let start_result = start_tool.execute(start_input).await;
        if start_result.is_err() {
            // Skip if process creation fails
            return;
        }

        let start_output = start_result.unwrap().as_text();
        // Extract PID
        if let Some(pid_line) = start_output.lines().find(|l| l.contains("PID:")) {
            if let Some(pid_str) = pid_line.split(':').nth(1) {
                if let Ok(pid) = pid_str.trim().parse::<u32>() {
                    // Send input without waiting for response
                    let interact_tool = InteractWithProcessTool;
                    let interact_input = InteractWithProcessInput {
                        pid,
                        input: "hello".to_string(),
                        wait_for_response: false,
                        response_timeout_ms: 100,
                    };

                    let result = interact_tool.execute(interact_input).await;
                    assert!(result.is_ok());
                    let output = result.unwrap().as_text();
                    assert!(output.contains("Sent input to process"));
                    return;
                }
            }
        }
    }

    #[tokio::test]
    async fn test_interact_with_process_with_wait() {
        // Start an interactive cat process
        let start_tool = StartProcessTool;
        let start_input = StartProcessInput {
            command: "cat".to_string(),
            timeout_ms: Some(5000),
            shell: None,
        };

        let start_result = start_tool.execute(start_input).await;
        if start_result.is_err() {
            return;
        }

        let start_output = start_result.unwrap().as_text();
        if let Some(pid_line) = start_output.lines().find(|l| l.contains("PID:")) {
            if let Some(pid_str) = pid_line.split(':').nth(1) {
                if let Ok(pid) = pid_str.trim().parse::<u32>() {
                    // Send input and wait for response
                    let interact_tool = InteractWithProcessTool;
                    let interact_input = InteractWithProcessInput {
                        pid,
                        input: "echo test".to_string(),
                        wait_for_response: true,
                        response_timeout_ms: 500,
                    };

                    let result = interact_tool.execute(interact_input).await;
                    assert!(result.is_ok());
                    let output = result.unwrap().as_text();
                    assert!(output.contains("Sent to process"));
                    assert!(output.contains("Response"));
                    return;
                }
            }
        }
    }

    // ==================== parse_interact_output tests ====================

    #[test]
    fn test_parse_interact_output_complete() {
        let output = "Sent to process 12345: hello\nStatus: Running (prompt_detected)\n\nResponse (2 lines):\nworld\nmore";
        let (pid, input_sent, status, lines) = parse_interact_output(output);

        assert_eq!(pid, Some("12345"));
        assert_eq!(input_sent, Some("hello"));
        assert_eq!(status, Some("Running (prompt_detected)"));
        assert_eq!(lines, vec!["world", "more"]);
    }

    #[test]
    fn test_parse_interact_output_short_form() {
        let output = "Sent input to process 12345: test command";
        let (pid, input_sent, status, lines) = parse_interact_output(output);

        assert_eq!(pid, Some("12345"));
        assert_eq!(input_sent, Some("test command"));
        assert_eq!(status, None);
        assert!(lines.is_empty());
    }

    #[test]
    fn test_parse_interact_output_empty() {
        let output = "";
        let (pid, input_sent, status, lines) = parse_interact_output(output);

        assert_eq!(pid, None);
        assert_eq!(input_sent, None);
        assert_eq!(status, None);
        assert!(lines.is_empty());
    }

    #[test]
    fn test_parse_interact_output_with_multiline_response() {
        let output = "Sent to process 1: cmd\nStatus: Completed\n\nResponse (3 lines):\na\nb\nc";
        let (_, _, _, lines) = parse_interact_output(output);

        assert_eq!(lines.len(), 3);
        assert_eq!(lines, vec!["a", "b", "c"]);
    }

    // ==================== format_output tests ====================

    #[test]
    fn test_format_output_plain_basic() {
        let tool = InteractWithProcessTool;
        let result: ToolResult =
            "Sent to process 12345: hello\nStatus: Running\n\nResponse (1 lines):\nworld".into();

        let formatted = tool.format_output_plain(&result);

        assert!(formatted.contains("Process 12345"));
        assert!(formatted.contains(">>> hello"));
        assert!(formatted.contains("world"));
    }

    #[test]
    fn test_format_output_plain_no_response() {
        let tool = InteractWithProcessTool;
        let result: ToolResult = "Sent input to process 12345: test".into();

        let formatted = tool.format_output_plain(&result);

        assert!(formatted.contains("Process 12345"));
        assert!(formatted.contains(">>> test"));
    }

    #[test]
    fn test_format_output_ansi_running() {
        let tool = InteractWithProcessTool;
        let result: ToolResult =
            "Sent to process 12345: hello\nStatus: Running\n\nResponse (1 lines):\nworld".into();

        let formatted = tool.format_output_ansi(&result);

        assert!(formatted.contains("\x1b[")); // ANSI codes
        assert!(formatted.contains("\x1b[32m")); // green for running
        assert!(formatted.contains("\x1b[33m")); // yellow for >>>
        assert!(formatted.contains("\x1b[36m")); // cyan for input
    }

    #[test]
    fn test_format_output_ansi_completed() {
        let tool = InteractWithProcessTool;
        let result: ToolResult =
            "Sent to process 12345: hello\nStatus: Completed\n\nResponse (1 lines):\ndone".into();

        let formatted = tool.format_output_ansi(&result);

        assert!(formatted.contains("\x1b[34m")); // blue for completed
    }

    #[test]
    fn test_format_output_markdown_with_response() {
        let tool = InteractWithProcessTool;
        let result: ToolResult =
            "Sent to process 12345: hello\nStatus: Running\n\nResponse (2 lines):\nline1\nline2"
                .into();

        let formatted = tool.format_output_markdown(&result);

        assert!(formatted.contains("### 🟢 Process 12345"));
        assert!(formatted.contains("**Input:** `hello`"));
        assert!(formatted.contains("**Response:**"));
        assert!(formatted.contains("```"));
    }

    #[test]
    fn test_format_output_markdown_status_emojis() {
        let tool = InteractWithProcessTool;

        // Running
        let running: ToolResult =
            "Sent to process 1: x\nStatus: Running\n\nResponse (0 lines):".into();
        assert!(tool.format_output_markdown(&running).contains("🟢"));

        // Completed
        let completed: ToolResult =
            "Sent to process 1: x\nStatus: Completed\n\nResponse (0 lines):".into();
        assert!(tool.format_output_markdown(&completed).contains("🔵"));

        // Waiting
        let waiting: ToolResult =
            "Sent to process 1: x\nStatus: WaitingForInput\n\nResponse (0 lines):".into();
        assert!(tool.format_output_markdown(&waiting).contains("🟡"));
    }

    // ==================== default value tests ====================

    #[test]
    fn test_default_wait() {
        assert!(default_wait());
    }

    #[test]
    fn test_default_response_timeout() {
        assert_eq!(default_response_timeout(), 5000);
    }

    // ==================== Tool metadata tests ====================

    #[test]
    fn test_tool_name() {
        let tool = InteractWithProcessTool;
        assert_eq!(tool.name(), "interact_with_process");
    }

    #[test]
    fn test_tool_description() {
        let tool = InteractWithProcessTool;
        assert!(!tool.description().is_empty());
        assert!(tool.description().contains("input"));
    }
}
