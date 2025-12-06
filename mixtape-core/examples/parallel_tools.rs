// Example demonstrating parallel tool execution
//
// This shows how mixtape executes multiple tool calls concurrently,
// significantly reducing total execution time for independent operations.
//
// Also demonstrates:
// - SimpleConversationManager for count-based context limits
// - One-shot task without session persistence
//
// Run with: cargo run --example parallel_tools

use mixtape_core::{
    Agent, BedrockProvider, InferenceProfile, NovaMicro, SimpleConversationManager, Tool,
    ToolError, ToolResult,
};
use schemars::JsonSchema;
use serde::Deserialize;
use std::time::Duration;

/// A tool that simulates work by sleeping for a specified duration
///
/// Doc comments on fields become "description" in the JSON schema,
/// helping the model understand how to call the tool correctly.
#[derive(Debug, Deserialize, JsonSchema)]
struct SlowInput {
    /// Name to identify this task
    name: String,
    /// Duration in milliseconds to sleep
    duration_ms: u64,
}

struct SlowTool;

impl Tool for SlowTool {
    type Input = SlowInput;

    fn name(&self) -> &str {
        "slow_task"
    }

    fn description(&self) -> &str {
        "Simulate a slow task by sleeping for a specified duration"
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        println!(
            "[{}] Starting (expected: {}ms)",
            input.name, input.duration_ms
        );
        let start = std::time::Instant::now();

        tokio::time::sleep(Duration::from_millis(input.duration_ms)).await;

        let actual = start.elapsed().as_millis();
        println!("[{}] Completed (actual: {}ms)", input.name, actual);

        Ok(format!(
            "Task '{}' completed (expected: {}ms, actual: {}ms)",
            input.name, input.duration_ms, actual
        )
        .into())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Parallel Tool Execution Demo ===\n");
    println!("Watch the timestamps to see parallel execution!\n");

    // US inference profile provides cross-region failover for improved reliability
    let provider = BedrockProvider::new(NovaMicro)
        .await?
        .with_inference_profile(InferenceProfile::US);

    // For tool-heavy agents, SimpleConversationManager with a low limit
    // keeps context small (tool results can be verbose)
    let agent = Agent::builder()
        .provider(provider)
        .with_system_prompt("You are a helpful assistant that can run slow tasks in parallel.")
        .with_conversation_manager(SimpleConversationManager::new(10)) // Keep last 10 messages
        .with_max_concurrent_tools(12)
        .add_tool(SlowTool)
        .build()
        .await?;

    let response = agent
        .run(
            "Please run 5 slow tasks in parallel with these durations: \
             Task A (1000ms), Task B (2000ms), Task C (3000ms), Task D (5000ms), Task E (5000ms). \
             If sequential: 16000ms total. If parallel: ~5000ms (longest task).",
        )
        .await?;

    println!("\n=== Agent Response ===\n{}", response.text);

    println!("\n=== Execution Stats ===");
    println!("  Total duration: {:.2}s", response.duration.as_secs_f64());
    println!("  Model calls: {}", response.model_calls);
    println!("  Tool calls: {}", response.tool_calls.len());
    for tc in &response.tool_calls {
        println!(
            "    - {} ({:.2}s) {}",
            tc.name,
            tc.duration.as_secs_f64(),
            if tc.success { "✓" } else { "✗" }
        );
    }

    // Show context stats
    let usage = agent.get_context_usage();
    println!("\n=== Context Stats ===");
    println!("  Messages in history: {}", usage.total_messages);
    println!("  Messages in context: {}", usage.context_messages);
    println!("  (SimpleConversationManager keeps last 10, truncating older messages)");

    let expected_sequential = 16.0;
    let actual = response.duration.as_secs_f64();
    if actual < expected_sequential * 0.6 {
        println!(
            "\n  Parallel execution saved ~{:.1}s vs sequential!",
            expected_sequential - actual
        );
    }

    Ok(())
}
