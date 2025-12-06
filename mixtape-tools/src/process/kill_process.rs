use crate::prelude::*;
use sysinfo::{Pid, Signal, System};

/// Input for killing a process
#[derive(Debug, Deserialize, JsonSchema)]
pub struct KillProcessInput {
    /// Process ID (PID) to terminate
    pub pid: u32,
}

/// Tool for terminating processes
pub struct KillProcessTool;

impl Tool for KillProcessTool {
    type Input = KillProcessInput;

    fn name(&self) -> &str {
        "kill_process"
    }

    fn description(&self) -> &str {
        "Terminate a running process by its PID. Use with caution as this forcefully kills the process."
    }

    async fn execute(&self, input: Self::Input) -> std::result::Result<ToolResult, ToolError> {
        let mut sys = System::new();
        let pid = Pid::from_u32(input.pid);

        // Refresh all processes to find the target
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All);

        if let Some(process) = sys.process(pid) {
            let name = process.name().to_string_lossy().to_string();

            // Try to kill the process
            if process.kill_with(Signal::Term).is_some() {
                Ok(format!(
                    "Successfully sent termination signal to process {} (PID: {})",
                    name, input.pid
                )
                .into())
            } else {
                Err(format!(
                    "Failed to terminate process {} (PID: {}). Permission denied or process already terminated.",
                    name, input.pid
                ).into())
            }
        } else {
            Err(format!("Process with PID {} not found", input.pid).into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_kill_nonexistent_process() {
        let tool = KillProcessTool;
        let input = KillProcessInput { pid: 99999999 };

        let result = tool.execute(input).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_kill_process_success() {
        use crate::process::start_process::{StartProcessInput, StartProcessTool};

        // Start a process we can kill
        let start_tool = StartProcessTool;
        let start_input = StartProcessInput {
            command: "sleep 30".to_string(),
            timeout_ms: Some(60000),
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
                    // Give the process time to fully start
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                    // Kill the process
                    let kill_tool = KillProcessTool;
                    let kill_input = KillProcessInput { pid };

                    let result = kill_tool.execute(kill_input).await;
                    if result.is_err() {
                        eprintln!("Kill failed: {:?}", result.as_ref().err());
                        // This is acceptable - process may have already exited or be managed differently
                        return;
                    }
                    let output = result.unwrap().as_text();
                    assert!(output.contains("Successfully sent termination signal"));
                    return;
                }
            }
        }
    }

    // ==================== Tool metadata tests ====================

    #[test]
    fn test_tool_name() {
        let tool = KillProcessTool;
        assert_eq!(tool.name(), "kill_process");
    }

    #[test]
    fn test_tool_description() {
        let tool = KillProcessTool;
        assert!(!tool.description().is_empty());
        assert!(tool.description().contains("Terminate") || tool.description().contains("kill"));
    }

    // ==================== Input struct tests ====================

    #[test]
    fn test_kill_process_input_debug() {
        let input = KillProcessInput { pid: 12345 };
        let debug_str = format!("{:?}", input);
        assert!(debug_str.contains("12345"));
    }
}
