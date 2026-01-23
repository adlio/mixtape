//! AgentBuilder for fluent agent construction
//!
//! The builder pattern allows configuring all agent options before
//! creating the provider, moving the async work to `.build().await`.
//!
//! Also contains post-construction mutation methods for Agent (`set_*`, `add_*`)
//! for runtime configuration changes.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use crate::conversation::{BoxedConversationManager, SlidingWindowConversationManager};
use crate::permission::{GrantStore, ToolAuthorizationPolicy, ToolCallAuthorizer};
use crate::provider::ModelProvider;
use crate::tool::{box_tool, DynTool, Tool};

use super::context::{ContextConfig, ContextSource};
use super::types::{DEFAULT_MAX_CONCURRENT_TOOLS, DEFAULT_PERMISSION_TIMEOUT};
use super::Agent;

#[cfg(feature = "session")]
use crate::session::SessionStore;

#[cfg(feature = "bedrock")]
use crate::model::BedrockModel;
#[cfg(feature = "bedrock")]
use crate::provider::BedrockProvider;

#[cfg(feature = "anthropic")]
use crate::model::AnthropicModel;
#[cfg(feature = "anthropic")]
use crate::provider::AnthropicProvider;

/// Factory function that creates a provider asynchronously
type ProviderFactory = Box<
    dyn FnOnce()
            -> Pin<Box<dyn Future<Output = crate::error::Result<Arc<dyn ModelProvider>>> + Send>>
        + Send,
>;

