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
//! Use the Makefile to deploy infrastructure (CloudFormation) and the agent
//! runtime in a single command:
//!
//! ```sh
//! # From the mixtape-server/ directory:
//! make deploy STACK_NAME=my-agent
//! ```
//!
//! This will:
//! 1. Deploy a CloudFormation stack (ECR repository + IAM role)
//! 2. Build an ARM64 Docker image
//! 3. Push the image to ECR
//! 4. Create or update the AgentCore runtime
//!
//! To deploy a custom binary instead of this example:
//! ```sh
//! make deploy STACK_NAME=my-agent BINARY=my_custom_agent
//! ```
//!
//! Check deployment status:
//! ```sh
//! make status STACK_NAME=my-agent
//! ```
//!
//! Tear down everything:
//! ```sh
//! make clean STACK_NAME=my-agent
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
