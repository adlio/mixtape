//! Basic mixtape server example with AG-UI support.
//!
//! This example creates an HTTP server that exposes an agent via AG-UI protocol.
//!
//! Run with:
//! ```sh
//! cargo run -p mixtape-server --example basic_server --features agui
//! ```
//!
//! Test with curl:
//! ```sh
//! curl -X POST http://localhost:3000/api/copilotkit \
//!   -H "Content-Type: application/json" \
//!   -d '{"message": "Hello!"}' \
//!   -N
//! ```

use mixtape_core::{Agent, ClaudeHaiku4_5};
use mixtape_server::MixtapeRouter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create the agent
    let agent = Agent::builder()
        .bedrock(ClaudeHaiku4_5)
        .with_system_prompt("You are a helpful assistant.")
        .interactive() // Enable permission prompts (for demonstration)
        .build()
        .await?;

    // Build the router with AG-UI endpoint
    let app = MixtapeRouter::new(agent)
        .with_agui("/api/copilotkit") // SSE endpoint
        .build()?;

    // Start the server
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    println!("Server running at http://localhost:3000");
    println!("AG-UI endpoint: POST http://localhost:3000/api/copilotkit");
    println!("Interrupt endpoint: POST http://localhost:3000/api/copilotkit/interrupt");

    axum::serve(listener, app).await?;

    Ok(())
}