/// Builder for creating an Agent with fluent configuration
///
/// Use `Agent::builder()` to create a new builder, configure it with
/// the various `with_*` methods, and call `.build().await` to create
/// the agent.
///
/// # Example
///
/// ```ignore
/// use mixtape_core::{Agent, ClaudeHaiku4_5, Result};
///
/// #[tokio::main]
/// async fn main() -> Result<()> {
///     let agent = Agent::builder()
///         .bedrock(ClaudeHaiku4_5)
///         .with_system_prompt("You are a helpful assistant")
///         .add_tool(Calculator)
///         .build()
///         .await?;
///
///     let response = agent.run("What's 2 + 2?").await?;
///     println!("{}", response);
///     Ok(())
/// }
/// ```
pub struct AgentBuilder {
    provider_factory: Option<ProviderFactory>,
    tools: Vec<Box<dyn DynTool>>,
    system_prompt: Option<String>,
    max_concurrent_tools: usize,
    /// Custom grant store (if None, uses MemoryGrantStore)
    pub(super) grant_store: Option<Box<dyn GrantStore>>,
    /// Policy for tools without grants (default: AutoDeny)
    pub(super) authorization_policy: ToolAuthorizationPolicy,
    /// Timeout for authorization requests
    pub(super) authorization_timeout: Duration,
    /// Tools to automatically grant permissions for
    trusted_tools: Vec<String>,
    conversation_manager: Option<BoxedConversationManager>,
    #[cfg(feature = "session")]
    session_store: Option<Arc<dyn SessionStore>>,
    // MCP fields - configured via mcp.rs
    #[cfg(feature = "mcp")]
    pub(super) mcp_servers: Vec<crate::mcp::McpServerConfig>,
    #[cfg(feature = "mcp")]
    pub(super) mcp_config_files: Vec<std::path::PathBuf>,
    // Context file fields
    /// Context file sources (resolved at runtime)
    context_sources: Vec<ContextSource>,
    /// Context configuration (size limits)
    context_config: ContextConfig,
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentBuilder {
    /// Create a new AgentBuilder with default settings
    pub fn new() -> Self {
        Self {
            provider_factory: None,
            tools: Vec::new(),
            system_prompt: None,
            max_concurrent_tools: DEFAULT_MAX_CONCURRENT_TOOLS,
            grant_store: None,
            authorization_policy: ToolAuthorizationPolicy::default(), // AutoDeny by default
            authorization_timeout: DEFAULT_PERMISSION_TIMEOUT,
            trusted_tools: Vec::new(),
            conversation_manager: None,
            #[cfg(feature = "session")]
            session_store: None,
            #[cfg(feature = "mcp")]
            mcp_servers: Vec::new(),
            #[cfg(feature = "mcp")]
            mcp_config_files: Vec::new(),
            context_sources: Vec::new(),
            context_config: ContextConfig::default(),
        }
    }

    /// Configure the agent to use AWS Bedrock with the specified model
    ///
    /// The AWS credentials will be loaded from the environment when
    /// `.build().await` is called.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let agent = Agent::builder()
    ///     .bedrock(ClaudeSonnet4_5)
    ///     .build()
    ///     .await?;
    /// ```
    #[cfg(feature = "bedrock")]
    pub fn bedrock(mut self, model: impl BedrockModel + 'static) -> Self {
        self.provider_factory = Some(Box::new(move || {
            Box::pin(async move {
                let provider = BedrockProvider::new(model).await?;
                Ok(Arc::new(provider) as Arc<dyn ModelProvider>)
            })
        }));
        self
    }

    /// Configure the agent to use the Anthropic API directly
    ///
    /// # Example
    ///
    /// ```ignore
    /// let agent = Agent::builder()
    ///     .anthropic(ClaudeSonnet4_5, "sk-ant-...")
    ///     .build()
    ///     .await?;
    /// ```
    #[cfg(feature = "anthropic")]
    pub fn anthropic(
        mut self,
        model: impl AnthropicModel + 'static,
        api_key: impl Into<String>,
    ) -> Self {
        let api_key = api_key.into();
        self.provider_factory = Some(Box::new(move || {
            Box::pin(async move {
                let provider = AnthropicProvider::new(api_key, model)?;
                Ok(Arc::new(provider) as Arc<dyn ModelProvider>)
            })
        }));
        self
    }

    /// Configure the agent to use the Anthropic API with key from environment
    ///
    /// Reads `ANTHROPIC_API_KEY` from the environment.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let agent = Agent::builder()
    ///     .anthropic_from_env(ClaudeSonnet4_5)
    ///     .build()
    ///     .await?;
    /// ```
    #[cfg(feature = "anthropic")]
    pub fn anthropic_from_env(mut self, model: impl AnthropicModel + 'static) -> Self {
        self.provider_factory = Some(Box::new(move || {
            Box::pin(async move {
                let provider = AnthropicProvider::from_env(model)?;
                Ok(Arc::new(provider) as Arc<dyn ModelProvider>)
            })
        }));
        self
    }

    /// Use a pre-configured provider
    ///
    /// Use this when you need custom provider configuration (e.g., custom
    /// retry settings, inference profiles) or a custom provider implementation.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let provider = BedrockProvider::new(ClaudeSonnet4_5).await
    ///     .with_max_retries(5)
    ///     .with_inference_profile(InferenceProfile::US);
    ///
    /// let agent = Agent::builder()
    ///     .provider(provider)
    ///     .build()
    ///     .await?;
    /// ```
    pub fn provider(mut self, provider: impl ModelProvider + 'static) -> Self {
        let provider = Arc::new(provider) as Arc<dyn ModelProvider>;
        self.provider_factory = Some(Box::new(move || Box::pin(async move { Ok(provider) })));
        self
    }

    /// Add a tool to the agent
    ///
    /// # Example
    ///
    /// ```ignore
    /// let agent = Agent::builder()
    ///     .bedrock(ClaudeHaiku4_5)
    ///     .add_tool(Calculator)
    ///     .add_tool(WeatherLookup)
    ///     .build()
    ///     .await?;
    /// ```
    pub fn add_tool(mut self, tool: impl Tool + 'static) -> Self {
        self.tools.push(box_tool(tool));
        self
    }

    /// Add a trusted tool to the agent with automatic permission grant
    ///
    /// This is a convenience method that adds the tool and automatically grants
    /// permission for it to execute. Use this for tools you trust completely.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let agent = Agent::builder()
    ///     .bedrock(ClaudeHaiku4_5)
    ///     .add_trusted_tool(Calculator)
    ///     .add_trusted_tool(WeatherLookup)
    ///     .build()
    ///     .await?;
    /// ```
    pub fn add_trusted_tool(mut self, tool: impl Tool + 'static) -> Self {
        let tool_name = tool.name().to_string();
        self.tools.push(box_tool(tool));
        self.trusted_tools.push(tool_name);
        self
    }

    /// Add multiple tools to the agent
    ///
    /// Accepts pre-boxed dynamic tools, typically from tool group helper functions.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use mixtape_tools::sqlite;
    ///
    /// // Add all read-only SQLite tools
    /// let agent = Agent::builder()
    ///     .bedrock(ClaudeHaiku4_5)
    ///     .add_tools(sqlite::read_only_tools())
    ///     .build()
    ///     .await?;
    ///
    /// // Or add all SQLite tools
    /// let agent = Agent::builder()
    ///     .bedrock(ClaudeHaiku4_5)
    ///     .add_tools(sqlite::all_tools())
    ///     .build()
    ///     .await?;
    /// ```
    pub fn add_tools(mut self, tools: impl IntoIterator<Item = Box<dyn DynTool>>) -> Self {
        self.tools.extend(tools);
        self
    }

    /// Add multiple trusted tools to the agent with automatic permission grants
    ///
    /// This is a convenience method that adds the tools and automatically grants
    /// permission for them to execute. Use this for tools you trust completely.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use mixtape_tools::sqlite;
    ///
    /// // Add all read-only SQLite tools as trusted
    /// let agent = Agent::builder()
    ///     .bedrock(ClaudeHaiku4_5)
    ///     .add_trusted_tools(sqlite::read_only_tools())
    ///     .build()
    ///     .await?;
    /// ```
    pub fn add_trusted_tools(mut self, tools: impl IntoIterator<Item = Box<dyn DynTool>>) -> Self {
        for tool in tools {
            let tool_name = tool.name().to_string();
            self.tools.push(tool);
            self.trusted_tools.push(tool_name);
        }
        self
    }

    /// Set the system prompt
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    /// Set the maximum number of tools that can execute concurrently
    pub fn with_max_concurrent_tools(mut self, max: usize) -> Self {
        self.max_concurrent_tools = max;
        self
    }

    // Authorization methods are in permission.rs:
    // - with_grant_store
    // - with_authorization_timeout

    /// Set a custom conversation manager
    pub fn with_conversation_manager(
        mut self,
        manager: impl crate::conversation::ConversationManager + 'static,
    ) -> Self {
        self.conversation_manager = Some(Box::new(manager));
        self
    }

    /// Enable session management for conversation memory
    #[cfg(feature = "session")]
    pub fn with_session_store(mut self, store: impl SessionStore + 'static) -> Self {
        self.session_store = Some(Arc::new(store));
        self
    }

    // Context file methods

    /// Add literal string content as context
    ///
    /// The content will be included directly in the system prompt.
    /// Use this for dynamic context that doesn't come from a file.
    ///
    /// # Example
    /// ```ignore
    /// let agent = Agent::builder()
    ///     .bedrock(ClaudeSonnet4_5)
    ///     .add_context("# Project Rules\nAlways use async/await.")
    ///     .build()
    ///     .await?;
    /// ```
    pub fn add_context(mut self, content: impl Into<String>) -> Self {
        self.context_sources.push(ContextSource::Content {
            content: content.into(),
        });
        self
    }

    /// Add a required context file
    ///
    /// The path supports variable substitution:
    /// - `$CWD` - current working directory at resolution time
    /// - `$HOME` or `~` - user's home directory
    ///
    /// Relative paths are resolved against the current working directory.
    /// The file must exist or an error is returned at runtime.
    ///
    /// Context files are resolved at runtime (each `run()` call), allowing
    /// files to change between runs.
    ///
    /// # Example
    /// ```ignore
    /// let agent = Agent::builder()
    ///     .bedrock(ClaudeSonnet4_5)
    ///     .add_context_file("~/.config/myagent/rules.md")
    ///     .build()
    ///     .await?;
    /// ```
    pub fn add_context_file(mut self, path: impl Into<String>) -> Self {
        self.context_sources.push(ContextSource::File {
            path: path.into(),
            required: true,
        });
        self
    }

    /// Add an optional context file
    ///
    /// Same as `add_context_file()` but the file is optional.
    /// If the file doesn't exist, it will be silently skipped.
    ///
    /// # Example
    /// ```ignore
    /// let agent = Agent::builder()
    ///     .bedrock(ClaudeSonnet4_5)
    ///     .add_optional_context_file("AGENTS.md")
    ///     .build()
    ///     .await?;
    /// ```
    pub fn add_optional_context_file(mut self, path: impl Into<String>) -> Self {
        self.context_sources.push(ContextSource::File {
            path: path.into(),
            required: false,
        });
        self
    }

    /// Add multiple required context files
    ///
    /// All files must exist or an error is returned at runtime.
    /// Files are loaded in the order provided.
    ///
    /// # Example
    /// ```ignore
    /// let agent = Agent::builder()
    ///     .bedrock(ClaudeSonnet4_5)
    ///     .add_context_files(["rules.md", "examples.md"])
    ///     .build()
    ///     .await?;
    /// ```
    pub fn add_context_files(mut self, paths: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.context_sources.push(ContextSource::Files {
            paths: paths.into_iter().map(|p| p.into()).collect(),
            required: true,
        });
        self
    }

    /// Add multiple optional context files
    ///
    /// Files that exist are loaded; missing files are skipped.
    /// Files are loaded in the order provided.
    ///
    /// # Example
    /// ```ignore
    /// let agent = Agent::builder()
    ///     .bedrock(ClaudeSonnet4_5)
    ///     .add_optional_context_files(["AGENTS.md", "agents.md", "CLAUDE.md"])
    ///     .build()
    ///     .await?;
    /// ```
    pub fn add_optional_context_files(
        mut self,
        paths: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.context_sources.push(ContextSource::Files {
            paths: paths.into_iter().map(|p| p.into()).collect(),
            required: false,
        });
        self
    }

    /// Add context files matching a glob pattern
    ///
    /// The pattern supports variable substitution (same as `add_context_file()`).
    /// Files matching the pattern are sorted alphabetically and loaded in order.
    ///
    /// Glob patterns are inherently optional - zero matches is acceptable.
    ///
    /// # Example
    /// ```ignore
    /// let agent = Agent::builder()
    ///     .bedrock(ClaudeSonnet4_5)
    ///     .add_context_files_glob("$CWD/.context/*.md")
    ///     .build()
    ///     .await?;
    /// ```
    pub fn add_context_files_glob(mut self, pattern: impl Into<String>) -> Self {
        self.context_sources.push(ContextSource::Glob {
            pattern: pattern.into(),
        });
        self
    }

    /// Configure context file size limits
    ///
    /// # Example
    /// ```ignore
    /// use mixtape_core::ContextConfig;
    ///
    /// let agent = Agent::builder()
    ///     .bedrock(ClaudeSonnet4_5)
    ///     .with_context_config(ContextConfig {
    ///         max_file_size: 512 * 1024,       // 512KB per file
    ///         max_total_size: 2 * 1024 * 1024, // 2MB total
    ///     })
    ///     .with_context_pattern("$CWD/docs/*.md")
    ///     .build()
    ///     .await?;
    /// ```
    pub fn with_context_config(mut self, config: ContextConfig) -> Self {
        self.context_config = config;
        self
    }

    // MCP methods are in mcp.rs:
    // - with_mcp_server
    // - with_mcp_config_file

    /// Build the agent
    ///
    /// This is where the async provider creation happens. For Bedrock,
    /// this loads AWS credentials from the environment.
    ///
    /// # Errors
    ///
    /// Returns an error if no provider was configured (call `.bedrock()`,
    /// `.anthropic()`, or `.provider()` first).
    ///
    /// # Example
    ///
    /// ```ignore
    /// let agent = Agent::builder()
    ///     .bedrock(ClaudeHaiku4_5)
    ///     .build()
    ///     .await?;
    /// ```
    pub async fn build(self) -> crate::error::Result<Agent> {
        let provider_factory = self
            .provider_factory
            .ok_or_else(|| crate::error::Error::Config(
                "No provider configured. Call .bedrock(), .anthropic(), or .provider() before .build()".to_string()
            ))?;

        let provider = provider_factory().await?;

        let conversation_manager = self
            .conversation_manager
            .unwrap_or_else(|| Box::new(SlidingWindowConversationManager::new()));

        // Create authorizer with custom store or default MemoryGrantStore,
        // and apply the configured policy
        let authorizer = match self.grant_store {
            Some(store) => ToolCallAuthorizer::with_boxed_store(store),
            None => ToolCallAuthorizer::new(),
        }
        .with_authorization_policy(self.authorization_policy);

        // Grant permissions for trusted tools
        for tool_name in &self.trusted_tools {
            authorizer.grant_tool(tool_name).await?;
        }

        #[allow(unused_mut)]
        let mut agent = Agent {
            provider,
            system_prompt: self.system_prompt,
            max_concurrent_tools: self.max_concurrent_tools,
            tools: self.tools,
            hooks: Arc::new(parking_lot::RwLock::new(HashMap::new())),
            next_hook_id: AtomicU64::new(0),
            authorizer: Arc::new(RwLock::new(authorizer)),
            authorization_timeout: self.authorization_timeout,
            pending_authorizations: Arc::new(RwLock::new(HashMap::new())),
            #[cfg(feature = "mcp")]
            mcp_clients: Vec::new(),
            conversation_manager: parking_lot::RwLock::new(conversation_manager),
            #[cfg(feature = "session")]
            session_store: self.session_store,
            // Context file fields
            context_sources: self.context_sources,
            context_config: self.context_config,
            last_context_result: parking_lot::RwLock::new(None),
        };

        // Connect to MCP servers specified in builder
        #[cfg(feature = "mcp")]
        {
            super::mcp::connect_mcp_servers(&mut agent, self.mcp_servers, self.mcp_config_files)
                .await?;
        }

        Ok(agent)
    }
}

impl Agent {
    /// Create a new AgentBuilder for fluent configuration
    ///
    /// # Example
    ///
    /// ```ignore
    /// use mixtape_core::{Agent, ClaudeHaiku4_5, Result};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<()> {
    ///     let agent = Agent::builder()
    ///         .bedrock(ClaudeHaiku4_5)
    ///         .with_system_prompt("You are a helpful assistant")
    ///         .build()
    ///         .await?;
    ///
    ///     let response = agent.run("Hello!").await?;
    ///     println!("{}", response);
    ///     Ok(())
    /// }
    /// ```
    pub fn builder() -> AgentBuilder {
        AgentBuilder::new()
    }

