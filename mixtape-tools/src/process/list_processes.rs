use crate::prelude::*;
use schemars::JsonSchema;
use serde::Deserialize;
use sysinfo::{ProcessesToUpdate, System};

/// Input for listing processes (no parameters needed)
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListProcessesInput {}

/// Tool for listing running processes
pub struct ListProcessesTool;

impl Tool for ListProcessesTool {
    type Input = ListProcessesInput;

    fn name(&self) -> &str {
        "list_processes"
    }

    fn description(&self) -> &str {
        "List all running processes on the system with their PID, name, CPU and memory usage."
    }

    async fn execute(&self, _input: Self::Input) -> std::result::Result<ToolResult, ToolError> {
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::All);

        let mut processes: Vec<_> = sys.processes().iter().collect();
        processes.sort_by_key(|(pid, _)| pid.as_u32());

        let mut output = String::from("PID     | NAME                          | CPU%  | MEMORY\n");
        output.push_str("--------|-------------------------------|-------|----------\n");

        for (pid, process) in processes.iter().take(50) {
            let name = process.name().to_string_lossy();
            let cpu = process.cpu_usage();
            let memory = process.memory();

            let memory_str = if memory < 1024 * 1024 {
                format!("{:.1} KB", memory as f64 / 1024.0)
            } else if memory < 1024 * 1024 * 1024 {
                format!("{:.1} MB", memory as f64 / (1024.0 * 1024.0))
            } else {
                format!("{:.1} GB", memory as f64 / (1024.0 * 1024.0 * 1024.0))
            };

            output.push_str(&format!(
                "{:<7} | {:<29} | {:>5.1} | {:>8}\n",
                pid.as_u32(),
                if name.len() > 29 {
                    format!("{}...", &name[..26])
                } else {
                    name.to_string()
                },
                cpu,
                memory_str
            ));
        }

        if processes.len() > 50 {
            output.push_str(&format!(
                "\n... and {} more processes\n",
                processes.len() - 50
            ));
        }

        Ok(output.into())
    }

    fn format_output_plain(&self, result: &ToolResult) -> String {
        result.as_text().to_string()
    }

    fn format_output_ansi(&self, result: &ToolResult) -> String {
        let output = result.as_text();
        let lines: Vec<&str> = output.lines().collect();
        if lines.len() < 3 {
            return output.to_string();
        }

        let mut out = String::new();
        out.push_str(&format!(
            "\x1b[1m{:>7}  {:<25}  {:>6}  {:>10}  {}\x1b[0m\n",
            "PID", "NAME", "CPU%", "MEMORY", "CPU"
        ));
        out.push_str(&format!("{}\n", "─".repeat(70)));

        for line in lines.iter().skip(2) {
            if line.starts_with("...") {
                out.push_str(&format!("\x1b[2m{}\x1b[0m\n", line));
                continue;
            }

            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() >= 4 {
                let pid = parts[0].trim();
                let name = parts[1].trim();
                let cpu_str = parts[2].trim();
                let memory = parts[3].trim();
                let cpu: f32 = cpu_str.parse().unwrap_or(0.0);
                let color = resource_color(cpu);
                let bar = resource_bar(cpu.min(100.0), 10);

                out.push_str(&format!(
                    "\x1b[36m{:>7}\x1b[0m  {:<25}  {}{:>5.1}%\x1b[0m  {:>10}  {}{}\x1b[0m\n",
                    pid,
                    if name.len() > 25 { &name[..22] } else { name },
                    color,
                    cpu,
                    memory,
                    color,
                    bar
                ));
            }
        }
        out
    }

    fn format_output_markdown(&self, result: &ToolResult) -> String {
        let output = result.as_text();
        let lines: Vec<&str> = output.lines().collect();
        if lines.len() < 3 {
            return format!("```\n{}\n```", output);
        }

        let mut out =
            String::from("| PID | Name | CPU% | Memory |\n|-----|------|------|--------|\n");
        for line in lines.iter().skip(2) {
            if line.starts_with("...") {
                out.push_str(&format!("\n*{}*\n", line));
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() >= 4 {
                out.push_str(&format!(
                    "| {} | `{}` | {} | {} |\n",
                    parts[0].trim(),
                    parts[1].trim(),
                    parts[2].trim(),
                    parts[3].trim()
                ));
            }
        }
        out
    }
}

