// Example demonstrating multi-turn conversations
//
// This shows how the Agent maintains conversation context across multiple
// calls to run() - no session storage required for in-memory conversations.
//
// Key concepts:
// - ConversationManager (built-in) handles in-memory context automatically
// - SessionStore (optional) persists context across process restarts
// - get_context_usage() monitors token consumption
//
// Run with: cargo run --example multi_turn

use mixtape_core::{Agent, ClaudeHaiku4_5};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Multi-Turn Conversation Demo ===\n");
    println!("This example shows how the agent remembers context");
    println!("across multiple run() calls - no persistence needed!\n");

    // Create agent WITHOUT session store
    // ConversationManager (default: SlidingWindow) handles context in memory
    let agent = Agent::builder().bedrock(ClaudeHaiku4_5).build().await?;

    // First message - introduce ourselves
    println!("You: Hello! My name is Alice and I love Rust programming.");
    let response1 = agent
        .run("Hello! My name is Alice and I love Rust programming.")
        .await?;
    println!("Agent: {}\n", response1);

    // Check context usage after first exchange
    let usage = agent.get_context_usage();
    println!(
        "[Context: {} messages, ~{} tokens, {:.1}% of window]\n",
        usage.total_messages,
        usage.context_tokens,
        usage.usage_percentage * 100.0
    );

    // Second message - test memory
    println!("You: What's my name and what do I like?");
    let response2 = agent.run("What's my name and what do I like?").await?;
    println!("Agent: {}\n", response2);

    // Check context usage again
    let usage = agent.get_context_usage();
    println!(
        "[Context: {} messages, ~{} tokens, {:.1}% of window]\n",
        usage.total_messages,
        usage.context_tokens,
        usage.usage_percentage * 100.0
    );

    // Third message - test deeper context
    println!("You: Based on what I told you, suggest a project for me.");
    let response3 = agent
        .run("Based on what I told you, suggest a project for me.")
        .await?;
    println!("Agent: {}\n", response3);

    // Final context usage
    let usage = agent.get_context_usage();
    println!("=== Final Context Stats ===");
    println!("  Messages: {}", usage.total_messages);
    println!("  Context tokens: ~{}", usage.context_tokens);
    println!("  Max tokens: {}", usage.max_context_tokens);
    println!("  Usage: {:.1}%", usage.usage_percentage * 100.0);

    println!("\n=== Demo Complete ===");
    println!("Note: Context is lost when the process exits.");
    println!("For persistence across restarts, use .with_session_store(SqliteStore)");

    Ok(())
}