    // Post-construction methods are in their respective modules:
    // - add_mcp_server, add_mcp_config_file are in mcp.rs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::box_tools;
    use crate::conversation::SimpleConversationManager;
    use crate::provider::{ModelProvider, ProviderError};
    use crate::types::{ContentBlock, Message, Role, StopReason, ToolDefinition};
    use crate::ModelResponse;

    /// Mock provider for builder tests
    #[derive(Clone)]
    struct MockProvider;

    #[async_trait::async_trait]
    impl ModelProvider for MockProvider {
        fn name(&self) -> &str {
            "MockProvider"
        }

        fn max_context_tokens(&self) -> usize {
            200_000
        }

        fn max_output_tokens(&self) -> usize {
            8_192
        }

        async fn generate(
            &self,
            _messages: Vec<Message>,
            _tools: Vec<ToolDefinition>,
            _system_prompt: Option<String>,
        ) -> Result<ModelResponse, ProviderError> {
            Ok(ModelResponse {
                message: Message {
                    role: Role::Assistant,
                    content: vec![ContentBlock::Text("ok".to_string())],
                },
                stop_reason: StopReason::EndTurn,
                usage: None,
            })
        }
    }

    #[test]
    fn test_builder_creation() {
        let builder = Agent::builder();
        assert!(builder.provider_factory.is_none());
        assert!(builder.tools.is_empty());
        assert!(builder.system_prompt.is_none());
    }

