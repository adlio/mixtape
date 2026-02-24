// ACP echo agent using MockProvider — no credentials required.
//
// This example starts an ACP server over stdio that responds to every prompt
// with a fixed text reply. Useful for verifying that the ACP protocol wiring
// works end-to-end without needing real LLM credentials.
//
// Run with:
//   cargo run -p mixtape-acp --example echo_agent

use mixtape_acp::{serve_stdio, MixtapeAcpBuilder};
use mixtape_core::test_utils::MockProvider;
use mixtape_core::Agent;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = MixtapeAcpBuilder::new("echo-agent", env!("CARGO_PKG_VERSION"))
        .with_agent_factory(|| async {
            let provider = MockProvider::new().with_text("Hello from the echo agent!");
            Agent::builder().provider(provider).build().await
        })
        .build()?;

    serve_stdio(server).await?;
    Ok(())
}
