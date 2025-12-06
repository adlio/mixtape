use std::collections::{HashMap, HashSet};

/// Configuration for an MCP server connection
#[derive(Debug, Clone)]
pub struct McpServerConfig {
    /// Server name for identification
    pub name: String,
    /// Transport configuration
    pub transport: McpTransport,
    /// Optional tool filter
    tool_filter: Option<ToolFilter>,
    /// Optional namespace prefix for tool names (e.g., "perplexity_")
    namespace: Option<String>,
}

/// Filter for selecting which tools to expose from an MCP server
#[derive(Debug, Clone)]
pub enum ToolFilter {
    /// Only include these tools (whitelist)
    Only(HashSet<String>),
    /// Include all tools except these (blacklist)
    Exclude(HashSet<String>),
}

impl McpServerConfig {
    /// Create a new MCP server configuration
    ///
    /// By default, tools are namespaced with the server name (e.g., "chrome_search").
    /// Use `.without_namespace()` to disable this behavior.
    ///
    /// # Example
    /// ```
    /// # use mixtape_core::mcp::{McpServerConfig, McpTransport};
    /// let config = McpServerConfig::new("chrome",
    ///     McpTransport::stdio("npx")
    ///         .args(["-y", "chrome-devtools-mcp@latest"])
    /// );
    /// // Tools will be named: chrome_search, chrome_navigate, etc.
    /// ```
    pub fn new(name: impl Into<String>, transport: impl Into<McpTransport>) -> Self {
        let name = name.into();
        let namespace = format!("{}_", name);
        Self {
            name,
            transport: transport.into(),
            tool_filter: None,
            namespace: Some(namespace),
        }
    }

    /// Override the namespace prefix for tool names
    ///
    /// # Example
    /// ```
    /// # use mixtape_core::mcp::{McpServerConfig, McpTransport};
    /// let config = McpServerConfig::new("chrome-devtools",
    ///     McpTransport::stdio("npx").args(["-y", "chrome-devtools-mcp@latest"])
    /// )
    /// .with_namespace("chrome");  // Use shorter prefix
    /// // Tools will be named: chrome_search, chrome_navigate, etc.
    /// ```
    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        let ns = namespace.into();
        // Ensure namespace ends with underscore for clean separation
        let ns = if ns.ends_with('_') {
            ns
        } else {
            format!("{}_", ns)
        };
        self.namespace = Some(ns);
        self
    }

    /// Disable namespacing - tool names will not be prefixed
    ///
    /// # Example
    /// ```
    /// # use mixtape_core::mcp::{McpServerConfig, McpTransport};
    /// let config = McpServerConfig::new("filesystem",
    ///     McpTransport::stdio("npx").args(["-y", "@modelcontextprotocol/server-filesystem"])
    /// )
    /// .without_namespace();
    /// // Tools keep original names: read_file, write_file, etc.
    /// ```
    pub fn without_namespace(mut self) -> Self {
        self.namespace = None;
        self
    }

    /// Get the namespace prefix if set
    pub(crate) fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }

    /// Only expose specific tools from this server (whitelist)
    ///
    /// # Example
    /// ```
    /// # use mixtape_core::mcp::{McpServerConfig, McpTransport};
    /// let config = McpServerConfig::new("filesystem",
    ///     McpTransport::stdio("npx")
    ///         .args(["-y", "@modelcontextprotocol/server-filesystem"])
    /// )
    /// .only_tools(["read_file", "write_file"]);
    /// ```
    pub fn only_tools<I, S>(mut self, tools: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.tool_filter = Some(ToolFilter::Only(
            tools.into_iter().map(|s| s.into()).collect(),
        ));
        self
    }

    /// Exclude specific tools from this server (blacklist)
    ///
    /// # Example
    /// ```
    /// # use mixtape_core::mcp::{McpServerConfig, McpTransport};
    /// let config = McpServerConfig::new("filesystem",
    ///     McpTransport::stdio("npx")
    ///         .args(["-y", "@modelcontextprotocol/server-filesystem"])
    /// )
    /// .exclude_tools(["delete_file", "execute_command"]);
    /// ```
    pub fn exclude_tools<I, S>(mut self, tools: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.tool_filter = Some(ToolFilter::Exclude(
            tools.into_iter().map(|s| s.into()).collect(),
        ));
        self
    }

    /// Check if a tool should be included based on the filter
    pub(crate) fn should_include_tool(&self, tool_name: &str) -> bool {
        match &self.tool_filter {
            None => true, // No filter, include everything
            Some(ToolFilter::Only(allowed)) => allowed.contains(tool_name),
            Some(ToolFilter::Exclude(excluded)) => !excluded.contains(tool_name),
        }
    }
}

