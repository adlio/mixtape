//! AWS Bedrock AgentCore server example.
//!
//! Builds a mixtape agent as a server that implements the AgentCore HTTP
//! protocol contract, ready to run inside an AgentCore runtime.
//!
//! # Run locally
//!
//! ```sh
//! cargo run -p mixtape-server --example agentcore_server --features agentcore
//! ```
//!
//! # Test with curl
//!
//! Health check:
//! ```sh
//! curl http://localhost:8080/ping
//! ```
//!
//! Invoke the agent (streaming):
//! ```sh
//! curl -X POST http://localhost:8080/invocations \
//!   -H "Content-Type: application/json" \
//!   -d '{"prompt": "What is Rust?"}' \
//!   -N
//! ```
//!
//! # Deploy to AgentCore
//!
//! Install the deploy CLI and run it from your project root:
//!
//! ```sh
//! cargo install cargo-mixtape
//! cargo mixtape deploy
//! ```
//!
//! Configure in your Cargo.toml:
//!
//! ```toml
//! [package.metadata.mixtape]
//! agent-name = "my-agent"
//! region = "us-west-2"
//! ```
//!
//! Or pass options directly:
//! ```sh
//! cargo mixtape deploy --name my-agent --region us-west-2
//! ```
//!
//! Other commands:
//! ```sh
//! cargo mixtape local              # build and run locally
//! cargo mixtape status             # check deployment status
//! cargo mixtape destroy            # tear down everything
//! ```

use mixtape_core::{Agent, ClaudeHaiku4_5};
use mixtape_server::MixtapeRouter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Build the agent with all tools trusted (no interactive permissions)
    // since AgentCore runs agents headlessly without a human in the loop.
    let agent = Agent::builder()
        .bedrock(ClaudeHaiku4_5)
        .with_system_prompt("You are a helpful assistant running on AWS Bedrock AgentCore.")
        .build()
        .await?;

    // Build the router with AgentCore endpoints
    let app = MixtapeRouter::new(agent).with_agentcore().build()?;

    // AgentCore expects the container to listen on port 8080
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    println!("AgentCore server running on port 8080");
    println!("  Health check: GET  http://localhost:8080/ping");
    println!("  Invocations:  POST http://localhost:8080/invocations");

    axum::serve(listener, app).await?;

    Ok(())
}
