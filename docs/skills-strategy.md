# Mixtape Skills Support Strategy

This document outlines a strategy for implementing Agent Skills support in Mixtape, addressing progressive disclosure, tool search, and script execution.

## Background

### What Are Agent Skills?

Agent Skills ([agentskills.io](https://agentskills.io)) provide a standardized way to package domain expertise, tools, and automation scripts for AI agents. The key innovation is **progressive disclosure** - instead of loading all tool definitions into the context window upfront, skills allow:

1. **Level 1 (Metadata)**: Only name + description (~100 tokens per skill)
2. **Level 2 (Instructions)**: Full SKILL.md content loaded on activation (~5000 tokens)
3. **Level 3 (Resources)**: scripts/, references/, assets/ loaded on-demand

### Skills Directory Structure

```
skill-name/
├── SKILL.md           # Required: YAML frontmatter + instructions
├── scripts/           # Optional: Executable Python/Bash scripts
├── references/        # Optional: Documentation loaded on-demand
└── assets/            # Optional: Templates, data files
```

### SKILL.md Format

```yaml
---
name: github-workflow    # 1-64 chars, lowercase + hyphens
description: |           # 1-1024 chars
  Manages GitHub workflows including PR creation,
  issue triage, and code review automation.
license: MIT
compatibility: "Requires gh CLI installed"
allowed-tools: "bash read_file write_file"  # Experimental
metadata:
  version: "1.0.0"
  author: "example"
---

# GitHub Workflow Skill

[Markdown instructions for the agent...]
```

## Current Mixtape Architecture

Mixtape's tool system has these relevant characteristics:

| Component | Current State |
|-----------|--------------|
| Tool Trait | Generic `Input` type with JsonSchema derivation |
| Tool Registration | `add_tool()`, MCP integration via adapters |
| Tool Filtering | MCP servers support `only_tools()`/`exclude_tools()` |
| Permission System | Grant store + Interactive/AutoDeny policies |
| Context Building | All tools converted to `ToolDefinition` on every turn |

**Key file locations:**
- `mixtape-core/src/tool.rs` - Tool trait definition
- `mixtape-core/src/agent/tools.rs` - Tool registration/execution
- `mixtape-core/src/mcp/` - MCP server integration

---

## Question 1: Build-Time vs Runtime Skills

### Recommendation: Support Both

#### Build-Time Skills (Embedded)

**Use case**: Domain-specific agents with known skill requirements compiled into the binary.

```rust
// Proposed API
use mixtape_skills::embed_skill;

let agent = Agent::builder()
    .anthropic(ClaudeSonnet4)
    .embed_skill(include_skill!("./skills/github-workflow"))  // Compile-time
    .embed_skill(include_skill!("./skills/code-review"))
    .build()
    .await?;
```

**Implementation approach:**
- Use `include_str!()` / `include_bytes!()` macros to embed SKILL.md and resources
- Parse at compile-time via proc-macro or lazy static initialization
- Zero runtime I/O overhead for embedded skills

#### Runtime Skills (Loaded from Disk/S3)

**Use case**: Configurable agents that load skills based on user configuration, similar to MCP servers.

```rust
// Proposed API - configuration-driven
let agent = Agent::builder()
    .anthropic(ClaudeSonnet4)
    .load_skills_from_config("~/.mixtape/skills.json")  // Like MCP config
    .load_skill_directory("/path/to/skills/github-workflow")
    .load_skill_archive("s3://bucket/skills/code-review.zip")
    .build()
    .await?;
```

**Configuration format** (compatible with existing MCP config style):

```json
{
  "skills": {
    "github-workflow": {
      "source": "directory",
      "path": "~/.mixtape/skills/github-workflow"
    },
    "code-review": {
      "source": "archive",
      "url": "s3://my-bucket/skills/code-review.zip"
    },
    "local-dev": {
      "source": "directory",
      "path": "./skills/local-dev"
    }
  }
}
```

**Implementation approach:**
- `SkillLoader` trait with implementations for:
  - `DirectorySkillLoader` - reads from filesystem
  - `ArchiveSkillLoader` - extracts from ZIP (local or S3)
  - `EmbeddedSkillLoader` - from compiled-in resources
- Async loading during agent build phase
- Validate SKILL.md frontmatter using serde

---

## Question 2: Tool Trait Modifications for Progressive Disclosure

### Current Problem

Every tool's full definition (name, description, input_schema) is sent to the LLM on every turn:

```rust
// agent/run.rs:102-111
let tool_defs: Vec<ToolDefinition> = self.tools.iter()
    .map(|t| ToolDefinition {
        name: t.name().to_string(),
        description: t.description().to_string(),  // Full description every time
        input_schema: t.input_schema(),            // Full schema every time
    })
    .collect();
```

### Proposed Solution: Tiered Tool Disclosure

Add disclosure level metadata to the Tool trait:

```rust
/// Disclosure level for progressive tool loading
#[derive(Debug, Clone, Copy, Default)]
pub enum ToolDisclosure {
    /// Always include full definition (default, current behavior)
    #[default]
    Always,
    /// Include only in searches, load on-demand
    Deferred,
    /// Never include in initial context, must be explicitly activated
    Hidden,
}

pub trait Tool: Send + Sync {
    type Input: DeserializeOwned + JsonSchema;

    fn name(&self) -> &str;
    fn description(&self) -> &str;

    // NEW: Short description for search/metadata (~50 chars)
    fn summary(&self) -> &str {
        self.description()  // Default: use full description
    }

    // NEW: Disclosure level
    fn disclosure(&self) -> ToolDisclosure {
        ToolDisclosure::Always  // Default: backward compatible
    }

    // NEW: Searchable keywords for tool discovery
    fn keywords(&self) -> &[&str] {
        &[]
    }

    fn execute(&self, input: Self::Input)
        -> impl Future<Output = Result<ToolResult, ToolError>> + Send;

    fn input_schema(&self) -> Value { ... }
}
```

### Agent-Side Changes

```rust
pub struct Agent {
    tools: Vec<Box<dyn DynTool>>,
    deferred_tools: Vec<Box<dyn DynTool>>,  // NEW: Separately tracked
    active_deferred: HashSet<String>,        // NEW: Currently activated
    // ...
}

impl Agent {
    fn get_tool_definitions(&self) -> Vec<ToolDefinition> {
        let mut defs = Vec::new();

        // Always-included tools: full definition
        for tool in &self.tools {
            if matches!(tool.disclosure(), ToolDisclosure::Always) {
                defs.push(ToolDefinition::full(tool));
            }
        }

        // Deferred tools: summary only (or excluded entirely if using tool_search)
        if !self.use_tool_search {
            for tool in &self.deferred_tools {
                defs.push(ToolDefinition::summary_only(tool));
            }
        }

        // Activated deferred tools: full definition
        for name in &self.active_deferred {
            if let Some(tool) = self.find_deferred_tool(name) {
                defs.push(ToolDefinition::full(tool));
            }
        }

        defs
    }
}
```

### Automatic Progressive Disclosure for Existing Tools

To avoid requiring changes to all existing tools, provide a wrapper:

```rust
/// Wraps any tool to make it deferred
pub struct DeferredTool<T: Tool> {
    inner: T,
    summary: String,
    keywords: Vec<String>,
}

impl<T: Tool> DeferredTool<T> {
    pub fn new(tool: T) -> Self {
        Self {
            summary: truncate_to_summary(tool.description()),
            keywords: extract_keywords(tool.description()),
            inner: tool,
        }
    }

    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = summary.into();
        self
    }
}

impl<T: Tool + 'static> Tool for DeferredTool<T> {
    type Input = T::Input;

    fn disclosure(&self) -> ToolDisclosure {
        ToolDisclosure::Deferred
    }

    fn summary(&self) -> &str {
        &self.summary
    }

    // Delegate other methods to inner...
}
```

**Usage:**

```rust
let agent = Agent::builder()
    .add_tool(ReadFile)                              // Always loaded
    .add_tool(WriteFile)                             // Always loaded
    .add_deferred_tool(DeferredTool::new(JiraCreate) // Deferred
        .with_summary("Create JIRA tickets"))
    .add_deferred_tool(DeferredTool::new(SlackPost)  // Deferred
        .with_summary("Post to Slack channels"))
    .build();
```

---

## Question 3: Tool Search vs Skills vs Native Progressive Disclosure

### Analysis of the Three Approaches

| Approach | Token Efficiency | Complexity | Provider Support | User Control |
|----------|------------------|------------|------------------|--------------|
| **A: Tool Search Tool** | Excellent (defer_loading) | Low | Anthropic only* | Automatic |
| **B: External Skills** | Good (on-demand loading) | Medium | All providers | Configuration |
| **C: Native Progressive Disclosure** | Good (tiered) | Medium | All providers | Per-tool |

*Tool Search requires `advanced-tool-use-2025-11-20` header, Sonnet/Opus only.

### Recommendation: Implement All Three (Layered)

These approaches are **complementary, not competing**:

```
┌─────────────────────────────────────────────────────────────┐
│                    Provider Layer                           │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  A: Tool Search Tool (Anthropic API feature)        │   │
│  │  - Handles defer_loading flag                       │   │
│  │  - BM25/Regex search built into API                 │   │
│  │  - 10,000 tool capacity                             │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                    Mixtape Agent Layer                      │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  C: Native Progressive Disclosure                   │   │
│  │  - Tool.disclosure() → Always/Deferred/Hidden       │   │
│  │  - Automatic summary generation                     │   │
│  │  - Works with ALL providers (Bedrock, etc.)         │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                    Skills Layer                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  B: External Skills System                          │   │
│  │  - Standardized packaging (SKILL.md)                │   │
│  │  - Runtime loading from disk/S3/embedded            │   │
│  │  - scripts/ execution environment                   │   │
│  │  - Cross-agent portability                          │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

### Implementation Priority

1. **Phase 1: Tool Search Tool Support** (your uncommitted code)
   - Lowest effort (API feature, not agent logic)
   - Immediate benefit for Anthropic users
   - Foundation for defer_loading semantics

2. **Phase 2: Native Progressive Disclosure**
   - Add `disclosure()`, `summary()`, `keywords()` to Tool trait
   - Implement `DeferredTool<T>` wrapper
   - Works with Bedrock and future providers

3. **Phase 3: External Skills System**
   - Skill loader infrastructure
   - SKILL.md parser with frontmatter validation
   - Script execution sandbox (see Question 4)
   - S3/archive support

### Integration Example

```rust
let agent = Agent::builder()
    .anthropic(ClaudeSonnet4)
    .enable_tool_search()  // Phase 1: Use Anthropic's tool_search

    // Phase 2: Native tools with disclosure levels
    .add_tool(ReadFile)
    .add_deferred_tool(DeferredTool::new(DatabaseQuery))

    // Phase 3: External skills
    .load_skill("./skills/github-workflow")
    .load_skill_archive("s3://skills/data-analysis.zip")

    .build()
    .await?;
```

When `enable_tool_search()` is active with Anthropic:
- Deferred tools get `defer_loading: true` in API calls
- Skills' tools also get `defer_loading: true`
- Anthropic handles the search internally

When using Bedrock or other providers:
- Mixtape implements its own search tool
- Deferred tools show summary-only in context
- Skills activate on explicit agent request

---

## Question 4: Script Execution for Skills

### The Challenge

Skills can include `scripts/` directories with Python/Bash executables. We need a secure execution environment that:

1. Isolates script execution from the host system
2. Provides controlled access to files/network
3. Works from a compiled Rust binary
4. Supports common languages (Python, Bash, Node.js)

### Option Analysis

| Approach | Security | Complexity | Dependencies | Performance |
|----------|----------|------------|--------------|-------------|
| **Subprocess (Basic)** | Low | Low | None | Fast |
| **Docker Container** | High | Medium | Docker daemon | Medium |
| **WASM Runtime** | High | High | wasmtime | Fast |
| **E2B Sandbox** | Very High | Low | Network + API key | Medium |
| **Cloudflare Sandbox** | Very High | Medium | Network + Account | Medium |
| **gVisor/Firecracker** | Very High | Very High | Linux only | Fast |

### Recommended Approach: Tiered Execution

Implement multiple execution backends, selected by configuration:

```rust
/// Script execution environment
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum ScriptExecutor {
    /// Direct subprocess - fast but no isolation (trusted skills only)
    #[serde(rename = "subprocess")]
    Subprocess {
        /// Working directory for script execution
        working_dir: Option<PathBuf>,
        /// Environment variables to set
        env: HashMap<String, String>,
        /// Timeout in seconds
        timeout_secs: u64,
    },

    /// Docker container - good isolation, requires Docker
    #[serde(rename = "docker")]
    Docker {
        /// Base image to use
        image: String,
        /// Volume mounts (host:container)
        mounts: Vec<String>,
        /// Network mode (none, bridge, host)
        network: String,
        /// Memory limit
        memory_limit: Option<String>,
        /// CPU limit
        cpu_limit: Option<f64>,
    },

    /// E2B Cloud Sandbox - highest isolation, requires API key
    #[serde(rename = "e2b")]
    E2B {
        api_key: String,
        /// Sandbox template
        template: Option<String>,
    },

    /// WASM-based execution for supported languages
    #[serde(rename = "wasm")]
    Wasm {
        /// Allowed WASI capabilities
        capabilities: WasmCapabilities,
    },
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WasmCapabilities {
    pub filesystem_read: Vec<PathBuf>,
    pub filesystem_write: Vec<PathBuf>,
    pub network: bool,
    pub env_vars: Vec<String>,
}
```

### Script Execution Trait

```rust
#[async_trait]
pub trait ScriptRunner: Send + Sync {
    /// Execute a script and return stdout/stderr
    async fn execute(
        &self,
        script_path: &Path,
        args: &[String],
        stdin: Option<&str>,
    ) -> Result<ScriptOutput, ScriptError>;

    /// Check if this runner supports the given script type
    fn supports(&self, script_path: &Path) -> bool;
}

pub struct ScriptOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration: Duration,
}
```

### Implementation: Subprocess Runner (Phase 1)

Start with the simplest approach, gated by explicit trust:

```rust
pub struct SubprocessRunner {
    working_dir: PathBuf,
    timeout: Duration,
    env: HashMap<String, String>,
}

#[async_trait]
impl ScriptRunner for SubprocessRunner {
    async fn execute(
        &self,
        script_path: &Path,
        args: &[String],
        stdin: Option<&str>,
    ) -> Result<ScriptOutput, ScriptError> {
        let interpreter = detect_interpreter(script_path)?;

        let mut cmd = tokio::process::Command::new(&interpreter);
        cmd.arg(script_path)
            .args(args)
            .current_dir(&self.working_dir)
            .envs(&self.env)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(input) = stdin {
            cmd.stdin(Stdio::piped());
        }

        let output = tokio::time::timeout(
            self.timeout,
            cmd.output()
        ).await??;

        Ok(ScriptOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
            duration: /* measured */,
        })
    }

    fn supports(&self, script_path: &Path) -> bool {
        matches!(
            script_path.extension().and_then(|e| e.to_str()),
            Some("py" | "sh" | "bash" | "js" | "ts")
        )
    }
}

fn detect_interpreter(path: &Path) -> Result<String, ScriptError> {
    // Check shebang first
    if let Ok(content) = std::fs::read_to_string(path) {
        if content.starts_with("#!") {
            if let Some(line) = content.lines().next() {
                return Ok(line.trim_start_matches("#!").trim().to_string());
            }
        }
    }

    // Fall back to extension
    match path.extension().and_then(|e| e.to_str()) {
        Some("py") => Ok("python3".to_string()),
        Some("sh" | "bash") => Ok("bash".to_string()),
        Some("js") => Ok("node".to_string()),
        Some("ts") => Ok("npx ts-node".to_string()),
        _ => Err(ScriptError::UnsupportedLanguage),
    }
}
```

### Integration with Skills

```rust
pub struct Skill {
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub scripts: HashMap<String, SkillScript>,
    pub references: HashMap<String, String>,
    pub assets: HashMap<String, Vec<u8>>,
}

pub struct SkillScript {
    pub path: PathBuf,
    pub description: Option<String>,
    pub language: ScriptLanguage,
}

impl Skill {
    /// Create a tool that executes a skill script
    pub fn script_tool(&self, script_name: &str, runner: Arc<dyn ScriptRunner>) -> impl Tool {
        SkillScriptTool {
            skill_name: self.name.clone(),
            script: self.scripts.get(script_name).cloned(),
            runner,
        }
    }
}
```

### Security Configuration

```json
{
  "skills": {
    "github-workflow": {
      "source": "directory",
      "path": "./skills/github-workflow",
      "script_executor": {
        "type": "subprocess",
        "timeout_secs": 30,
        "env": {
          "GITHUB_TOKEN": "${GITHUB_TOKEN}"
        }
      },
      "trusted": true  // Required for subprocess execution
    },
    "untrusted-skill": {
      "source": "archive",
      "url": "https://example.com/skill.zip",
      "script_executor": {
        "type": "docker",
        "image": "python:3.11-slim",
        "network": "none",
        "memory_limit": "512m"
      },
      "trusted": false
    }
  }
}
```

### Human-in-the-Loop for Script Execution

Integrate with Mixtape's existing permission system:

```rust
impl Agent {
    async fn execute_skill_script(
        &self,
        skill: &Skill,
        script_name: &str,
        args: &[String],
    ) -> Result<ScriptOutput, AgentError> {
        // Check if skill is trusted
        if !self.is_skill_trusted(&skill.name) {
            // Use permission system for approval
            let tool_id = format!("skill_script:{}:{}", skill.name, script_name);
            self.check_tool_approval(&tool_id, /* ... */).await?;
        }

        let runner = self.get_script_runner(&skill.name)?;
        let script = skill.scripts.get(script_name)
            .ok_or(AgentError::ScriptNotFound)?;

        runner.execute(&script.path, args, None).await
            .map_err(AgentError::ScriptExecution)
    }
}
```

---

## Proposed Module Structure

```
mixtape-core/
├── src/
│   ├── skill/
│   │   ├── mod.rs           # Skill struct, SkillLoader trait
│   │   ├── parser.rs        # SKILL.md frontmatter parsing
│   │   ├── loader/
│   │   │   ├── mod.rs
│   │   │   ├── directory.rs # Load from filesystem
│   │   │   ├── archive.rs   # Load from ZIP
│   │   │   └── embedded.rs  # Compile-time embedding
│   │   └── script/
│   │       ├── mod.rs       # ScriptRunner trait
│   │       ├── subprocess.rs
│   │       ├── docker.rs
│   │       └── wasm.rs
│   ├── tool/
│   │   ├── mod.rs           # Add disclosure(), summary()
│   │   ├── deferred.rs      # DeferredTool<T> wrapper
│   │   └── search.rs        # Built-in tool search (for non-Anthropic)
│   └── agent/
│       └── tools.rs         # Update to handle disclosure levels

mixtape-skills/                # New crate (optional)
├── Cargo.toml
├── src/
│   └── lib.rs               # include_skill! proc-macro
```

---

## Implementation Roadmap

### Phase 1: Foundation (Tool Search + Disclosure)
- [ ] Merge tool_search tool support
- [ ] Add `disclosure()`, `summary()`, `keywords()` to Tool trait
- [ ] Implement `DeferredTool<T>` wrapper
- [ ] Update agent to respect disclosure levels
- [ ] Add `enable_tool_search()` builder method

### Phase 2: Skills Loading
- [ ] Create `skill/` module structure
- [ ] Implement SKILL.md parser with frontmatter validation
- [ ] `DirectorySkillLoader` implementation
- [ ] `ArchiveSkillLoader` (ZIP) implementation
- [ ] Skills configuration in JSON config files
- [ ] `load_skill()` / `load_skills_from_config()` builder methods

### Phase 3: Script Execution
- [ ] `ScriptRunner` trait definition
- [ ] `SubprocessRunner` implementation (with trust gate)
- [ ] Integration with permission system
- [ ] `DockerRunner` implementation
- [ ] Optional: WASM/E2B runners

### Phase 4: Build-Time Embedding
- [ ] `mixtape-skills` proc-macro crate
- [ ] `include_skill!` macro implementation
- [ ] `embed_skill()` builder method

---

## Open Questions

1. **Skill versioning**: Should we support multiple versions of the same skill?

2. **Skill dependencies**: Skills may depend on other skills or specific tools being available. How do we handle this?

3. **Skill marketplace/registry**: Should Mixtape have a way to fetch skills from a central registry?

4. **MCP + Skills interaction**: Can MCP servers expose skills? Should skills be able to define MCP resources?

5. **Caching**: Should extracted archives be cached? For how long?

---

## References

- [Agent Skills Specification](https://agentskills.io/specification)
- [Anthropic Tool Search Tool](https://platform.claude.com/docs/en/agents-and-tools/tool-use/tool-search-tool)
- [Claude Agent Skills Deep Dive](https://leehanchung.github.io/blogs/2025/10/26/claude-skills-deep-dive/)
- [Progressive Disclosure Pattern](https://lethain.com/agents-large-files/)
- [E2B Sandbox](https://e2b.dev/)
- [Cloudflare Sandbox SDK](https://developers.cloudflare.com/sandbox/)
