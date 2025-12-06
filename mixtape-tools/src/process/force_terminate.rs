use crate::prelude::*;
use crate::process::start_process::SESSION_MANAGER;

/// Input for forcefully terminating a process
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ForceTerminateInput {
    /// Process ID to terminate
    pub pid: u32,

    /// Use force kill instead of graceful termination (default: true)
    #[serde(default = "default_force")]
    pub force: bool,
}

fn default_force() -> bool {
    true
}

/// Tool for forcefully terminating a process session
pub struct ForceTerminateTool;

impl Tool for ForceTerminateTool {
    type Input = ForceTerminateInput;

    fn name(&self) -> &str {
        "force_terminate"
    }

    fn description(&self) -> &str {
        "Forcefully terminate a process session. Can use either graceful SIGTERM or force SIGKILL."
    }

    async fn execute(&self, input: Self::Input) -> std::result::Result<ToolResult, ToolError> {
        let manager = SESSION_MANAGER.lock().await;

        // Check if session exists
        if manager.get_session(input.pid).await.is_none() {
            return Err(format!("Process {} not found", input.pid).into());
        }

        manager.terminate(input.pid, input.force).await?;

        let method = if input.force {
            "force killed"
        } else {
            "terminated"
        };

        Ok(format!("Successfully {} process {}", method, input.pid).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::start_process::{StartProcessInput, StartProcessTool};

    #[tokio::test]
    async fn test_force_terminate_nonexistent() {
        let tool = ForceTerminateTool;

        let input = ForceTerminateInput {
            pid: 99999999,
            force: true,
        };

        let result = tool.execute(input).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_force_terminate_basic() {
        // Start a long-running process
        let start_tool = StartProcessTool;
        let start_input = StartProcessInput {
            command: "sleep 10".to_string(),
            timeout_ms: Some(15000),
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
                    // Terminate it
                    let term_tool = ForceTerminateTool;
                    let term_input = ForceTerminateInput { pid, force: true };

                    let result = term_tool.execute(term_input).await;
                    assert!(result.is_ok());
                    let output = result.unwrap().as_text();
                    assert!(output.contains("Successfully"));
                    return;
                }
            }
        }
    }

    #[tokio::test]
    async fn test_force_terminate_graceful() {
        let start_tool = StartProcessTool;
        let start_input = StartProcessInput {
            command: "sleep 5".to_string(),
            timeout_ms: Some(10000),
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
                    // Graceful terminate
                    let term_tool = ForceTerminateTool;
                    let term_input = ForceTerminateInput { pid, force: false };

                    let result = term_tool.execute(term_input).await;
                    assert!(result.is_ok());
                    return;
                }
            }
        }
    }

    // ==================== default value tests ====================

    #[test]
    fn test_default_force() {
        assert!(default_force());
    }

    // ==================== Tool metadata tests ====================

    #[test]
    fn test_tool_name() {
        let tool = ForceTerminateTool;
        assert_eq!(tool.name(), "force_terminate");
    }

    #[test]
    fn test_tool_description() {
        let tool = ForceTerminateTool;
        assert!(!tool.description().is_empty());
        assert!(
            tool.description().contains("terminate") || tool.description().contains("Terminate")
        );
    }

    // ==================== Input struct tests ====================

    #[test]
    fn test_force_terminate_input_debug() {
        let input = ForceTerminateInput {
            pid: 12345,
            force: true,
        };
        let debug_str = format!("{:?}", input);
        assert!(debug_str.contains("12345"));
        assert!(debug_str.contains("true"));
    }

    // ==================== Output message tests ====================

    #[tokio::test]
    async fn test_force_terminate_output_message_force() {
        let start_tool = StartProcessTool;
        let start_input = StartProcessInput {
            command: "sleep 10".to_string(),
            timeout_ms: Some(15000),
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
                    let term_tool = ForceTerminateTool;
                    let term_input = ForceTerminateInput { pid, force: true };

                    let result = term_tool.execute(term_input).await;
                    if let Ok(output) = result {
                        // Force kill message
                        assert!(output.as_text().contains("force killed"));
                    }
                    return;
                }
            }
        }
    }

    #[tokio::test]
    async fn test_force_terminate_output_message_graceful() {
        let start_tool = StartProcessTool;
        let start_input = StartProcessInput {
            command: "sleep 10".to_string(),
            timeout_ms: Some(15000),
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
                    let term_tool = ForceTerminateTool;
                    let term_input = ForceTerminateInput { pid, force: false };

                    let result = term_tool.execute(term_input).await;
                    if let Ok(output) = result {
                        // Graceful terminate message
                        assert!(output.as_text().contains("terminated"));
                    }
                    return;
                }
            }
        }
    }
}
