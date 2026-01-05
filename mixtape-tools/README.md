# Mixtape Tools

Ready-to-use tools for mixtape agents. Seventeen tools across five categories: filesystem, process management, search, code editing, and web fetching.

## Quick Start

```rust
use mixtape_core::{Agent, ClaudeSonnet4_5};
use mixtape_tools::filesystem::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let agent = Agent::builder()
        .bedrock(ClaudeSonnet4_5)
        .add_tool(ReadFileTool::new())
        .add_tool(ListDirectoryTool::new())
        .build()
        .await?;

    let response = agent.run("What files are in this directory?").await?;
    println!("{}", response);
    Ok(())
}
```

## Tool Groups

For granular permission control, use tool grouping functions:

```rust
use mixtape_tools::{read_only_filesystem_tools, all_filesystem_tools, all_process_tools};

// Read-only filesystem agent
let agent = Agent::builder()
    .bedrock(ClaudeSonnet4_5)
    .add_tools(read_only_filesystem_tools())
    .build()
    .await?;

// Full filesystem access
let agent = Agent::builder()
    .bedrock(ClaudeSonnet4_5)
    .add_tools(all_filesystem_tools())
    .add_tools(all_process_tools())
    .build()
    .await?;
```

| Function | Count | Description |
|----------|-------|-------------|
| `read_only_filesystem_tools()` | 4 | Safe read operations - read files, list directories, get file info |
| `mutative_filesystem_tools()` | 3 | Destructive operations - write, create, move files |
| `all_filesystem_tools()` | 7 | All filesystem tools |
| `all_process_tools()` | 7 | All process management tools |

## Tools

### Filesystem

All filesystem tools validate paths against a configurable base directory.

| Tool | Description |
|------|-------------|
| `read_file` | Read contents with optional line range |
| `read_multiple_files` | Read several files concurrently |
| `write_file` | Write or append, creating parent directories |
| `list_directory` | List contents recursively to a given depth |
| `create_directory` | Create directories with parents |
| `move_file` | Move or rename files and directories |
| `file_info` | Get size, MIME type, timestamps |

### Process

| Tool | Description |
|------|-------------|
| `list_processes` | List running processes with CPU and memory |
| `kill_process` | Terminate by PID (SIGTERM) |
| `start_process` | Start with captured output |
| `interact_with_process` | Send input, read responses |
| `read_process_output` | Read accumulated output |
| `force_terminate` | Kill with SIGKILL |
| `list_sessions` | List active process sessions |

### Search

| Tool | Description |
|------|-------------|
| `search` | Search file contents (regex) or filenames (glob), with context lines and .gitignore support |

### Edit

| Tool | Description |
|------|-------------|
| `edit_block` | Replace text blocks with exact or fuzzy matching |

### Web

| Tool | Description |
|------|-------------|
| `fetch` | Fetch URLs, convert HTML to markdown |

## Security

### Filesystem Protection

Filesystem tools operate within a configurable base directory. Paths are canonicalized before validation. Attempts to escape with `../` fail with a clear error.

```rust
use std::path::PathBuf;
use mixtape_tools::filesystem::*;

// Restrict operations to /safe/directory
let read_tool = ReadFileTool::with_base_path(PathBuf::from("/safe/directory"));
let write_tool = WriteFileTool::with_base_path(PathBuf::from("/safe/directory"));
```

### Process Management

Process tools operate at the system level without sandboxing. Deploy with appropriate system controls.

## Tool Naming

Tools use `snake_case` names when called by the model (`read_file`, `list_processes`) but are exported as `PascalCase` structs in Rust (`ReadFileTool`, `ListProcessesTool`).

## Prelude

For convenience when implementing custom tools:

```rust
use mixtape_tools::prelude::*;

// Imports: Tool, ToolResult, ToolError, JsonSchema, Deserialize, Serialize
```