    #[test]
    fn test_builder_default() {
        let builder = AgentBuilder::default();
        assert!(builder.provider_factory.is_none());
        assert_eq!(builder.max_concurrent_tools, DEFAULT_MAX_CONCURRENT_TOOLS);
        assert_eq!(builder.authorization_timeout, DEFAULT_PERMISSION_TIMEOUT);
    }

    #[test]
    fn test_builder_system_prompt() {
        let builder = Agent::builder().with_system_prompt("Test prompt");
        assert_eq!(builder.system_prompt, Some("Test prompt".to_string()));
    }

    #[test]
    fn test_builder_max_concurrent_tools() {
        let builder = Agent::builder().with_max_concurrent_tools(4);
        assert_eq!(builder.max_concurrent_tools, 4);
    }

    #[test]
    fn test_builder_conversation_manager() {
        let builder =
            Agent::builder().with_conversation_manager(SimpleConversationManager::new(100));
        assert!(builder.conversation_manager.is_some());
    }

    #[tokio::test]
    async fn test_build_with_provider() {
        let agent = Agent::builder()
            .provider(MockProvider)
            .build()
            .await
            .unwrap();

        assert_eq!(agent.provider.name(), "MockProvider");
    }

    #[tokio::test]
    async fn test_build_with_system_prompt() {
        let agent = Agent::builder()
            .provider(MockProvider)
            .with_system_prompt("Be helpful")
            .build()
            .await
            .unwrap();

        assert_eq!(agent.system_prompt, Some("Be helpful".to_string()));
    }

