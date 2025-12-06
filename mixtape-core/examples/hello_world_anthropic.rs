// Minimal mixtape example using Anthropic's direct API
//
// Prerequisites: Set ANTHROPIC_API_KEY environment variable
//
// Run with: cargo run --example hello_world_anthropic --features anthropic

use mixtape_core::{Agent, ClaudeHaiku4_5};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let agent = Agent::builder()
        .anthropic_from_env(ClaudeHaiku4_5)
        .with_system_prompt("You are a pirate. Always respond in pirate speak.")
        .build()
        .await?;

    let response = agent.run("What is the capital of France?").await?;

    println!("{}", response);

    Ok(())
}