/// Create a visual bar for resource usage
fn resource_bar(percent: f32, width: usize) -> String {
    let filled = ((percent / 100.0) * width as f32).round() as usize;
    let empty = width.saturating_sub(filled);
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

/// Color code for resource usage (green → yellow → red)
fn resource_color(percent: f32) -> &'static str {
    if percent < 25.0 {
        "\x1b[32m"
    } else if percent < 50.0 {
        "\x1b[33m"
    } else if percent < 75.0 {
        "\x1b[38;5;208m"
    } else {
        "\x1b[31m"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixtape_core::ToolResult;

    #[tokio::test]
    async fn test_list_processes_basic() {
        let tool = ListProcessesTool;
        let input = ListProcessesInput {};

        let result = tool.execute(input).await;
        assert!(result.is_ok());

        let output = result.unwrap().as_text();
        // Should have headers
        assert!(output.contains("PID"));
        assert!(output.contains("NAME"));
        assert!(output.contains("CPU"));
        assert!(output.contains("MEMORY"));
    }

    #[tokio::test]
    async fn test_list_processes_contains_processes() {
        let tool = ListProcessesTool;
        let input = ListProcessesInput {};

        let result = tool.execute(input).await;
        assert!(result.is_ok());

        let output = result.unwrap().as_text();
        // Should have at least one process (this test process)
        let line_count = output.lines().count();
        assert!(line_count > 2); // More than just headers
    }

    #[tokio::test]
    async fn test_list_processes_memory_formatting() {
        let tool = ListProcessesTool;
        let input = ListProcessesInput {};

        let result = tool.execute(input).await;
        assert!(result.is_ok());

        let output = result.unwrap().as_text();
        // Should show memory units (KB, MB, or GB)
        assert!(
            output.contains("KB") || output.contains("MB") || output.contains("GB"),
            "Expected memory units in output"
        );
    }

    // ==================== resource_bar tests ====================

    #[test]
    fn test_resource_bar_zero() {
        let bar = resource_bar(0.0, 10);
        assert_eq!(bar, "[░░░░░░░░░░]");
    }

    #[test]
    fn test_resource_bar_half() {
        let bar = resource_bar(50.0, 10);
        assert_eq!(bar, "[█████░░░░░]");
    }

    #[test]
    fn test_resource_bar_full() {
        let bar = resource_bar(100.0, 10);
        assert_eq!(bar, "[██████████]");
    }

    #[test]
    fn test_resource_bar_quarter() {
        let bar = resource_bar(25.0, 10);
        // 25% of 10 = 2.5, rounded = 3 (or 2 depending on rounding)
        let filled_count = bar.chars().filter(|c| *c == '█').count();
        assert!((2..=3).contains(&filled_count));
    }

    #[test]
    fn test_resource_bar_different_width() {
        let bar = resource_bar(50.0, 20);
        let filled_count = bar.chars().filter(|c| *c == '█').count();
        assert_eq!(filled_count, 10); // 50% of 20
    }

    #[test]
    fn test_resource_bar_overflow() {
        // Values over 100 are NOT capped internally - caller is responsible for capping
        // (format_output_ansi calls resource_bar(cpu.min(100.0), 10))
        let bar = resource_bar(150.0, 10);
        let filled_count = bar.chars().filter(|c| *c == '█').count();
        assert_eq!(filled_count, 15); // 150% of 10 = 15
    }

    // ==================== resource_color tests ====================

    #[test]
    fn test_resource_color_low() {
        let color = resource_color(10.0);
        assert_eq!(color, "\x1b[32m"); // green
    }

    #[test]
    fn test_resource_color_medium_low() {
        let color = resource_color(30.0);
        assert_eq!(color, "\x1b[33m"); // yellow
    }

    #[test]
    fn test_resource_color_medium_high() {
        let color = resource_color(60.0);
        assert_eq!(color, "\x1b[38;5;208m"); // orange
    }

    #[test]
    fn test_resource_color_high() {
        let color = resource_color(80.0);
        assert_eq!(color, "\x1b[31m"); // red
    }

    #[test]
    fn test_resource_color_boundaries() {
        // Test boundary values
        assert_eq!(resource_color(0.0), "\x1b[32m"); // green at 0
        assert_eq!(resource_color(24.9), "\x1b[32m"); // green just under 25
        assert_eq!(resource_color(25.0), "\x1b[33m"); // yellow at 25
        assert_eq!(resource_color(49.9), "\x1b[33m"); // yellow just under 50
        assert_eq!(resource_color(50.0), "\x1b[38;5;208m"); // orange at 50
        assert_eq!(resource_color(74.9), "\x1b[38;5;208m"); // orange just under 75
        assert_eq!(resource_color(75.0), "\x1b[31m"); // red at 75
        assert_eq!(resource_color(100.0), "\x1b[31m"); // red at 100
    }

    // ==================== format_output tests ====================

    #[test]
    fn test_format_output_plain() {
        let tool = ListProcessesTool;
        let result: ToolResult = "PID     | NAME                          | CPU%  | MEMORY\n--------|-------------------------------|-------|----------\n1       | init                          |   0.0 |   10.0 MB".into();

        let formatted = tool.format_output_plain(&result);

        // Plain format should return the raw text
        assert!(formatted.contains("PID"));
        assert!(formatted.contains("init"));
    }

    #[test]
    fn test_format_output_ansi_basic() {
        let tool = ListProcessesTool;
        let result: ToolResult = "PID     | NAME                          | CPU%  | MEMORY\n--------|-------------------------------|-------|----------\n1       | init                          |   0.0 |   10.0 MB".into();

        let formatted = tool.format_output_ansi(&result);

        // Should have ANSI codes
        assert!(formatted.contains("\x1b["));
        assert!(formatted.contains("\x1b[1m")); // bold header
        assert!(formatted.contains("\x1b[36m")); // cyan for PID
    }

    #[test]
    fn test_format_output_ansi_cpu_colors() {
        let tool = ListProcessesTool;

        // Low CPU (green)
        let low_cpu: ToolResult = "PID     | NAME                          | CPU%  | MEMORY\n--------|-------------------------------|-------|----------\n1       | proc                          |   5.0 |   10.0 MB".into();
        let formatted = tool.format_output_ansi(&low_cpu);
        assert!(formatted.contains("\x1b[32m")); // green

        // High CPU (red)
        let high_cpu: ToolResult = "PID     | NAME                          | CPU%  | MEMORY\n--------|-------------------------------|-------|----------\n1       | proc                          |  80.0 |   10.0 MB".into();
        let formatted = tool.format_output_ansi(&high_cpu);
        assert!(formatted.contains("\x1b[31m")); // red
    }

    #[test]
    fn test_format_output_ansi_with_overflow_indicator() {
        let tool = ListProcessesTool;
        let result: ToolResult = "PID     | NAME                          | CPU%  | MEMORY\n--------|-------------------------------|-------|----------\n1       | proc                          |   5.0 |   10.0 MB\n... and 100 more processes".into();

        let formatted = tool.format_output_ansi(&result);

        // Overflow indicator should be dimmed
        assert!(formatted.contains("\x1b[2m")); // dim
        assert!(formatted.contains("more processes"));
    }

    #[test]
    fn test_format_output_markdown_basic() {
        let tool = ListProcessesTool;
        let result: ToolResult = "PID     | NAME                          | CPU%  | MEMORY\n--------|-------------------------------|-------|----------\n1       | init                          |   0.0 |   10.0 MB".into();

        let formatted = tool.format_output_markdown(&result);

        // Should have markdown table
        assert!(formatted.contains("| PID |"));
        assert!(formatted.contains("|-----|"));
        assert!(formatted.contains("`init`")); // name in code style
    }

    #[test]
    fn test_format_output_markdown_with_overflow() {
        let tool = ListProcessesTool;
        let result: ToolResult = "PID     | NAME                          | CPU%  | MEMORY\n--------|-------------------------------|-------|----------\n1       | proc                          |   5.0 |   10.0 MB\n... and 50 more processes".into();

        let formatted = tool.format_output_markdown(&result);

        // Overflow should be italicized
        assert!(formatted.contains("*... and 50 more processes*"));
    }

    #[test]
    fn test_format_output_markdown_short_input() {
        let tool = ListProcessesTool;
        let result: ToolResult = "short".into();

        let formatted = tool.format_output_markdown(&result);

        // Short input should be wrapped in code block
        assert!(formatted.contains("```"));
    }

    // ==================== Tool metadata tests ====================

    #[test]
    fn test_tool_name() {
        let tool = ListProcessesTool;
        assert_eq!(tool.name(), "list_processes");
    }

    #[test]
    fn test_tool_description() {
        let tool = ListProcessesTool;
        assert!(!tool.description().is_empty());
        assert!(tool.description().contains("process"));
    }
}
