# Mixtape

[![CI](https://github.com/adlio/mixtape/actions/workflows/ci.yml/badge.svg)](https://github.com/adlio/mixtape/actions/workflows/ci.yml)
[![Coverage](https://codecov.io/gh/adlio/mixtape/branch/main/graph/badge.svg)](https://codecov.io/gh/adlio/mixtape)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

An agentic AI framework for Rust.

## Quick Start

```rust
use mixtape_core::{Agent, ClaudeHaiku4_5, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let agent = Agent::builder()
        .bedrock(ClaudeHaiku4_5) // Leverages AWS environment credentials
        .with_system_prompt("You are a pirate. Always respond in pirate speak.")
        .build()
        .await?;

    let response = agent.run("What is the capital of France?").await?;
    println!("{}", response);
    Ok(())
}
```

Run with `cargo run --example hello_world --features bedrock`.

## Cargo Features

Enable only what you need. All agents need `mixtape-core` with one of the provider features enabled (`"bedrock"`, or
`"anthropic"`). Add `mixtape-tools` to
leverage foundational agentic tools.

```toml
# In your Cargo.toml

[dependencies]
mixtape-core = { version = "0.1", features = ["bedrock"] }
```

| Feature     | Description            |
|-------------|------------------------|
| `bedrock`   | AWS Bedrock provider   |
| `anthropic` | Anthropic API provider |
| `mcp`       | Connect to MCP servers |
| `session`   | Session persistence    |

Add `mcp` for MCP server integration, `session` for conversation persistence.

## Workspace Crates

This repository contains four crates:

| Crate                     | Purpose                                                |
|---------------------------|--------------------------------------------------------|
| **mixtape-core**          | Core agent framework                                   |
| **mixtape-tools**         | Pre-built filesystem, process, web, and database tools |
| **mixtape-cli**           | Session storage and interactive REPL features          |
| **mixtape-anthropic-sdk** | Low-level Anthropic API client (used internally)       |

Most projects need only `mixtape-core`. Add `mixtape-tools` for ready-to-use tools.

## Tools Defined in Idiomatic Rust

Define tools with Rust types. Agent tool schemas generate automatically.

```rust
use mixtape_core::{Tool, ToolResult, ToolError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, JsonSchema)]
struct WeatherInput {
    /// City name to get weather for
    city: String,
}

struct WeatherTool;

impl Tool for WeatherTool {
    type Input = WeatherInput;

    fn name(&self) -> &str { "get_weather" }
    fn description(&self) -> &str { "Get current weather for a city" }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        // Hard-coded for demo simplicity. In real life your weather API retrieval code would be here.
        Ok(format!("Weather in {}: sunny", input.city).into())
    }
}
```

Doc comments on fields become descriptions in the JSON schema. The model sees these when deciding how to call your tool.

See [`weather_tool.rs`](mixtape/examples/weather_tool.rs) for a complete example.

### Tool Results

Tools return `ToolResult`, which supports several content types:

```rust
// Text - strings convert automatically
Ok("Success".into())
Ok(ToolResult::text("Done"))

// JSON - for structured data
ToolResult::json(my_struct)?

// Images - for multimodal models
ToolResult::image(ImageFormat::Png, bytes)

// Documents - PDFs, spreadsheets, etc.
ToolResult::document(DocumentFormat::Pdf, bytes)
ToolResult::document_with_name(DocumentFormat::Csv, bytes, "report.csv")
```

### Parallel Execution

When the model requests multiple tools, they run concurrently:

```rust
let agent = Agent::builder()
    .bedrock(ClaudeSonnet4_5)
    .with_max_concurrent_tools(10)
    .build()
    .await?;
```

## Conversations

The agent maintains conversation history in memory:

```rust
let agent = Agent::builder()
    .bedrock(ClaudeSonnet4_5)
    .build()
    .await?;

agent.run("My name is Alice").await?;
agent.run("What's my name?").await?;  // Remembers "Alice"

let usage = agent.get_context_usage();
println!("{}% of context used", usage.usage_percentage * 100.0);
```

The default `SlidingWindowConversationManager` is token-aware. It keeps recent messages that fit within the model's
context limit, dropping older ones as needed.

Context lives in memory and disappears when the process exits. For persistence, use a session store.

### Session Persistence

Save conversations to SQLite (requires `session` feature and `mixtape-cli` crate):

```toml
[dependencies]
mixtape = { version = "0.1", features = ["session"] }
mixtape-cli = "0.1"
```

```rust
use mixtape_cli::SqliteStore;

let store = SqliteStore::default_location()?;
let agent = Agent::builder()
    .bedrock(ClaudeSonnet4_5)
    .with_session_store(store)
    .build()
    .await?;
```

## Streaming

Receive tokens as they arrive:

```rust
use mixtape_core::AgentEvent;

agent.add_hook(|event: &AgentEvent| {
    if let AgentEvent::ModelCallStreaming { delta, .. } = event {
        print!("{}", delta);
    }
});
```

See [`streaming.rs`](mixtape/examples/streaming.rs).

## Context Files

Load context from files into the system prompt at runtime:

```rust
let agent = Agent::builder()
    .bedrock(ClaudeSonnet4_5)
    .with_system_prompt("You are a helpful assistant.")
    .add_optional_context_file("AGENTS.md")           // Optional - skipped if missing
    .add_context_file("~/.config/myagent/rules.md")   // Required - errors if missing
    .add_context_files_glob("$CWD/.context/*.md")     // Glob pattern
    .build()
    .await?;
```

Context files are resolved at runtime on each `run()` call. Path variables:

- `~` or `$HOME` - user's home directory
- `$CWD` - current working directory

Inspect loaded context after a run:

```rust
if let Some(ctx) = agent.last_context_info() {
    println!("Loaded {} files ({} bytes)", ctx.files.len(), ctx.total_bytes);
}
```

| Method                              | Behavior                    |
|-------------------------------------|-----------------------------|
| `add_context(content)`              | Inline string content       |
| `add_context_file(path)`            | Required file               |
| `add_optional_context_file(path)`   | Optional file               |
| `add_context_files([...])`          | Multiple required files     |
| `add_optional_context_files([...])` | Multiple optional files     |
| `add_context_files_glob(pattern)`   | Glob pattern (0 matches OK) |

## MCP Client

Connect to [Model Context Protocol](https://modelcontextprotocol.io/) servers (requires `mcp` feature):

```rust
use mixtape_core::mcp::{McpServerConfig, McpTransport};

let agent = Agent::builder()
    .bedrock(ClaudeSonnet4_5)
    .with_mcp_server(McpServerConfig::new(
        "filesystem",
        McpTransport::stdio("npx")
            .args(["-y", "@modelcontextprotocol/server-filesystem"])
        ))
    .with_mcp_server(McpServerConfig::new(
        "gitmcp",
        McpTransport::http("https://gitmcp.io/owner/repo")
    ))
    .build()
    .await?;
```

Load from Claude Desktop/Code config files:

```rust
let agent = Agent::builder()
    .bedrock(ClaudeSonnet4_5)
    .with_mcp_config_file("~/.claude.json")
    .build()
    .await?;
```

## Hierarchical Agents

Wrap agents as tools to create orchestrator patterns:

```rust
struct SpecialistTool {
    agent: Agent,
}

impl Tool for SpecialistTool {
    // ...
    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        self.agent.run(&input.query).await
            .map(ToolResult::text)
            .map_err(ToolError::Custom)
    }
}
```

See [`hierarchical_agents.rs`](mixtape/examples/hierarchical_agents.rs).

## Tool Permissions

Control which tools require user approval:

```rust
use mixtape_core::MemoryGrantStore;

// Create store and pre-approve safe tools
let store = MemoryGrantStore::new();
store.grant_tool("read_file").await?;  // Trust entire tool

let agent = Agent::builder()
    .bedrock(ClaudeSonnet4_5)
    .with_grant_store(store)
    .build()
    .await?;
```

Grant entire tool groups for convenience:

```rust
use mixtape_core::MemoryGrantStore;
use mixtape_tools::read_only_filesystem_tools;

let store = MemoryGrantStore::new();

// Trust all read-only filesystem operations
for tool in read_only_filesystem_tools() {
    store.grant_tool(tool.name()).await?;
}

let agent = Agent::builder()
    .bedrock(ClaudeSonnet4_5)
    .add_tools(read_only_filesystem_tools())
    .with_grant_store(store)
    .build()
    .await?;
```

Tools without a matching grant emit `PermissionRequired` events. See [
`permissions.rs`](mixtape/examples/permissions.rs).

## Models

Mixtape supports models through AWS Bedrock or Anthropic's API:

```rust
// AWS Bedrock (requires "bedrock" feature)
Agent::builder().bedrock(ClaudeSonnet4_5).build().await?;
Agent::builder().bedrock(NovaPro).build().await?;

// Anthropic API (requires "anthropic" feature)
Agent::builder().anthropic(ClaudeSonnet4_5, api_key).build().await?;
```

Bedrock supports Claude, Nova, Mistral, Llama, Cohere, DeepSeek, and others.

## Examples

| Example                                                              | Features              | Description          |
|----------------------------------------------------------------------|-----------------------|----------------------|
| [`hello_world`](mixtape/examples/hello_world.rs)                     | `bedrock`             | Minimal agent        |
| [`multi_turn`](mixtape/examples/multi_turn.rs)                       | `bedrock`             | Conversation memory  |
| [`streaming`](mixtape/examples/streaming.rs)                         | `bedrock`             | Real-time output     |
| [`parallel_tools`](mixtape/examples/parallel_tools.rs)               | `bedrock`             | Concurrent tools     |
| [`weather_tool`](mixtape/examples/weather_tool.rs)                   | `bedrock`             | HTTP API calls       |
| [`hierarchical_agents`](mixtape/examples/hierarchical_agents.rs)     | `bedrock`             | Orchestrator pattern |
| [`interactive_agent`](mixtape/examples/interactive_agent.rs)         | `bedrock,mcp,session` | Full CLI             |
| [`interactive_mcp_agent`](mixtape/examples/interactive_mcp_agent.rs) | `bedrock,mcp,session` | MCP integration      |

Run any example:

```bash
cargo run --example hello_world --features bedrock
cargo run --example weather_tool --features bedrock
cargo run --example interactive_mcp_agent --features bedrock,mcp,session
```

## Requirements

For `bedrock` feature:

- AWS credentials configured
- Access to AWS Bedrock with your chosen models

For `anthropic` feature:

- `ANTHROPIC_API_KEY` environment variable

## Development

```bash
make test      # Run tests
make coverage  # Coverage report
make lint      # Run clippy
make fmt       # Format code
make help      # Show all targets
```
