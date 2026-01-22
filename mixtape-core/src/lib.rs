//! # Mixtape
//!
//! A Rust SDK for building AI agents with tool use, streaming, and multi-provider support.
//!
//! Mixtape provides a high-level API for creating conversational AI agents that can use tools,
//! stream responses, and work with multiple LLM providers (AWS Bedrock, Anthropic API).
//!
//! ## Quick Start
//!
//! ```ignore
//! use mixtape_core::{Agent, ClaudeSonnet4_5};
//!
//! #[tokio::main]
//! async fn main() -> mixtape_core::Result<()> {
//!     // Create an agent with Bedrock provider
//!     let agent = Agent::builder()
//!         .bedrock(ClaudeSonnet4_5)
//!         .with_system_prompt("You are a helpful assistant.")
//!         .build()
//!         .await?;
//!
//!     // Run a conversation
//!     let response = agent.run("What is 2 + 2?").await?;
//!     println!("{}", response);
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Features
//!
//! - **Multiple Providers**: Support for AWS Bedrock and Anthropic API
//! - **Tool Use**: Define custom tools with automatic JSON schema generation
//! - **Streaming**: Real-time response streaming with event hooks
//! - **Session Management**: Persist conversations across runs (optional)
//! - **MCP Support**: Connect to Model Context Protocol servers (optional)
//! - **Extended Thinking**: Enable Claude's reasoning capabilities
//!
//! ## Creating Agents
//!
//! Use the builder pattern to create agents:
//!
//! ```ignore
//! use mixtape_core::{Agent, ClaudeSonnet4_5};
//!
//! # async fn example() -> mixtape_core::Result<()> {
//! // Using AWS Bedrock
//! let agent = Agent::builder()
//!     .bedrock(ClaudeSonnet4_5)
//!     .build()
//!     .await?;
//!
//! // Using Anthropic API directly
//! let agent = Agent::builder()
//!     .anthropic(ClaudeSonnet4_5, "sk-ant-api-key")
//!     .build()
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Adding Tools
//!
//! Implement the [`Tool`] trait to create custom tools:
//!
//! ```ignore
//! use mixtape_core::{Tool, ToolError, ToolResult};
//! use schemars::JsonSchema;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, Deserialize, Serialize, JsonSchema)]
//! struct CalculatorInput {
//!     expression: String,
//! }
//!
//! struct Calculator;
//!
//! impl Tool for Calculator {
//!     type Input = CalculatorInput;
//!
//!     fn name(&self) -> &str { "calculator" }
//!     fn description(&self) -> &str { "Evaluate a math expression" }
//!
//!     async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
//!         // Parse and evaluate the expression
//!         Ok(ToolResult::text("42"))
//!     }
//! }
//! ```
//!
//! Add tools to an agent with `add_tool()` or `add_tools()`:
//!
//! ```ignore
//! use mixtape_core::{Agent, box_tools, ClaudeSonnet4};
//!
//! // Single tool
//! let agent = Agent::builder()
//!     .bedrock(ClaudeSonnet4)
//!     .add_tool(Calculator)
//!     .build()
//!     .await?;
//!
//! // Multiple heterogeneous tools with the box_tools! macro
//! let agent = Agent::builder()
//!     .bedrock(ClaudeSonnet4)
//!     .add_tools(box_tools![Calculator, WeatherLookup, FileReader])
//!     .build()
//!     .await?;
//!
//! // Tool groups from mixtape-tools
//! use mixtape_tools::sqlite;
//!
//! let agent = Agent::builder()
//!     .bedrock(ClaudeSonnet4)
//!     .add_tools(sqlite::read_only_tools())
//!     .build()
//!     .await?;
//! ```
//!
//! ## Feature Flags
//!
//! - `bedrock` - AWS Bedrock provider support (enabled by default)
//! - `anthropic` - Anthropic API provider support
//! - `session` - Session persistence for multi-turn conversations
//! - `mcp` - Model Context Protocol server integration

pub mod agent;
pub mod conversation;
pub mod error;
pub mod events;
pub mod model;
pub mod models;
pub mod permission;
pub mod presentation;
pub mod provider;
pub mod tokenizer;
pub mod tool;
pub mod types;

#[cfg(feature = "mcp")]
pub mod mcp;

#[cfg(feature = "session")]
pub mod session;

pub use agent::{
    Agent, AgentBuilder, AgentError, AgentResponse, ContextConfig, ContextError, ContextLoadResult,
    ContextSource, PermissionError, TokenUsageStats, ToolCallInfo, ToolInfo,
    DEFAULT_MAX_CONCURRENT_TOOLS, DEFAULT_PERMISSION_TIMEOUT,
};
pub use conversation::{
    BoxedConversationManager, ContextLimits, ContextUsage, ConversationManager,
    NoOpConversationManager, SimpleConversationManager, SlidingWindowConversationManager,
    TokenEstimator,
};
pub use error::{Error, Result};
pub use events::{AgentEvent, AgentHook, TokenUsage};

pub use model::{
    AnthropicModel, BedrockModel, InferenceProfile, Model, ModelRequest, ModelResponse,
};

// Permission system
pub use permission::{
    hash_params, Authorization, AuthorizationResponse, FileGrantStore, Grant, GrantStore,
    GrantStoreError, MemoryGrantStore, Scope, ToolAuthorizationPolicy, ToolCallAuthorizer,
};
pub use presentation::Display;

// Providers - core types always available
pub use provider::{ModelProvider, ProviderError, RetryConfig, RetryInfo, StreamEvent};

// Provider implementations - feature-gated
#[cfg(feature = "anthropic")]
pub use provider::AnthropicProvider;
#[cfg(feature = "bedrock")]
pub use provider::BedrockProvider;

// Models (organized by vendor)
pub use models::{
    // Anthropic Claude
    Claude3_7Sonnet,
    ClaudeHaiku4_5,
    ClaudeOpus4,
    ClaudeOpus4_5,
    ClaudeSonnet4,
    ClaudeSonnet4_5,
    // Cohere
    CohereCommandRPlus,
    // DeepSeek
    DeepSeekR1,
    DeepSeekV3,
    // Google
    Gemma3_27B,
    // Moonshot Kimi
    KimiK2Thinking,
    // Meta Llama
    Llama3_1_405B,
    Llama3_1_70B,
    Llama3_1_8B,
    Llama3_2_11B,
    Llama3_2_1B,
    Llama3_2_3B,
    Llama3_2_90B,
    Llama3_3_70B,
    Llama4Maverick17B,
    Llama4Scout17B,
    // Mistral
    MagistralSmall,
    MistralLarge3,
    // Amazon Nova
    Nova2Lite,
    NovaLite,
    NovaMicro,
    NovaPremier,
    NovaPro,
    // Alibaba Qwen
    Qwen3Coder480B,
    Qwen3_235B,
};

pub use tokenizer::CharacterTokenizer;
pub use tool::{box_tool, DocumentFormat, DynTool, ImageFormat, Tool, ToolError, ToolResult};
pub use types::{
    ContentBlock, Message, Role, ServerToolUseBlock, StopReason, ThinkingConfig, ToolDefinition,
    ToolReference, ToolResultBlock, ToolResultStatus, ToolSearchResultBlock, ToolSearchType,
    ToolUseBlock,
};

#[cfg(feature = "session")]
pub use agent::SessionInfo;

#[cfg(feature = "session")]
pub use session::{
    MessageRole, Session, SessionError, SessionMessage, SessionStore, SessionSummary, ToolCall,
    ToolResult as SessionToolResult,
};