    #[tokio::test]
    async fn test_build_with_conversation_manager() {
        let agent = Agent::builder()
            .provider(MockProvider)
            .with_conversation_manager(SimpleConversationManager::new(100))
            .build()
            .await
            .unwrap();

        // Just verify it built successfully with custom manager
        assert_eq!(agent.provider.name(), "MockProvider");
    }

    #[tokio::test]
    async fn test_build_without_provider_fails() {
        let result = Agent::builder().build().await;
        match result {
            Err(err) => assert!(err.is_config()),
            Ok(_) => panic!("Expected error when building without provider"),
        }
    }

    #[tokio::test]
    async fn test_builder_chaining() {
        let agent = Agent::builder()
            .provider(MockProvider)
            .with_system_prompt("Test")
            .with_max_concurrent_tools(8)
            .with_authorization_timeout(Duration::from_secs(60))
            .build()
            .await
            .unwrap();

        assert_eq!(agent.system_prompt, Some("Test".to_string()));
        assert_eq!(agent.max_concurrent_tools, 8);
        assert_eq!(agent.authorization_timeout, Duration::from_secs(60));
    }

    // ===== add_tool/add_tools Builder Tests =====

    #[test]
    fn test_builder_add_tool_single() {
        use crate::tool::{Tool, ToolError, ToolResult};
        use schemars::JsonSchema;
        use serde::{Deserialize, Serialize};

        #[derive(Debug, Deserialize, Serialize, JsonSchema)]
        #[allow(dead_code)]
        struct TestInput {
            value: String,
        }

        struct TestTool;

        impl Tool for TestTool {
            type Input = TestInput;
            fn name(&self) -> &str {
                "test_tool"
            }
            fn description(&self) -> &str {
                "A test tool"
            }
            async fn execute(&self, _input: Self::Input) -> Result<ToolResult, ToolError> {
                Ok(ToolResult::text("result"))
            }
        }

        let builder = Agent::builder().add_tool(TestTool);
        assert_eq!(builder.tools.len(), 1);
        assert_eq!(builder.tools[0].name(), "test_tool");
    }

