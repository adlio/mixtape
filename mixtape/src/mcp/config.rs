use super::{McpError, McpServerConfig, McpTransport};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// MCP configuration file format (compatible with Claude Desktop/Code)
#[derive(Debug, Deserialize)]
pub struct McpConfigFile {
    /// Map of server name to server configuration
    #[serde(rename = "mcpServers")]
    pub mcp_servers: HashMap<String, McpServerEntry>,
}

/// Individual server configuration entry
#[derive(Debug, Deserialize)]
pub struct McpServerEntry {
    /// Transport type: "stdio", "http", or "sse" (deprecated)
    #[serde(rename = "type", default)]
    pub server_type: Option<String>,
    /// Command to execute (for stdio)
    pub command: Option<String>,
    /// Command-line arguments (for stdio)
    pub args: Option<Vec<String>>,
    /// Environment variables
    pub env: Option<HashMap<String, String>>,
    /// Server URL (for http/sse)
    pub url: Option<String>,
    /// HTTP headers (for http/sse)
    pub headers: Option<HashMap<String, String>>,
}

/// Load MCP configuration from a JSON file
///
/// Supports environment variable expansion using `${VAR}` or `${VAR:-default}` syntax.
/// The path itself is also expanded using shell expansion (e.g., `~/.claude.json`).
pub async fn load_config_file(path: impl AsRef<Path>) -> Result<Vec<McpServerConfig>, McpError> {
    // Expand ~ and other shell variables in the path
    let path_str = path.as_ref().to_string_lossy().to_string();
    let expanded_path = shellexpand::tilde(&path_str);
    let path = Path::new(expanded_path.as_ref());

    // Read the file
    let content = tokio::fs::read_to_string(path).await?;

    // Expand environment variables in the JSON content
    let expanded_content = expand_env_vars(&content)?;

    // Parse the JSON
    let config: McpConfigFile = serde_json::from_str(&expanded_content)?;

    // Convert to Vec<McpServerConfig>
    let mut servers = Vec::new();
    for (name, entry) in config.mcp_servers {
        let server = entry_to_config(name, entry)?;
        servers.push(server);
    }

    Ok(servers)
}

/// Convert an McpServerEntry to McpServerConfig
fn entry_to_config(name: String, entry: McpServerEntry) -> Result<McpServerConfig, McpError> {
    let server_type = entry.server_type.as_deref().unwrap_or("stdio");

    let transport = match server_type {
        "stdio" => {
            let command = entry
                .command
                .ok_or_else(|| McpError::Config(format!("Server '{}': missing 'command'", name)))?;
            McpTransport::Stdio {
                command,
                args: entry.args.unwrap_or_default(),
                env: entry.env.unwrap_or_default(),
            }
        }
        "http" => {
            let url = entry
                .url
                .ok_or_else(|| McpError::Config(format!("Server '{}': missing 'url'", name)))?;
            McpTransport::Http {
                url,
                headers: entry.headers.unwrap_or_default(),
            }
        }
        "sse" => {
            // SSE is deprecated but treat it like HTTP for now
            let url = entry
                .url
                .ok_or_else(|| McpError::Config(format!("Server '{}': missing 'url'", name)))?;
            McpTransport::Http {
                url,
                headers: entry.headers.unwrap_or_default(),
            }
        }
        other => {
            return Err(McpError::Config(format!(
                "Server '{}': unknown transport type '{}'",
                name, other
            )))
        }
    };

    // McpServerConfig::new() auto-namespaces with the server name
    Ok(McpServerConfig::new(&name, transport))
}

