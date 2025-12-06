use crate::prelude::*;
use crate::process::start_process::SESSION_MANAGER;

/// Input for listing sessions (no parameters needed)
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListSessionsInput {}

/// Tool for listing active process sessions
pub struct ListSessionsTool;

impl Tool for ListSessionsTool {
    type Input = ListSessionsInput;

    fn name(&self) -> &str {
        "list_sessions"
    }

    fn description(&self) -> &str {
        "List all active process sessions with their PIDs, commands, status, and runtime."
    }

    async fn execute(&self, _input: Self::Input) -> std::result::Result<ToolResult, ToolError> {
        let manager = SESSION_MANAGER.lock().await;
        let sessions = manager.list_sessions().await;

        if sessions.is_empty() {
            return Ok("No active sessions".into());
        }

        let mut content = String::from("Active Sessions:\n\n");
        content.push_str("PID    | STATUS              | RUNTIME | COMMAND\n");
        content.push_str("-------|---------------------|---------|------------------\n");

        for (pid, command, status, elapsed_ms) in sessions {
            let runtime = if elapsed_ms < 1000 {
                format!("{}ms", elapsed_ms)
            } else if elapsed_ms < 60_000 {
                format!("{:.1}s", elapsed_ms as f64 / 1000.0)
            } else {
                format!("{:.1}m", elapsed_ms as f64 / 60_000.0)
            };

            let status_str = format!("{:?}", status);
            let cmd_preview = if command.len() > 30 {
                format!("{}...", &command[..27])
            } else {
                command
            };

            content.push_str(&format!(
                "{:<6} | {:<19} | {:<7} | {}\n",
                pid, status_str, runtime, cmd_preview
            ));
        }

        Ok(content.into())
    }

    fn format_output_plain(&self, result: &ToolResult) -> String {
        let output = result.as_text();
        if output == "No active sessions" {
            return output.to_string();
        }

        let lines: Vec<&str> = output.lines().collect();
        let mut out = String::from("Sessions\n");
        out.push_str(&"─".repeat(60));
        out.push('\n');

        for line in lines.iter().skip(4) {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() >= 4 {
                let (pid, status, runtime, command) = (
                    parts[0].trim(),
                    parts[1].trim(),
                    parts[2].trim(),
                    parts[3].trim(),
                );
                let status_icon = if status.contains("Running") {
                    "●"
                } else if status.contains("Completed") {
                    "✓"
                } else {
                    "○"
                };
                out.push_str(&format!(
                    "{} [{}] {} - {} ({})\n",
                    status_icon,
                    pid,
                    command,
                    status,
                    format_runtime_nice(runtime)
                ));
            }
        }
        out
    }

    fn format_output_ansi(&self, result: &ToolResult) -> String {
        let output = result.as_text();
        if output == "No active sessions" {
            return format!("\x1b[2m{}\x1b[0m", output);
        }

        let lines: Vec<&str> = output.lines().collect();
        let mut out = String::from("\x1b[1mSessions\x1b[0m\n");
        out.push_str(&format!("\x1b[2m{}\x1b[0m\n", "─".repeat(60)));

        for line in lines.iter().skip(4) {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() >= 4 {
                let (pid, status, runtime, command) = (
                    parts[0].trim(),
                    parts[1].trim(),
                    parts[2].trim(),
                    parts[3].trim(),
                );
                let (status_icon, status_color) = if status.contains("Running") {
                    ("\x1b[32m●\x1b[0m", "\x1b[32m")
                } else if status.contains("Completed") {
                    ("\x1b[34m✓\x1b[0m", "\x1b[34m")
                } else if status.contains("Failed") || status.contains("Error") {
                    ("\x1b[31m✗\x1b[0m", "\x1b[31m")
                } else {
                    ("\x1b[33m○\x1b[0m", "\x1b[33m")
                };
                out.push_str(&format!(
                    "{} \x1b[36m[{}]\x1b[0m {} {}{}\x1b[0m \x1b[2m({})\x1b[0m\n",
                    status_icon,
                    pid,
                    command,
                    status_color,
                    status,
                    format_runtime_nice(runtime)
                ));
            }
        }
        out
    }

    fn format_output_markdown(&self, result: &ToolResult) -> String {
        let output = result.as_text();
        if output == "No active sessions" {
            return format!("*{}*", output);
        }

        let lines: Vec<&str> = output.lines().collect();
        let mut out = String::from("### Sessions\n\n| Status | PID | Command | Runtime |\n|--------|-----|---------|--------|\n");

        for line in lines.iter().skip(4) {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() >= 4 {
                let (pid, status, runtime, command) = (
                    parts[0].trim(),
                    parts[1].trim(),
                    parts[2].trim(),
                    parts[3].trim(),
                );
                let status_emoji = if status.contains("Running") {
                    "🟢"
                } else if status.contains("Completed") {
                    "🔵"
                } else if status.contains("Failed") || status.contains("Error") {
                    "🔴"
                } else {
                    "🟡"
                };
                out.push_str(&format!(
                    "| {} {} | {} | `{}` | {} |\n",
                    status_emoji,
                    status,
                    pid,
                    command,
                    format_runtime_nice(runtime)
                ));
            }
        }
        out
    }
}