/// MCP transport types
#[derive(Debug, Clone)]
pub enum McpTransport {
    /// Spawn a child process and communicate via stdio
    ///
    /// This is the most common transport for local MCP servers installed via npm.
    Stdio {
        /// Command to execute (e.g., "npx", "python", "node")
        command: String,
        /// Command-line arguments
        args: Vec<String>,
        /// Environment variables to pass to the process
        env: HashMap<String, String>,
    },
    /// Connect to an HTTP endpoint using Streamable HTTP transport
    ///
    /// Uses the MCP Streamable HTTP protocol (2025+) which combines HTTP POST
    /// for requests with SSE (Server-Sent Events) for streaming responses.
    /// Connect to the server's `/mcp` endpoint.
    ///
    /// All custom headers are passed to the server with each request.
    Http {
        /// Server URL (typically ending in `/mcp`)
        url: String,
        /// HTTP headers (for authentication, API keys, etc.)
        headers: HashMap<String, String>,
    },
}

impl McpTransport {
    /// Create a stdio transport builder with the given command
    ///
    /// # Example
    /// ```
    /// # use mixtape_core::mcp::McpTransport;
    /// let transport = McpTransport::stdio("npx")
    ///     .args(["-y", "some-mcp-server"])
    ///     .env("API_KEY", "secret");
    /// ```
    pub fn stdio(command: impl Into<String>) -> StdioBuilder {
        StdioBuilder::new(command)
    }

    /// Create an HTTP transport builder with the given URL
    ///
    /// Uses Streamable HTTP protocol with SSE streaming. All headers added via
    /// `.header()` or `.headers()` are sent with every request.
    ///
    /// # Example
    /// ```
    /// # use mixtape_core::mcp::McpTransport;
    /// let transport = McpTransport::http("https://gitmcp.io/owner/repo")
    ///     .header("Authorization", "Bearer token")
    ///     .header("X-Custom-Header", "value");
    /// ```
    pub fn http(url: impl Into<String>) -> HttpBuilder {
        HttpBuilder::new(url)
    }
}

/// Builder for stdio transport configuration
#[derive(Debug, Clone)]
pub struct StdioBuilder {
    command: String,
    args: Vec<String>,
    env: HashMap<String, String>,
}

impl StdioBuilder {
    /// Create a new stdio builder with the given command
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
            env: HashMap::new(),
        }
    }

    /// Add a single argument
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Add multiple arguments
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(|s| s.into()));
        self
    }

    /// Set a single environment variable
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Set multiple environment variables
    pub fn envs<I, K, V>(mut self, vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.env
            .extend(vars.into_iter().map(|(k, v)| (k.into(), v.into())));
        self
    }

    /// Build the transport (explicit conversion)
    pub fn build(self) -> McpTransport {
        self.into()
    }
}

impl From<StdioBuilder> for McpTransport {
    fn from(builder: StdioBuilder) -> Self {
        McpTransport::Stdio {
            command: builder.command,
            args: builder.args,
            env: builder.env,
        }
    }
}

/// Builder for HTTP transport configuration
#[derive(Debug, Clone)]
pub struct HttpBuilder {
    url: String,
    headers: HashMap<String, String>,
}

