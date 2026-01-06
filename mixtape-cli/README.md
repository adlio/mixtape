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

For agents that need user confirmation before running tools, use `.interactive()` with a grant store:

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
        .interactive()  // Enable permission prompting
        .with_grant_store(store)
        .build()
        .await?;

    run_cli(agent).await?;
    Ok(())
}
```

The `.interactive()` builder method configures the agent to emit `PermissionRequired` events for tools without a matching grant. The `run_cli()` function automatically handles these events by prompting the user for approval.

## Approval Prompters

The CLI provides pluggable approval UX via the `ApprovalPrompter` trait:

```rust
use mixtape_cli::{prompt_for_approval, PermissionRequest};

let request = PermissionRequest {
    tool_name: "write_file".to_string(),
    tool_use_id: "toolu_123".to_string(),
    params_hash: "abc123".to_string(),
    formatted_display: None,
};

let choice = prompt_for_approval(&request);
```

The default prompter offers four options:
- `y` - approve once (don't remember)
- `e` - trust this exact call (session)
- `t` - trust entire tool (session)
- `n` - deny

Implement `ApprovalPrompter` for custom approval UX (e.g., GUI dialogs, web interfaces).

## Custom Event Presentation

For building custom UIs that handle tool output and permissions, use the event queue pattern:

```rust
use mixtape_cli::{new_event_queue, EventPresenter, PresentationHook, Verbosity};
use std::sync::{Arc, Mutex};

// Create shared event queue
let event_queue = new_event_queue();

// Add presentation hook to agent
agent.add_hook(PresentationHook::new(Arc::clone(&event_queue)));

// Create presenter to render events
let verbosity = Arc::new(Mutex::new(Verbosity::Normal));
let presenter = EventPresenter::new(
    Arc::clone(&agent),
    verbosity,
    Arc::clone(&event_queue),
);

// Call presenter.flush() periodically to render queued events
```

See the `permissions` example for a complete implementation.

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
| `PresentationHook` | Hook that queues tool events for presentation |
| `EventPresenter` | Renders queued events with formatting |
| `new_event_queue` | Create event queue for PresentationHook |
| `Verbosity` | Output verbosity level (Quiet, Normal, Verbose) |
| `CliError` | Error type for CLI operations |
