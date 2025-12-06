//! Interactive Permission System Example
//!
//! Demonstrates simple tool permissions:
//!
//! 1. **EchoTool** - Fully pre-trusted (never prompts)
//! 2. **DatabaseTool** - Not trusted (always prompts)
//! 3. **CommandTool** - Not trusted (always prompts)
//!
//! Run with: cargo run --example permissions --features bedrock

use mixtape_cli::{prompt_for_approval, PermissionRequest, PresentationHook, Verbosity};
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
        Ok(ToolResult::text(format!(
            "Database: {} on table '{}' completed successfully",
            input.operation, input.table
        )))
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

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        // In a real implementation, this would execute the command
        Ok(ToolResult::text(format!(
            "Command executed: {}",
            input.command
        )))
    }
}

// =============================================================================
// Main
// =============================================================================

#[tokio::main]
async fn main() -> mixtape_core::Result<()> {
    // Create memory store with pre-configured grants
    let store = MemoryGrantStore::new();

    // Trust EchoTool entirely - any invocation is auto-approved
    store.grant_tool("echo").await.unwrap();

    // DatabaseTool and CommandTool have no grants - always require approval

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

    // Create the agent with the grant store
    let agent = Agent::builder()
        .bedrock(ClaudeHaiku4_5)
        .interactive() // Enable permission prompts for tools without grants
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

    // Use the CLI's standard presentation hook for tool display
    let verbosity = Arc::new(Mutex::new(Verbosity::Verbose));
    agent.add_hook(PresentationHook::new(
        Arc::clone(&agent),
        Arc::clone(&verbosity),
    ));

    // Shared state for permission responses
    #[allow(clippy::type_complexity)]
    let pending: Arc<tokio::sync::Mutex<Option<(String, String, String)>>> =
        Arc::new(tokio::sync::Mutex::new(None));
    let pending_clone = pending.clone();

    // Add hook to handle permission events only
    agent.add_hook(move |event: &AgentEvent| {
        match event {
            AgentEvent::PermissionGranted { scope, .. } => {
                if let Some(s) = scope {
                    println!("  \x1b[32m+ auto-approved\x1b[0m \x1b[2m[{}]\x1b[0m", s);
                } else {
                    println!("  \x1b[32m+ approved\x1b[0m");
                }
            }
            AgentEvent::PermissionRequired {
                proposal_id,
                tool_name,
                params_hash,
                ..
            } => {
                // Store for async handling
                let pending_inner = pending_clone.clone();
                let id = proposal_id.clone();
                let tool = tool_name.clone();
                let hash = params_hash.clone();
                tokio::spawn(async move {
                    let mut guard = pending_inner.lock().await;
                    *guard = Some((id, tool, hash));
                });
            }
            AgentEvent::PermissionDenied { reason, .. } => {
                println!("  \x1b[31m- denied: {}\x1b[0m", reason);
            }
            _ => {}
        }
    });

    // Demonstrate with a prompt that triggers all three tools
    println!("\x1b[2m----------------------------------------\x1b[0m");
    println!("\x1b[1mAsking agent to use all tools...\x1b[0m");
    println!("\x1b[2m----------------------------------------\x1b[0m");

    // Spawn the agent run in background
    let agent_clone = agent.clone();
    let response_handle = tokio::spawn(async move {
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

    // Poll for permission requests and handle them using the CLI's approval prompt
    loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Check if there's a pending permission request
        let pending_request = {
            let mut guard = pending.lock().await;
            guard.take()
        };

        if let Some((proposal_id, tool_name, params_hash)) = pending_request {
            // Use the CLI's standard approval prompt
            let request = PermissionRequest {
                tool_name: tool_name.clone(),
                tool_use_id: proposal_id.clone(),
                params_hash: params_hash.clone(),
                formatted_display: None,
            };

            let choice = prompt_for_approval(&request);

            // Respond to the agent based on user's choice
            match choice {
                AuthorizationResponse::Once => {
                    agent.authorize_once(&proposal_id).await.ok();
                }
                AuthorizationResponse::Trust { grant } => {
                    agent
                        .respond_to_authorization(
                            &proposal_id,
                            AuthorizationResponse::Trust { grant },
                        )
                        .await
                        .ok();
                }
                AuthorizationResponse::Deny { reason } => {
                    agent.deny_authorization(&proposal_id, reason).await.ok();
                }
            }
        }

        // Check if the response task is done
        if response_handle.is_finished() {
            break;
        }
    }

    // Get the final response
    let response = response_handle.await.unwrap()?;

    println!();
    println!("\x1b[2m----------------------------------------\x1b[0m");
    println!("\x1b[1mAgent response:\x1b[0m");
    println!("\x1b[2m----------------------------------------\x1b[0m");
    println!("{}", response);

    Ok(())
}
