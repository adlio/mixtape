//! Mock MCP server helper for integration tests
//!
//! Provides utilities to spawn and interact with the mock MCP server binary.

/// Get the command to spawn the mock MCP server
///
/// Returns (command, args) suitable for use with McpTransport::stdio()
pub fn command() -> (String, Vec<String>) {
    let binary = env!("CARGO_BIN_EXE_mock_mcp_server");
    (binary.to_string(), vec![])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_returns_valid_path() {
        let (cmd, args) = command();
        assert!(!cmd.is_empty());
        assert!(args.is_empty());
        // The binary path should exist after cargo build
        assert!(cmd.contains("mock_mcp_server"));
    }
}
