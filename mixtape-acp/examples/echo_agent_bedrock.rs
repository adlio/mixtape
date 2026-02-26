// ACP agent backed by Claude Haiku 4.5 on Bedrock.
//
// This example starts an ACP server over stdio that creates a real
// mixtape Agent for each session, using Claude Haiku 4.5 via AWS Bedrock.
// Requires valid AWS credentials in the environment.
//
// Run with:
//   cargo run -p mixtape-acp --example echo_agent_bedrock

use mixtape_acp::{serve_stdio, MixtapeAcpBuilder};
use mixtape_core::{Agent, ClaudeHaiku4_5};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = MixtapeAcpBuilder::new("bedrock-agent", env!("CARGO_PKG_VERSION"))
        .with_agent_factory(|| async {
            Agent::builder()
                .bedrock(ClaudeHaiku4_5)
                .with_system_prompt("You are a helpful coding assistant.")
                .build()
                .await
        })
        .build()?;

    serve_stdio(server).await?;
    Ok(())
}
