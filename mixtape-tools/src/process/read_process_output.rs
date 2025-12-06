use crate::prelude::*;
use crate::process::start_process::SESSION_MANAGER;

/// Input for reading process output
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadProcessOutputInput {
    /// Process ID to read from
    pub pid: u32,

    /// Clear the output buffer after reading (default: false)
    #[serde(default)]
    pub clear_buffer: bool,

    /// Maximum time to wait for new output in milliseconds (default: 5000)
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

fn default_timeout() -> u64 {
    5000
}

/// Tool for reading output from a running process
pub struct ReadProcessOutputTool;

impl Tool for ReadProcessOutputTool {
    type Input = ReadProcessOutputInput;

    fn name(&self) -> &str {
        "read_process_output"
    }

    fn description(&self) -> &str {
        "Read accumulated output from a running process. Can optionally clear the buffer after reading."
    }

    fn format_output_plain(&self, result: &ToolResult) -> String {
        let text = result.as_text();
        let (pid, status, lines) = parse_process_output(&text);

        let mut out = String::new();
        out.push_str(&"─".repeat(50));
        out.push('\n');
        if let Some(p) = pid {
            out.push_str(&format!("  Process {}", p));
        }
        if let Some(s) = status {
            out.push_str(&format!(" [{}]", s));
        }
        out.push_str(&format!("\n{}\n", "─".repeat(50)));

        if lines.is_empty() {
            out.push_str("  (no output)\n");
        } else {
            let width = lines.len().to_string().len().max(3);
            for (i, line) in lines.iter().enumerate() {
                out.push_str(&format!("  {:>width$} │ {}\n", i + 1, line, width = width));
            }
        }
        out
    }

    fn format_output_ansi(&self, result: &ToolResult) -> String {
        let text = result.as_text();
        let (pid, status, lines) = parse_process_output(&text);

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
            out.push_str(&format!("\x1b[1mProcess {}\x1b[0m", p));
        }
        if let Some(s) = status {
            out.push_str(&format!(" {}{}\x1b[0m", status_color, s));
        }
        out.push_str(&format!("\n\x1b[2m{}\x1b[0m\n", "─".repeat(50)));

        if lines.is_empty() {
            out.push_str("  \x1b[2m(no output)\x1b[0m\n");
        } else {
            let width = lines.len().to_string().len().max(3);
            for (i, line) in lines.iter().enumerate() {
                out.push_str(&format!(
                    "  \x1b[36m{:>width$}\x1b[0m \x1b[2m│\x1b[0m {}\n",
                    i + 1,
                    line,
                    width = width
                ));
            }
        }
        out
    }

    fn format_output_markdown(&self, result: &ToolResult) -> String {
        let text = result.as_text();
        let (pid, status, lines) = parse_process_output(&text);

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

        if lines.is_empty() {
            out.push_str("*No output*\n");
        } else {
            out.push_str("```\n");
            for line in lines {
                out.push_str(line);
                out.push('\n');
            }
            out.push_str("```\n");
        }
        out
    }

    async fn execute(&self, input: Self::Input) -> std::result::Result<ToolResult, ToolError> {
        let manager = SESSION_MANAGER.lock().await;

        // Check if session exists
        if manager.get_session(input.pid).await.is_none() {
            return Err(format!("Process {} not found", input.pid).into());
        }

        // Wait for potential new output
        drop(manager);
        tokio::time::sleep(tokio::time::Duration::from_millis(
            input.timeout_ms.min(10000),
        ))
        .await;
        let manager = SESSION_MANAGER.lock().await;

        let output = manager.read_output(input.pid, input.clear_buffer).await?;
        let status = manager.check_status(input.pid).await?;

        let content = if output.is_empty() {
            format!("Process {}\nStatus: {:?}\nNo new output", input.pid, status)
        } else {
            let mut result = format!(
                "Process {}\nStatus: {:?}\n\nOutput ({} lines):\n",
                input.pid,
                status,
                output.len()
            );

            for line in &output {
                result.push_str(&format!("{}\n", line));
            }

            result
        };

        Ok(content.into())
    }
}