    #[test]
    fn test_builder_add_tools_multiple() {
        use crate::tool::{Tool, ToolError, ToolResult};
        use schemars::JsonSchema;
        use serde::{Deserialize, Serialize};

        #[derive(Debug, Deserialize, Serialize, JsonSchema)]
        #[allow(dead_code)]
        struct TestInput {
            value: String,
        }

        #[derive(Clone)]
        struct TestTool {
            name: &'static str,
            description: &'static str,
        }

        impl Tool for TestTool {
            type Input = TestInput;
            fn name(&self) -> &str {
                self.name
            }
            fn description(&self) -> &str {
                self.description
            }
            async fn execute(&self, _input: Self::Input) -> Result<ToolResult, ToolError> {
                Ok(ToolResult::text(self.name))
            }
        }

        let builder = Agent::builder().add_tools(box_tools![
            TestTool {
                name: "tool1",
                description: "First tool",
            },
            TestTool {
                name: "tool2",
                description: "Second tool",
            },
            TestTool {
                name: "tool3",
                description: "Third tool",
            },
        ]);

        assert_eq!(builder.tools.len(), 3);
        assert_eq!(builder.tools[0].name(), "tool1");
        assert_eq!(builder.tools[1].name(), "tool2");
        assert_eq!(builder.tools[2].name(), "tool3");
    }

