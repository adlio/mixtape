# Mixtape CLI

Session storage and REPL utilities for mixtape agents.

## Session Persistence

Store conversations in SQLite so they survive restarts:

```rust
use mixtape_core::{Agent, ClaudeSonnet4_5};
use mixtape_cli::SqliteStore;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = SqliteStore::default_location()?;  // .mixtape/sessions.db

    let agent = Agent::builder()
        .bedrock(ClaudeSonnet4_5)
        .with_session_store(store)
        .build()
        .await?;

    agent.run("Remember this for later").await?;
    Ok(())
}
```

Sessions are scoped to the current working directory. Each directory gets its own conversation history.

### Custom Location

```rust
let store = SqliteStore::new("/path/to/sessions.db")?;
```

Parent directories are created automatically.

## Interactive REPL

Run a full-featured CLI for your agent:

```rust
use mixtape_core::{Agent, ClaudeSonnet4_5};
use mixtape_cli::run_cli;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let agent = Agent::builder()
        .bedrock(ClaudeSonnet4_5)
        .build()
        .await?;

    run_cli(agent).await?;
    Ok(())
}
```

The REPL provides:

- Command history with up/down arrows
- Reverse search with Ctrl+R
- Multi-line input with Ctrl+J
- Special commands (`/help`, `/clear`, `!shell`)
- Rich tool output formatting
- Context usage display

## Tool Permissions

For agents that need user confirmation before running tools, use the permission system:

```rust
use mixtape_core::{Agent, ClaudeSonnet4_5, MemoryGrantStore};
use mixtape_cli::run_cli;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a grant store and pre-approve safe tools
    let store = MemoryGrantStore::new();
    store.grant_tool("read_file").await?;  // Trust entire tool

    let agent = Agent::builder()
        .bedrock(ClaudeSonnet4_5)
        .with_grant_store(store)
        .build()
        .await?;

    run_cli(agent).await?;
    Ok(())
}
```

Tools without a matching grant will emit `PermissionRequired` events. The REPL handles these
by prompting the user interactively.

## Approval Prompters

The CLI provides pluggable approval UX via the `ApprovalPrompter` trait:

```rust
use mixtape_cli::{ApprovalPrompter, PermissionRequest, SimplePrompter};
use mixtape_core::AuthorizationResponse;

// Use the default prompter
let prompter = SimplePrompter;
let choice: AuthorizationResponse = prompter.prompt(&request);
```

The `SimplePrompter` offers four options:
- `y` - approve once (don't remember)
- `e` - trust this exact call (session)
- `t` - trust entire tool (session)
- `n` - deny

Implement `ApprovalPrompter` for custom approval UX (e.g., GUI dialogs, web interfaces).

## Exports

| Item | Purpose |
|------|---------|
| `SqliteStore` | SQLite-based session storage |
| `run_cli` | Interactive REPL loop |
| `ApprovalPrompter` | Trait for custom approval UX |
| `SimplePrompter` | Default approval prompter |
| `DefaultPrompter` | Type alias for `SimplePrompter` |
| `PermissionRequest` | Permission request data for prompters |
| `prompt_for_approval` | Convenience function using default prompter |
| `PresentationHook` | Rich tool output formatting hook |
| `Verbosity` | Output verbosity level |
| `CliError` | Error type for CLI operations |