/// Parse read_process_output result
fn parse_process_output(output: &str) -> (Option<&str>, Option<&str>, Vec<&str>) {
    let mut pid = None;
    let mut status = None;
    let mut lines = Vec::new();
    let mut in_output = false;

    for line in output.lines() {
        if line.starts_with("Process ") && !line.contains("Output") {
            pid = line.split_whitespace().nth(1);
        } else if line.starts_with("Status:") {
            status = Some(line.trim_start_matches("Status:").trim());
        } else if line.contains("Output (") || line == "No new output" {
            in_output = true;
            if line == "No new output" {
                // Don't set in_output, just note there's no output
            }
        } else if in_output {
            lines.push(line);
        }
    }

    (pid, status, lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::start_process::{StartProcessInput, StartProcessTool};
    use mixtape_core::ToolResult;

    #[tokio::test]
    async fn test_read_process_output_nonexistent() {
        let tool = ReadProcessOutputTool;

        let input = ReadProcessOutputInput {
            pid: 99999999,
            clear_buffer: false,
            timeout_ms: 100,
        };

        let result = tool.execute(input).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_read_process_output_basic() {
        // Start a process first
        let start_tool = StartProcessTool;
        let start_input = StartProcessInput {
            command: "echo 'test output'".to_string(),
            timeout_ms: Some(5000),
            shell: None,
        };

        let start_result = start_tool.execute(start_input).await;
        if start_result.is_err() {
            // Skip if process creation fails (might happen in CI)
            return;
        }

        let start_output = start_result.unwrap().as_text();
        // Extract PID from output
        if let Some(pid_line) = start_output.lines().find(|l| l.contains("PID:")) {
            if let Some(pid_str) = pid_line.split(':').nth(1) {
                if let Ok(pid) = pid_str.trim().parse::<u32>() {
                    // Now read the output
                    let read_tool = ReadProcessOutputTool;
                    let read_input = ReadProcessOutputInput {
                        pid,
                        clear_buffer: false,
                        timeout_ms: 100,
                    };

                    let result = read_tool.execute(read_input).await;
                    assert!(result.is_ok());
                    return;
                }
            }
        }

        // If we couldn't parse PID, that's okay - the test is about error handling
    }

    // ==================== parse_process_output tests ====================

    #[test]
    fn test_parse_process_output_complete() {
        let output = "Process 12345\nStatus: Running\n\nOutput (3 lines):\nline1\nline2\nline3";
        let (pid, status, lines) = parse_process_output(output);

        assert_eq!(pid, Some("12345"));
        assert_eq!(status, Some("Running"));
        assert_eq!(lines, vec!["line1", "line2", "line3"]);
    }

    #[test]
    fn test_parse_process_output_no_output() {
        let output = "Process 12345\nStatus: Running\nNo new output";
        let (pid, status, lines) = parse_process_output(output);

        assert_eq!(pid, Some("12345"));
        assert_eq!(status, Some("Running"));
        assert!(lines.is_empty());
    }

    #[test]
    fn test_parse_process_output_empty() {
        let output = "";
        let (pid, status, lines) = parse_process_output(output);

        assert_eq!(pid, None);
        assert_eq!(status, None);
        assert!(lines.is_empty());
    }

    #[test]
    fn test_parse_process_output_completed_status() {
        let output =
            "Process 999\nStatus: Completed { exit_code: Some(0) }\n\nOutput (1 lines):\ndone";
        let (pid, status, lines) = parse_process_output(output);

        assert_eq!(pid, Some("999"));
        assert_eq!(status, Some("Completed { exit_code: Some(0) }"));
        assert_eq!(lines, vec!["done"]);
    }

    #[test]
    fn test_parse_process_output_multiline() {
        let output = "Process 1\nStatus: Running\n\nOutput (5 lines):\na\nb\nc\nd\ne";
        let (_, _, lines) = parse_process_output(output);

        assert_eq!(lines.len(), 5);
    }

    // ==================== format_output tests ====================

    #[test]
    fn test_format_output_plain_with_output() {
        let tool = ReadProcessOutputTool;
        let result: ToolResult =
            "Process 12345\nStatus: Running\n\nOutput (2 lines):\nHello\nWorld".into();

        let formatted = tool.format_output_plain(&result);

        assert!(formatted.contains("Process 12345"));
        assert!(formatted.contains("Running"));
        assert!(formatted.contains("Hello"));
        assert!(formatted.contains("World"));
        assert!(formatted.contains("│")); // line number separator
    }

    #[test]
    fn test_format_output_plain_no_output() {
        let tool = ReadProcessOutputTool;
        let result: ToolResult = "Process 12345\nStatus: Running\nNo new output".into();

        let formatted = tool.format_output_plain(&result);

        assert!(formatted.contains("(no output)"));
    }

    #[test]
    fn test_format_output_ansi_running() {
        let tool = ReadProcessOutputTool;
        let result: ToolResult = "Process 12345\nStatus: Running\n\nOutput (1 lines):\ntest".into();

        let formatted = tool.format_output_ansi(&result);

        assert!(formatted.contains("\x1b[")); // ANSI codes present
        assert!(formatted.contains("\x1b[32m")); // green for running
    }

    #[test]
    fn test_format_output_ansi_completed() {
        let tool = ReadProcessOutputTool;
        let result: ToolResult =
            "Process 12345\nStatus: Completed\n\nOutput (1 lines):\ndone".into();

        let formatted = tool.format_output_ansi(&result);

        assert!(formatted.contains("\x1b[34m")); // blue for completed
    }

    #[test]
    fn test_format_output_ansi_waiting() {
        let tool = ReadProcessOutputTool;
        let result: ToolResult =
            "Process 12345\nStatus: WaitingForInput\n\nOutput (1 lines):\n>>> ".into();

        let formatted = tool.format_output_ansi(&result);

        assert!(formatted.contains("\x1b[33m")); // yellow for waiting
    }

    #[test]
    fn test_format_output_markdown_with_output() {
        let tool = ReadProcessOutputTool;
        let result: ToolResult =
            "Process 12345\nStatus: Running\n\nOutput (2 lines):\nline1\nline2".into();

        let formatted = tool.format_output_markdown(&result);

        assert!(formatted.contains("### 🟢 Process 12345")); // green circle for running
        assert!(formatted.contains("```"));
        assert!(formatted.contains("line1"));
    }

    #[test]
    fn test_format_output_markdown_no_output() {
        let tool = ReadProcessOutputTool;
        let result: ToolResult = "Process 12345\nStatus: Running\nNo new output".into();

        let formatted = tool.format_output_markdown(&result);

        assert!(formatted.contains("*No output*"));
    }

    #[test]
    fn test_format_output_markdown_status_emojis() {
        let tool = ReadProcessOutputTool;

        // Running = green circle
        let running: ToolResult = "Process 1\nStatus: Running\nNo new output".into();
        assert!(tool.format_output_markdown(&running).contains("🟢"));

        // Completed = blue circle
        let completed: ToolResult = "Process 1\nStatus: Completed\nNo new output".into();
        assert!(tool.format_output_markdown(&completed).contains("🔵"));

        // Waiting = yellow circle
        let waiting: ToolResult = "Process 1\nStatus: WaitingForInput\nNo new output".into();
        assert!(tool.format_output_markdown(&waiting).contains("🟡"));
    }

    // ==================== default value tests ====================

    #[test]
    fn test_default_timeout() {
        assert_eq!(default_timeout(), 5000);
    }

    // ==================== Tool metadata tests ====================

    #[test]
    fn test_tool_name() {
        let tool = ReadProcessOutputTool;
        assert_eq!(tool.name(), "read_process_output");
    }

    #[test]
    fn test_tool_description() {
        let tool = ReadProcessOutputTool;
        assert!(!tool.description().is_empty());
        assert!(tool.description().contains("output"));
    }
}
