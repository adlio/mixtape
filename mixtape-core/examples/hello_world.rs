// Minimal mixtape example - ask a question, get an answer
//
// Run with: cargo run --example hello_world

use mixtape_core::{Agent, ClaudeHaiku4_5, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let agent = Agent::builder()
        .bedrock(ClaudeHaiku4_5)
        .with_system_prompt("You are a pirate. Always respond in pirate speak.")
        .build()
        .await?;

    let response = agent.run("What is the capital of France?").await?;

    println!("{}", response);

    Ok(())
}