    #[test]
    fn test_builder_add_tools_empty() {
        use crate::tool::{Tool, ToolError, ToolResult};
        use schemars::JsonSchema;
        use serde::{Deserialize, Serialize};

        #[derive(Debug, Deserialize, Serialize, JsonSchema)]
        #[allow(dead_code)]
        struct TestInput {
            value: String,
        }

        #[allow(dead_code)]
        struct TestTool;
        impl Tool for TestTool {
            type Input = TestInput;
            fn name(&self) -> &str {
                "test"
            }
            fn description(&self) -> &str {
                "Test"
            }
            async fn execute(&self, _input: Self::Input) -> Result<ToolResult, ToolError> {
                Ok(ToolResult::text("ok"))
            }
        }

        let builder = Agent::builder().add_tools(box_tools![]);

        assert_eq!(builder.tools.len(), 0);
    }

    #[test]
    fn test_builder_add_tool_and_add_tools_chaining() {
        use crate::tool::{Tool, ToolError, ToolResult};
        use schemars::JsonSchema;
        use serde::{Deserialize, Serialize};

        #[derive(Debug, Deserialize, Serialize, JsonSchema)]
        struct TestInput {}

        struct Tool1;
        impl Tool for Tool1 {
            type Input = TestInput;
            fn name(&self) -> &str {
                "tool1"
            }
            fn description(&self) -> &str {
                "First"
            }
            async fn execute(&self, _input: Self::Input) -> Result<ToolResult, ToolError> {
                Ok(ToolResult::text("1"))
            }
        }

        #[derive(Clone)]
        struct Tool2;
        impl Tool for Tool2 {
            type Input = TestInput;
            fn name(&self) -> &str {
                "tool2"
            }
            fn description(&self) -> &str {
                "Second"
            }
            async fn execute(&self, _input: Self::Input) -> Result<ToolResult, ToolError> {
                Ok(ToolResult::text("2"))
            }
        }

        // Mix add_tool (single) with box_tools! macro
        let builder = Agent::builder()
            .add_tool(Tool1)
            .add_tools(box_tools![Tool2, Tool2]);

        assert_eq!(builder.tools.len(), 3);
        assert_eq!(builder.tools[0].name(), "tool1");
        assert_eq!(builder.tools[1].name(), "tool2");
        assert_eq!(builder.tools[2].name(), "tool2");
    }

    #[tokio::test]
    async fn test_build_with_add_tools() {
        use crate::tool::{Tool, ToolError, ToolResult};
        use schemars::JsonSchema;
        use serde::{Deserialize, Serialize};

        #[derive(Debug, Deserialize, Serialize, JsonSchema)]
        struct TestInput {}

        #[derive(Clone)]
        struct NamedTool {
            tool_name: &'static str,
            tool_desc: &'static str,
        }

        impl Tool for NamedTool {
            type Input = TestInput;
            fn name(&self) -> &str {
                self.tool_name
            }
            fn description(&self) -> &str {
                self.tool_desc
            }
            async fn execute(&self, _input: Self::Input) -> Result<ToolResult, ToolError> {
                Ok(ToolResult::text(self.tool_name))
            }
        }

        let agent = Agent::builder()
            .provider(MockProvider)
            .add_tools(box_tools![
                NamedTool {
                    tool_name: "calculator",
                    tool_desc: "Calculates things",
                },
                NamedTool {
                    tool_name: "weather",
                    tool_desc: "Gets weather",
                },
            ])
            .build()
            .await
            .unwrap();

        let tools = agent.list_tools();
        assert_eq!(tools.len(), 2);

        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"calculator"));
        assert!(names.contains(&"weather"));
    }
}
