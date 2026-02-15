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
//! 1. Build the ARM64 Docker image:
//!    ```sh
//!    docker buildx build --platform linux/arm64 \
//!      -t <account>.dkr.ecr.<region>.amazonaws.com/my-agent:latest \
//!      --push -f Dockerfile.agentcore .
//!    ```
//!
//! 2. Create an AgentCore runtime:
//!    ```sh
//!    aws bedrock-agentcore-control create-agent-runtime \
//!      --agent-runtime-name my-mixtape-agent \
//!      --agent-runtime-artifact '{
//!        "containerConfiguration": {
//!          "containerUri": "<account>.dkr.ecr.<region>.amazonaws.com/my-agent:latest"
//!        }
//!      }' \
//!      --network-configuration '{"networkMode": "PUBLIC"}' \
//!      --role-arn arn:aws:iam::<account>:role/AgentCoreExecutionRole
//!    ```
//!
//! 3. Invoke remotely:
//!    ```sh
//!    aws bedrock-agentcore invoke-agent-runtime \
//!      --agent-runtime-arn <arn> \
//!      --payload '{"prompt": "Hello!"}' \
//!      --content-type application/json
//!    ```

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