impl HttpBuilder {
    /// Create a new HTTP builder with the given URL
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            headers: HashMap::new(),
        }
    }

    /// Set a single header
    pub fn header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    /// Set multiple headers
    pub fn headers<I, K, V>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.headers
            .extend(headers.into_iter().map(|(k, v)| (k.into(), v.into())));
        self
    }

    /// Build the transport (explicit conversion)
    pub fn build(self) -> McpTransport {
        self.into()
    }
}

impl From<HttpBuilder> for McpTransport {
    fn from(builder: HttpBuilder) -> Self {
        McpTransport::Http {
            url: builder.url,
            headers: builder.headers,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== StdioBuilder Tests =====

    #[test]
    fn test_stdio_builder_basic() {
        let transport = McpTransport::stdio("python").build();

        if let McpTransport::Stdio { command, args, env } = transport {
            assert_eq!(command, "python");
            assert!(args.is_empty());
            assert!(env.is_empty());
        } else {
            panic!("Expected Stdio transport");
        }
    }

    #[test]
    fn test_stdio_builder_with_arg() {
        let transport = McpTransport::stdio("node").arg("server.js").build();

        if let McpTransport::Stdio { args, .. } = transport {
            assert_eq!(args, vec!["server.js"]);
        } else {
            panic!("Expected Stdio transport");
        }
    }

    #[test]
    fn test_stdio_builder_with_args() {
        let transport = McpTransport::stdio("npx")
            .args(["-y", "@modelcontextprotocol/server-filesystem"])
            .build();

        if let McpTransport::Stdio { args, .. } = transport {
            assert_eq!(args, vec!["-y", "@modelcontextprotocol/server-filesystem"]);
        } else {
            panic!("Expected Stdio transport");
        }
    }

    #[test]
    fn test_stdio_builder_with_env() {
        let transport = McpTransport::stdio("python")
            .env("API_KEY", "secret123")
            .build();

        if let McpTransport::Stdio { env, .. } = transport {
            assert_eq!(env.get("API_KEY"), Some(&"secret123".to_string()));
        } else {
            panic!("Expected Stdio transport");
        }
    }

    #[test]
    fn test_stdio_builder_with_envs() {
        let transport = McpTransport::stdio("node")
            .envs([("KEY1", "val1"), ("KEY2", "val2")])
            .build();

        if let McpTransport::Stdio { env, .. } = transport {
            assert_eq!(env.get("KEY1"), Some(&"val1".to_string()));
            assert_eq!(env.get("KEY2"), Some(&"val2".to_string()));
        } else {
            panic!("Expected Stdio transport");
        }
    }

    #[test]
    fn test_stdio_builder_chaining() {
        let transport = McpTransport::stdio("npx")
            .arg("-y")
            .args(["mcp-server", "--port", "3000"])
            .env("DEBUG", "true")
            .envs([("NODE_ENV", "production")])
            .build();

        if let McpTransport::Stdio { command, args, env } = transport {
            assert_eq!(command, "npx");
            assert_eq!(args, vec!["-y", "mcp-server", "--port", "3000"]);
            assert_eq!(env.get("DEBUG"), Some(&"true".to_string()));
            assert_eq!(env.get("NODE_ENV"), Some(&"production".to_string()));
        } else {
            panic!("Expected Stdio transport");
        }
    }

    #[test]
    fn test_stdio_builder_into_transport() {
        // Test implicit conversion via Into trait
        let builder = McpTransport::stdio("echo").arg("hello");
        let transport: McpTransport = builder.into();

        assert!(matches!(transport, McpTransport::Stdio { .. }));
    }

    // ===== HttpBuilder Tests =====

    #[test]
    fn test_http_builder_basic() {
        let transport = McpTransport::http("https://example.com/mcp").build();

        if let McpTransport::Http { url, headers } = transport {
            assert_eq!(url, "https://example.com/mcp");
            assert!(headers.is_empty());
        } else {
            panic!("Expected Http transport");
        }
    }

    #[test]
    fn test_http_builder_with_header() {
        let transport = McpTransport::http("https://api.example.com")
            .header("Authorization", "Bearer token123")
            .build();

        if let McpTransport::Http { headers, .. } = transport {
            assert_eq!(
                headers.get("Authorization"),
                Some(&"Bearer token123".to_string())
            );
        } else {
            panic!("Expected Http transport");
        }
    }

    #[test]
    fn test_http_builder_with_headers() {
        let transport = McpTransport::http("https://api.example.com")
            .headers([("X-API-Key", "key123"), ("X-Custom-Header", "value")])
            .build();

        if let McpTransport::Http { headers, .. } = transport {
            assert_eq!(headers.get("X-API-Key"), Some(&"key123".to_string()));
            assert_eq!(headers.get("X-Custom-Header"), Some(&"value".to_string()));
        } else {
            panic!("Expected Http transport");
        }
    }

    #[test]
    fn test_http_builder_chaining() {
        let transport = McpTransport::http("https://gitmcp.io/owner/repo")
            .header("Authorization", "Bearer abc")
            .headers([("Accept", "application/json")])
            .header("X-Request-Id", "123")
            .build();

        if let McpTransport::Http { url, headers } = transport {
            assert_eq!(url, "https://gitmcp.io/owner/repo");
            assert_eq!(headers.len(), 3);
        } else {
            panic!("Expected Http transport");
        }
    }

    #[test]
    fn test_http_builder_into_transport() {
        let builder = McpTransport::http("https://test.com");
        let transport: McpTransport = builder.into();

        assert!(matches!(transport, McpTransport::Http { .. }));
    }

    // ===== McpServerConfig Tests =====

    #[test]
    fn test_config_default_namespace() {
        let config = McpServerConfig::new(
            "filesystem",
            McpTransport::stdio("npx").args(["-y", "mcp-server"]),
        );

        assert_eq!(config.namespace(), Some("filesystem_"));
    }

    #[test]
    fn test_config_with_namespace() {
        let config = McpServerConfig::new("long-server-name", McpTransport::stdio("node"))
            .with_namespace("short");

        assert_eq!(config.namespace(), Some("short_"));
    }

    #[test]
    fn test_config_with_namespace_already_has_underscore() {
        let config =
            McpServerConfig::new("server", McpTransport::stdio("node")).with_namespace("prefix_");

        // Should not double the underscore
        assert_eq!(config.namespace(), Some("prefix_"));
    }

    #[test]
    fn test_config_without_namespace() {
        let config =
            McpServerConfig::new("filesystem", McpTransport::stdio("node")).without_namespace();

        assert_eq!(config.namespace(), None);
    }

    #[test]
    fn test_config_only_tools() {
        let config = McpServerConfig::new("server", McpTransport::stdio("node"))
            .only_tools(["read_file", "write_file"]);

        assert!(config.should_include_tool("read_file"));
        assert!(config.should_include_tool("write_file"));
        assert!(!config.should_include_tool("delete_file"));
        assert!(!config.should_include_tool("execute"));
    }

    #[test]
    fn test_config_exclude_tools() {
        let config = McpServerConfig::new("server", McpTransport::stdio("node"))
            .exclude_tools(["dangerous_tool", "another_bad"]);

        assert!(config.should_include_tool("read_file"));
        assert!(config.should_include_tool("write_file"));
        assert!(!config.should_include_tool("dangerous_tool"));
        assert!(!config.should_include_tool("another_bad"));
    }

    #[test]
    fn test_config_no_filter() {
        let config = McpServerConfig::new("server", McpTransport::stdio("node"));

        // Without filter, all tools should be included
        assert!(config.should_include_tool("any_tool"));
        assert!(config.should_include_tool("another_tool"));
    }

    #[test]
    fn test_config_chaining() {
        let config = McpServerConfig::new(
            "chrome",
            McpTransport::stdio("npx").args(["-y", "chrome-mcp"]),
        )
        .with_namespace("browser")
        .only_tools(["navigate", "screenshot"]);

        assert_eq!(config.name, "chrome");
        assert_eq!(config.namespace(), Some("browser_"));
        assert!(config.should_include_tool("navigate"));
        assert!(config.should_include_tool("screenshot"));
        assert!(!config.should_include_tool("execute_js"));
    }
}
