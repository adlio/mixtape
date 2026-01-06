use mixtape_cli::{run_cli, SqliteStore};
/// Interactive full-featured mixtape agent with REPL
///
/// This example demonstrates:
/// 1. All default tools from mixtape-tools (filesystem, process, web, search, edit)
/// 2. Claude Sonnet 4.5 1M (extended context) via AWS Bedrock
/// 3. Chrome DevTools MCP server integration
/// 4. Interactive REPL with session management
/// 5. Command history and special commands
/// 6. **Permission system** - using grants to control tool execution
/// 7. **Context files** - loading AGENTS.md and CLAUDE.md if present
///
/// Prerequisites:
/// - AWS credentials configured (default credentials or AWS_PROFILE)
/// - Node.js and npx available (for MCP server)
///
/// To run:
/// ```bash
/// cargo run --example interactive_agent
/// ```
use mixtape_core::mcp::{McpServerConfig, McpTransport};
use mixtape_core::{Agent, BedrockProvider, ClaudeSonnet4_5, InferenceProfile, MemoryGrantStore};

// Import all the tools from mixtape-tools
use mixtape_tools::edit::EditBlockTool;
use mixtape_tools::fetch::FetchTool;
use mixtape_tools::filesystem::{
    CreateDirectoryTool, FileInfoTool, ListDirectoryTool, MoveFileTool, ReadFileTool,
    ReadMultipleFilesTool, WriteFileTool,
};
use mixtape_tools::process::{
    ForceTerminateTool, InteractWithProcessTool, KillProcessTool, ListProcessesTool,
    ListSessionsTool, ReadProcessOutputTool, StartProcessTool,
};
use mixtape_tools::search::SearchTool;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🎵 Interactive Mixtape Agent");
    println!("============================\n");

    // 1. Configure session store
    println!("1. Configuring session storage...");
    let session_store = SqliteStore::default_location()?;
    println!("   ✓ Session store ready");

    // 2. Configure grant store with pre-approved read-only tools
    println!("\n2. Configuring permissions...");
    let grant_store = MemoryGrantStore::new();

    // Pre-approve read-only tools (grant entire tool = all invocations)
    let read_only_tools = [
        "read_file",
        "read_multiple_files",
        "list_directory",
        "file_info",
        "search",
        "fetch",
        "list_processes",
        "list_sessions",
        "read_process_output",
    ];

    for tool in read_only_tools {
        grant_store.grant_tool(tool).await?;
    }

    println!("   ✓ Read-only tools auto-approved");
    println!("   ⚠ Write tools will require approval (write_file, edit_block, etc.)");

    // 3. Configure Chrome DevTools MCP Server
    println!("\n3. Configuring Chrome DevTools MCP Server...");
    let chrome_devtools_config = McpServerConfig::new(
        "chrome-devtools",
        McpTransport::stdio("npx").args(["-y", "chrome-devtools-mcp@latest"]),
    );

    // 4. Build the agent with all tools and features
    println!("\n4. Building agent...");

    // Provider with retry callback for visibility into rate limit handling
    // US inference profile provides cross-region failover for improved reliability
    // 1M context window enabled via beta header for extended context support
    let provider = BedrockProvider::new(ClaudeSonnet4_5)
        .await?
        .with_inference_profile(InferenceProfile::US)
        .with_1m_context()
        .with_retry_callback(|info| {
            eprintln!(
                "   ⚠ Retry {}/{} in {:?} (rate limited)",
                info.attempt, info.max_attempts, info.delay
            );
        });

    println!("   Adding filesystem tools...");
    println!("   Adding process management tools...");
    println!("   Adding web and search tools...");
    println!("   Adding code editing tools...");

    let agent = Agent::builder()
        .provider(provider)
        .interactive() // Enable permission prompts for tools without grants
        .with_system_prompt(
            "You are a helpful AI assistant with access to:\n\
             - Filesystem operations (read, write, search, organize files)\n\
             - Process management (start, monitor, interact with processes)\n\
             - Web content fetching and research\n\
             - Code editing capabilities\n\
             - Real-time web search via Perplexity\n\n\
             Use these tools thoughtfully to help users accomplish their goals.\n\n\
             Note: Some tools require user approval before execution.",
        )
        // Context files (loaded if present in current directory)
        .add_optional_context_files(["AGENTS.md", "CLAUDE.md"])
        .with_max_concurrent_tools(5)
        .with_session_store(session_store)
        .with_grant_store(grant_store)
        // Filesystem tools
        .add_tool(ReadFileTool::new())
        .add_tool(WriteFileTool::new())
        .add_tool(ReadMultipleFilesTool::new())
        .add_tool(ListDirectoryTool::new())
        .add_tool(CreateDirectoryTool::new())
        .add_tool(MoveFileTool::new())
        .add_tool(FileInfoTool::new())
        // Process management tools
        .add_tool(StartProcessTool)
        .add_tool(ReadProcessOutputTool)
        .add_tool(InteractWithProcessTool)
        .add_tool(KillProcessTool)
        .add_tool(ForceTerminateTool)
        .add_tool(ListProcessesTool)
        .add_tool(ListSessionsTool)
        // Web and search tools
        .add_tool(FetchTool::new())
        .add_tool(SearchTool::new())
        // Editing tools
        .add_tool(EditBlockTool::new())
        // MCP servers
        .with_mcp_server(chrome_devtools_config)
        .build()
        .await?;

    let tool_count = agent.list_tools().len();
    println!("   ✓ Agent ready with {} tools", tool_count);

    // 5. Launch interactive REPL
    println!("\n5. Launching interactive REPL...");
    println!("   Type your messages and press Enter");
    println!();
    println!("   Permission handling:");
    println!("     - Read tools execute automatically (pre-approved)");
    println!("     - Write tools prompt for approval");
    println!("     - You can trust tools for session, project, or permanently");
    println!();
    println!("   Special commands:");
    println!("     /help     - Show help");
    println!("     /session  - Show current session info");
    println!("     /clear    - Clear conversation history");
    println!("     /exit     - Exit the CLI\n");

    run_cli(agent).await?;

    Ok(())
}
