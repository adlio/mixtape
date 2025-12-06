// Example demonstrating streaming token-by-token responses with Anthropic's direct API
//
// This shows how Agent.run() streams internally, emitting ModelCallStreaming
// events via hooks. You get real-time output while keeping all Agent features.
//
// Also demonstrates:
// - Context usage monitoring via get_context_usage()
// - NoOpConversationManager for one-shot tasks (no context management overhead)
//
// Prerequisites: Set ANTHROPIC_API_KEY environment variable
//
// Run with: cargo run --example streaming_anthropic --features anthropic

use mixtape_core::{Agent, AgentEvent, ClaudeHaiku4_5, NoOpConversationManager};
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Mixtape Streaming Example (Anthropic)\n");
    println!("Watch the response stream in real-time!\n");
    println!("{}", "=".repeat(60));

    // Track characters for stats
    let char_count = Arc::new(AtomicUsize::new(0));
    let char_count_clone = Arc::clone(&char_count);

    // For one-shot tasks, use NoOpConversationManager
    // (No context window management - slightly more efficient for single queries)
    let agent = Agent::builder()
        .anthropic_from_env(ClaudeHaiku4_5)
        .with_conversation_manager(NoOpConversationManager::new())
        .build()
        .await?;

    // Add hook to print streaming tokens
    agent.add_hook(move |event: &AgentEvent| {
        if let AgentEvent::ModelCallStreaming { delta, .. } = event {
            print!("{}", delta);
            let _ = std::io::stdout().flush();
            char_count_clone.fetch_add(delta.len(), Ordering::Relaxed);
        }
    });

    let question = "Tell me a short story about a robot learning to code in Rust.";
    println!("\n> {}\n", question);
    print!("Assistant: ");
    std::io::stdout().flush()?;

    // Agent.run() streams internally, emitting events via hooks
    let response = agent.run(question).await?;

    let total_chars = char_count.load(Ordering::Relaxed);

    println!("\n\n{}", "=".repeat(60));
    println!("Execution Stats:");
    println!(
        "  Streamed {} characters in {:.2}s ({:.0} chars/sec)",
        total_chars,
        response.duration.as_secs_f64(),
        total_chars as f64 / response.duration.as_secs_f64()
    );
    println!("  Model calls: {}", response.model_calls);

    // Show context usage (demonstrates the API even for one-shot)
    let usage = agent.get_context_usage();
    println!(
        "  Context: {} messages, ~{} tokens",
        usage.total_messages, usage.context_tokens
    );

    Ok(())
}
