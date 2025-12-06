use mixtape_cli::{run_cli, SqliteStore};
/// Interactive agent with MCP server integration using Anthropic's direct API
///
/// This example demonstrates how to add MCP (Model Context Protocol) tools
/// to a mixtape agent using Anthropic's API directly.
///
/// Features shown:
/// - HTTP transport for remote MCP servers (GitMCP)
/// - Stdio transport for local MCP servers (@modelcontextprotocol/server-everything)
/// - Both transports working together in a single agent
/// - Automatic tool namespacing
/// - Interactive REPL for testing
///
/// Prerequisites:
/// - ANTHROPIC_API_KEY environment variable set
/// - Node.js and npx available (for stdio MCP servers)
///
/// To run:
/// ```bash
/// cargo run --example interactive_mcp_agent_anthropic --features "anthropic mcp session"
/// ```
use mixtape_core::mcp::{McpServerConfig, McpTransport};
use mixtape_core::{Agent, AnthropicProvider, ClaudeHaiku4_5};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Interactive MCP Agent (Anthropic)");
    println!("==================================\n");

    // =========================================================================
    // MCP Server Configuration
    // =========================================================================

    // HTTP transport - remote MCP server (no local dependencies!)
    // GitMCP provides tools to search any GitHub repo's docs and code
    // Tools: fetch_documentation, search_documentation, search_code
    let gitmcp_server = McpServerConfig::new(
        "gitmcp",
        McpTransport::http("https://gitmcp.io/modelcontextprotocol/servers"),
    );

    // Stdio transport - local MCP server (requires npx)
    // The "everything" server provides demo tools for testing
    // Tools: echo, add, longRunningOperation, sampleLLM, getTinyImage, etc.
    let everything_server = McpServerConfig::new(
        "everything",
        McpTransport::stdio("npx").args(["-y", "@modelcontextprotocol/server-everything"]),
    );

    // =========================================================================
    // Build Agent with Both Transports
    // =========================================================================

    println!("Configuring agent with MCP servers...\n");

    let provider = AnthropicProvider::from_env(ClaudeHaiku4_5)?.with_retry_callback(|info| {
        eprintln!(
            "  Throttled, retry {}/{} in {:?}...",
            info.attempt, info.max_attempts, info.delay
        );
    });
    let store = SqliteStore::default_location()?;

    let mut agent = Agent::builder()
        .provider(provider)
        .with_system_prompt(
            "You are a helpful assistant with access to MCP tools.\n\
             - Use GitMCP tools to search the MCP servers repository\n\
             - Use the 'everything' server tools for demos and testing",
        )
        .with_session_store(store)
        .build()
        .await?;

    // Add HTTP transport server (remote)
    println!("  Adding 'gitmcp' server (HTTP)...");
    agent.add_mcp_server(gitmcp_server).await?;

    // Add stdio transport server (local)
    println!("  Adding 'everything' server (stdio)...");
    agent.add_mcp_server(everything_server).await?;

    // Show available tools from both servers
    let tools = agent.list_tools();
    println!("\n  Available tools ({}):", tools.len());
    for tool in &tools {
        println!("    - {}", tool.name);
    }

    // =========================================================================
    // Launch REPL
    // =========================================================================

    println!("\nLaunching interactive REPL...");
    println!("Type your messages and press Enter. Use /exit to quit.\n");
    println!("Try: \"Search the MCP docs for how to create a stdio server\"");
    println!("Try: \"Use the echo tool to say hello\"\n");

    run_cli(agent).await?;

    Ok(())
}