/// Expand environment variables in a string
///
/// Supports:
/// - `${VAR}` - expands to the value of VAR, or empty string if not set
/// - `${VAR:-default}` - expands to the value of VAR, or "default" if not set
fn expand_env_vars(input: &str) -> Result<String, McpError> {
    let mut result = String::new();
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' && chars.peek() == Some(&'{') {
            chars.next(); // consume '{'

            // Collect variable name until we hit '}' or ':'
            let mut var_name = String::new();
            let mut has_default = false;
            let mut default_value = String::new();

            while let Some(&next_ch) = chars.peek() {
                if next_ch == '}' {
                    chars.next(); // consume '}'
                    break;
                } else if next_ch == ':' {
                    chars.next(); // consume ':'
                    if chars.peek() == Some(&'-') {
                        chars.next(); // consume '-'
                        has_default = true;
                        // Collect default value until '}'
                        while let Some(&default_ch) = chars.peek() {
                            if default_ch == '}' {
                                chars.next(); // consume '}'
                                break;
                            }
                            default_value.push(default_ch);
                            chars.next();
                        }
                        break;
                    }
                } else {
                    var_name.push(next_ch);
                    chars.next();
                }
            }

            // Look up the variable
            let value = std::env::var(&var_name).ok();
            result.push_str(&value.unwrap_or_else(|| {
                if has_default {
                    default_value.clone()
                } else {
                    String::new()
                }
            }));
        } else {
            result.push(ch);
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_env_vars_simple() {
        std::env::set_var("TEST_VAR", "hello");
        let result = expand_env_vars("${TEST_VAR} world").unwrap();
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_expand_env_vars_with_default() {
        std::env::remove_var("NONEXISTENT_VAR");
        let result = expand_env_vars("${NONEXISTENT_VAR:-default}").unwrap();
        assert_eq!(result, "default");
    }

    #[test]
    fn test_expand_env_vars_empty_if_not_set() {
        std::env::remove_var("NONEXISTENT_VAR");
        let result = expand_env_vars("${NONEXISTENT_VAR}").unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_expand_env_vars_multiple() {
        std::env::set_var("VAR1", "foo");
        std::env::set_var("VAR2", "bar");
        let result = expand_env_vars("${VAR1}/${VAR2}").unwrap();
        assert_eq!(result, "foo/bar");
    }

    #[test]
    fn test_expand_env_vars_no_expansion() {
        let result = expand_env_vars("no variables here").unwrap();
        assert_eq!(result, "no variables here");
    }

    #[test]
    fn test_entry_to_config_stdio() {
        let entry = McpServerEntry {
            server_type: Some("stdio".to_string()),
            command: Some("npx".to_string()),
            args: Some(vec!["-y".to_string()]),
            env: None,
            url: None,
            headers: None,
        };

        let config = entry_to_config("test".to_string(), entry).unwrap();
        assert_eq!(config.name, "test");
        match &config.transport {
            McpTransport::Stdio { command, args, .. } => {
                assert_eq!(command, "npx");
                assert_eq!(args, &vec!["-y"]);
            }
            _ => panic!("Expected Stdio transport"),
        }
    }

    #[test]
    fn test_entry_to_config_http() {
        let entry = McpServerEntry {
            server_type: Some("http".to_string()),
            command: None,
            args: None,
            env: None,
            url: Some("https://api.example.com".to_string()),
            headers: Some(
                [("Authorization".to_string(), "Bearer token".to_string())]
                    .iter()
                    .cloned()
                    .collect(),
            ),
        };

        let config = entry_to_config("test".to_string(), entry).unwrap();
        match &config.transport {
            McpTransport::Http { url, headers } => {
                assert_eq!(url, "https://api.example.com");
                assert_eq!(headers.get("Authorization").unwrap(), "Bearer token");
            }
            _ => panic!("Expected Http transport"),
        }
    }

    #[test]
    fn test_tool_filtering_only() {
        use super::McpServerConfig;

        let config = McpServerConfig::new("test", McpTransport::stdio("npx"))
            .only_tools(["read_file", "write_file"]);

        assert!(config.should_include_tool("read_file"));
        assert!(config.should_include_tool("write_file"));
        assert!(!config.should_include_tool("delete_file"));
    }

    #[test]
    fn test_tool_filtering_exclude() {
        use super::McpServerConfig;

        let config = McpServerConfig::new("test", McpTransport::stdio("npx"))
            .exclude_tools(["delete_file", "execute_command"]);

        assert!(config.should_include_tool("read_file"));
        assert!(config.should_include_tool("write_file"));
        assert!(!config.should_include_tool("delete_file"));
        assert!(!config.should_include_tool("execute_command"));
    }

    #[test]
    fn test_tool_filtering_none() {
        use super::McpServerConfig;

        let config = McpServerConfig::new("test", McpTransport::stdio("npx"));

        // No filter, should include everything
        assert!(config.should_include_tool("any_tool"));
        assert!(config.should_include_tool("another_tool"));
    }
}