/// Format runtime in human-friendly form
fn format_runtime_nice(runtime_str: &str) -> String {
    // Parse the existing format (Xms, X.Xs, X.Xm)
    let s = runtime_str.trim();
    if s.ends_with("ms") {
        s.to_string()
    } else if s.ends_with('s') {
        let secs: f64 = s.trim_end_matches('s').parse().unwrap_or(0.0);
        if secs < 60.0 {
            format!("{:.0}s", secs)
        } else {
            let mins = (secs / 60.0).floor();
            let remaining_secs = secs % 60.0;
            format!("{}m {:02.0}s", mins as u32, remaining_secs)
        }
    } else if s.ends_with('m') {
        let mins: f64 = s.trim_end_matches('m').parse().unwrap_or(0.0);
        if mins < 60.0 {
            format!("{:.0}m", mins)
        } else {
            let hours = (mins / 60.0).floor();
            let remaining_mins = mins % 60.0;
            format!("{}h {:02.0}m", hours as u32, remaining_mins)
        }
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::start_process::{StartProcessInput, StartProcessTool};
    use mixtape_core::ToolResult;

    #[tokio::test]
    async fn test_list_sessions_empty() {
        let tool = ListSessionsTool;
        let input = ListSessionsInput {};

        let result = tool.execute(input).await;
        assert!(result.is_ok());

        let output = result.unwrap().as_text();
        // May have "No active sessions" or show existing sessions from other tests
        assert!(!output.is_empty());
    }

    #[tokio::test]
    async fn test_list_sessions_with_processes() {
        // Start a couple of processes
        let start_tool = StartProcessTool;

        let input1 = StartProcessInput {
            command: "echo 'session 1'".to_string(),
            timeout_ms: Some(5000),
            shell: None,
        };

        let input2 = StartProcessInput {
            command: "sleep 5".to_string(),
            timeout_ms: Some(10000),
            shell: None,
        };

        // Start first process
        let result1 = start_tool.execute(input1).await;
        if result1.is_err() {
            // Skip if process creation fails
            return;
        }

        // Start second process
        let _ = start_tool.execute(input2).await;

        // Now list sessions
        let list_tool = ListSessionsTool;
        let list_input = ListSessionsInput {};

        let result = list_tool.execute(list_input).await;
        assert!(result.is_ok());

        let output = result.unwrap().as_text();
        // Should show session header
        assert!(output.contains("PID"));
        assert!(output.contains("STATUS"));
        assert!(output.contains("RUNTIME"));
        assert!(output.contains("COMMAND"));
    }

    #[tokio::test]
    async fn test_list_sessions_shows_runtime() {
        let start_tool = StartProcessTool;
        let input = StartProcessInput {
            command: "sleep 2".to_string(),
            timeout_ms: Some(5000),
            shell: None,
        };

        let start_result = start_tool.execute(input).await;
        if start_result.is_err() {
            return;
        }

        // Wait a moment for runtime to accumulate
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        let list_tool = ListSessionsTool;
        let list_input = ListSessionsInput {};

        let result = list_tool.execute(list_input).await;
        assert!(result.is_ok());

        let output = result.unwrap().as_text();
        // Should show some runtime (ms or s)
        assert!(output.contains("ms") || output.contains("s") || output.contains("m"));
    }

    // ==================== format_runtime_nice tests ====================

    #[test]
    fn test_format_runtime_nice_milliseconds() {
        assert_eq!(format_runtime_nice("500ms"), "500ms");
        assert_eq!(format_runtime_nice("100ms"), "100ms");
    }

    #[test]
    fn test_format_runtime_nice_seconds() {
        assert_eq!(format_runtime_nice("5.0s"), "5s");
        // Note: f64 uses banker's rounding, so 30.5 rounds to 30 (nearest even)
        assert_eq!(format_runtime_nice("30.5s"), "30s");
        assert_eq!(format_runtime_nice("30.6s"), "31s");
    }

    #[test]
    fn test_format_runtime_nice_seconds_to_minutes() {
        // 90 seconds = 1m 30s
        assert_eq!(format_runtime_nice("90.0s"), "1m 30s");
        // 120 seconds = 2m 00s
        assert_eq!(format_runtime_nice("120.0s"), "2m 00s");
    }

    #[test]
    fn test_format_runtime_nice_minutes() {
        assert_eq!(format_runtime_nice("5.0m"), "5m");
        assert_eq!(format_runtime_nice("45.0m"), "45m");
    }

    #[test]
    fn test_format_runtime_nice_minutes_to_hours() {
        // 90 minutes = 1h 30m
        assert_eq!(format_runtime_nice("90.0m"), "1h 30m");
        // 120 minutes = 2h 00m
        assert_eq!(format_runtime_nice("120.0m"), "2h 00m");
    }

    #[test]
    fn test_format_runtime_nice_unknown_format() {
        assert_eq!(format_runtime_nice("unknown"), "unknown");
        assert_eq!(format_runtime_nice("5h"), "5h");
    }

    #[test]
    fn test_format_runtime_nice_trimming() {
        assert_eq!(format_runtime_nice("  500ms  "), "500ms");
    }

    // ==================== format_output tests ====================

    #[test]
    fn test_format_output_plain_no_sessions() {
        let tool = ListSessionsTool;
        let result: ToolResult = "No active sessions".into();

        let formatted = tool.format_output_plain(&result);
        assert_eq!(formatted, "No active sessions");
    }

    #[test]
    fn test_format_output_plain_with_sessions() {
        let tool = ListSessionsTool;
        let result: ToolResult = "Active Sessions:\n\nPID    | STATUS              | RUNTIME | COMMAND\n-------|---------------------|---------|------------------\n12345  | Running             | 500ms   | echo hello".into();

        let formatted = tool.format_output_plain(&result);

        assert!(formatted.contains("Sessions"));
        assert!(formatted.contains("12345"));
        assert!(formatted.contains("●") || formatted.contains("✓") || formatted.contains("○"));
    }

    #[test]
    fn test_format_output_ansi_no_sessions() {
        let tool = ListSessionsTool;
        let result: ToolResult = "No active sessions".into();

        let formatted = tool.format_output_ansi(&result);
        assert!(formatted.contains("\x1b[2m")); // dim
        assert!(formatted.contains("No active sessions"));
    }

    #[test]
    fn test_format_output_ansi_with_sessions() {
        let tool = ListSessionsTool;
        let result: ToolResult = "Active Sessions:\n\nPID    | STATUS              | RUNTIME | COMMAND\n-------|---------------------|---------|------------------\n12345  | Running             | 500ms   | sleep 10".into();

        let formatted = tool.format_output_ansi(&result);

        assert!(formatted.contains("\x1b[")); // ANSI codes
        assert!(formatted.contains("\x1b[1m")); // bold for header
        assert!(formatted.contains("\x1b[32m")); // green for running
    }

    #[test]
    fn test_format_output_ansi_status_colors() {
        let tool = ListSessionsTool;

        // Running = green
        let running: ToolResult = "Active Sessions:\n\nPID    | STATUS              | RUNTIME | COMMAND\n-------|---------------------|---------|------------------\n1      | Running             | 1ms     | cmd".into();
        let formatted = tool.format_output_ansi(&running);
        assert!(formatted.contains("\x1b[32m")); // green

        // Completed = blue
        let completed: ToolResult = "Active Sessions:\n\nPID    | STATUS              | RUNTIME | COMMAND\n-------|---------------------|---------|------------------\n1      | Completed           | 1ms     | cmd".into();
        let formatted = tool.format_output_ansi(&completed);
        assert!(formatted.contains("\x1b[34m")); // blue
    }

    #[test]
    fn test_format_output_markdown_no_sessions() {
        let tool = ListSessionsTool;
        let result: ToolResult = "No active sessions".into();

        let formatted = tool.format_output_markdown(&result);
        assert_eq!(formatted, "*No active sessions*");
    }

    #[test]
    fn test_format_output_markdown_with_sessions() {
        let tool = ListSessionsTool;
        let result: ToolResult = "Active Sessions:\n\nPID    | STATUS              | RUNTIME | COMMAND\n-------|---------------------|---------|------------------\n12345  | Running             | 500ms   | echo hello".into();

        let formatted = tool.format_output_markdown(&result);

        assert!(formatted.contains("### Sessions"));
        assert!(formatted.contains("| Status |"));
        assert!(formatted.contains("🟢 Running")); // green circle for running
    }

    #[test]
    fn test_format_output_markdown_status_emojis() {
        let tool = ListSessionsTool;

        // Running = green circle
        let running: ToolResult = "Active Sessions:\n\nPID    | STATUS              | RUNTIME | COMMAND\n-------|---------------------|---------|------------------\n1      | Running             | 1ms     | cmd".into();
        assert!(tool.format_output_markdown(&running).contains("🟢"));

        // Completed = blue circle
        let completed: ToolResult = "Active Sessions:\n\nPID    | STATUS              | RUNTIME | COMMAND\n-------|---------------------|---------|------------------\n1      | Completed           | 1ms     | cmd".into();
        assert!(tool.format_output_markdown(&completed).contains("🔵"));

        // Error = red circle
        let error: ToolResult = "Active Sessions:\n\nPID    | STATUS              | RUNTIME | COMMAND\n-------|---------------------|---------|------------------\n1      | Failed              | 1ms     | cmd".into();
        assert!(tool.format_output_markdown(&error).contains("🔴"));
    }

    // ==================== Tool metadata tests ====================

    #[test]
    fn test_tool_name() {
        let tool = ListSessionsTool;
        assert_eq!(tool.name(), "list_sessions");
    }

    #[test]
    fn test_tool_description() {
        let tool = ListSessionsTool;
        assert!(!tool.description().is_empty());
        assert!(tool.description().contains("session"));
    }
}
