//! Interactive Permission System Example
//!
//! Demonstrates simple tool permissions:
//!
//! 1. **EchoTool** - Fully pre-trusted (never prompts)
//! 2. **DatabaseTool** - Not trusted (always prompts)
//! 3. **CommandTool** - Not trusted (always prompts)
//!
//! For real-world usage with filesystem tools, use tool groups:
//! ```rust,ignore
//! use mixtape_tools::{read_only_filesystem_tools, mutative_filesystem_tools};
//!
//! let store = MemoryGrantStore::new();
//! // Trust all read-only operations
//! for tool in read_only_filesystem_tools() {
//!     store.grant_tool(tool.name()).await?;
//! }
//! // Mutative operations require approval
//! agent.add_tools(read_only_filesystem_tools())
//!      .add_tools(mutative_filesystem_tools())
//! ```
//!
//! Run with: cargo run --example permissions --features bedrock

use mixtape_cli::{
    new_event_queue, prompt_for_approval, EventPresenter, PermissionRequest, PresentationHook,
    Verbosity,
};
use mixtape_core::{
    Agent, AgentEvent, AuthorizationResponse, ClaudeHaiku4_5, MemoryGrantStore, Tool, ToolError,
    ToolResult,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

// =============================================================================
// Tool Definitions
// =============================================================================

/// A safe tool that just echoes messages - will be fully trusted
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
struct EchoInput {
    message: String,
}

struct EchoTool;

impl Tool for EchoTool {
    type Input = EchoInput;

    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> &str {
        "Echo a message back (safe, no side effects)"
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::text(format!("Echo: {}", input.message)))
    }
}

/// A database tool with read/write operations - will require approval
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
struct DatabaseInput {
    /// Operation type: "read" or "write"
    operation: String,
    /// Table name to operate on
    table: String,
}

#[derive(Serialize)]
struct DatabaseResult {
    operation: String,
    table: String,
    rows_affected: u32,
    success: bool,
}

struct DatabaseTool;

impl Tool for DatabaseTool {
    type Input = DatabaseInput;

    fn name(&self) -> &str {
        "database"
    }

    fn description(&self) -> &str {
        "Perform database operations (read or write on tables)"
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let result = DatabaseResult {
            operation: input.operation,
            table: input.table,
            rows_affected: 42,
            success: true,
        };
        Ok(ToolResult::json(&result)?)
    }
}

/// A shell command tool - completely untrusted
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
struct CommandInput {
    /// Shell command to execute
    command: String,
}

struct CommandTool;

impl Tool for CommandTool {
    type Input = CommandInput;

    fn name(&self) -> &str {
        "command"
    }

    fn description(&self) -> &str {
        "Execute a shell command (dangerous! requires approval)"
    }

    async fn execute(&self, _input: Self::Input) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::text(""))
    }
}

// =============================================================================
// Main
// =============================================================================

#[tokio::main]
async fn main() -> mixtape_core::Result<()> {
    // Create memory store with pre-configured grants
    let store = MemoryGrantStore::new();
    store.grant_tool("echo").await.unwrap();

    println!();
    println!("\x1b[1;36m========================================\x1b[0m");
    println!("\x1b[1;36m  Interactive Permission System Demo    \x1b[0m");
    println!("\x1b[1;36m========================================\x1b[0m");
    println!();
    println!("\x1b[1mPre-configured grants:\x1b[0m");
    println!("  \x1b[32m+\x1b[0m echo      -> trusted (auto-approves)");
    println!("  \x1b[31m-\x1b[0m database  -> not trusted (prompts)");
    println!("  \x1b[31m-\x1b[0m command   -> not trusted (prompts)");
    println!();

    let agent = Agent::builder()
        .bedrock(ClaudeHaiku4_5)
        .interactive()
        .with_system_prompt(
            "You are a helpful assistant with access to three tools:
1. echo - Echo a message back
2. database - Perform database operations (read/write on tables)
3. command - Execute shell commands

When asked to demonstrate the tools, use them in sequence.",
        )
        .add_tool(EchoTool)
        .add_tool(DatabaseTool)
        .add_tool(CommandTool)
        .with_grant_store(store)
        .build()
        .await?;

    let agent = Arc::new(agent);

    // Set up standard presentation hook
    let event_queue = new_event_queue();
    agent.add_hook(PresentationHook::new(Arc::clone(&event_queue)));

    let verbosity = Arc::new(Mutex::new(Verbosity::Normal));
    let presenter = EventPresenter::new(Arc::clone(&agent), verbosity, Arc::clone(&event_queue));

    // Channel for permission requests
    let (perm_tx, mut perm_rx) = tokio::sync::mpsc::unbounded_channel::<(String, String, String)>();
    agent.add_hook(move |event: &AgentEvent| {
        if let AgentEvent::PermissionRequired {
            proposal_id,
            tool_name,
            params_hash,
            ..
        } = event
        {
            let _ = perm_tx.send((proposal_id.clone(), tool_name.clone(), params_hash.clone()));
        }
    });

    println!("\x1b[2m----------------------------------------\x1b[0m");
    println!("\x1b[1mAsking agent to use all tools...\x1b[0m");
    println!("\x1b[2m----------------------------------------\x1b[0m");

    let agent_clone = agent.clone();
    let mut response_handle = tokio::spawn(async move {
        agent_clone
            .run(
                "Please demonstrate all three tools:
1. First, use the echo tool to say 'Hello World'
2. Then, use the database tool to READ from the 'users' table
3. Then, use the database tool to WRITE to the 'orders' table
4. Finally, use the command tool to run 'ls -la'

Execute each tool one at a time and report the results.",
            )
            .await
    });

    loop {
        tokio::select! {
            Some((proposal_id, tool_name, params_hash)) = perm_rx.recv() => {
                presenter.flush();

                let request = PermissionRequest {
                    tool_name: tool_name.clone(),
                    tool_use_id: proposal_id.clone(),
                    params_hash,
                    formatted_display: None,
                };

                let choice = prompt_for_approval(&request);
                match choice {
                    AuthorizationResponse::Once => {
                        agent.authorize_once(&proposal_id).await.ok();
                    }
                    AuthorizationResponse::Trust { grant } => {
                        agent
                            .respond_to_authorization(&proposal_id, AuthorizationResponse::Trust { grant })
                            .await
                            .ok();
                    }
                    AuthorizationResponse::Deny { reason } => {
                        agent.deny_authorization(&proposal_id, reason).await.ok();
                    }
                }
            }
            result = &mut response_handle => {
                presenter.flush();
                let response = result.unwrap()?;
                println!();
                println!("\x1b[2m----------------------------------------\x1b[0m");
                println!("\x1b[1mAgent response:\x1b[0m");
                println!("\x1b[2m----------------------------------------\x1b[0m");
                println!("{}", response);
                break;
            }
        }
    }

    Ok(())
}
