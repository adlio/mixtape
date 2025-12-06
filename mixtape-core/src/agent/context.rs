//! Context file support for agents
//!
//! This module provides builder methods and runtime resolution for
//! loading context files that are prepended to the system prompt.
//!
//! Context files are resolved at runtime (each `agent.run()` call),
//! allowing files to change between runs.
//!
//! ## Path Variables
//!
//! Paths support variable expansion:
//! - `$CWD` - current working directory at resolution time
//! - `$HOME` or `~` - user's home directory
//!
//! Relative paths (without prefix) are resolved relative to the current
//! working directory.
//!
//! ## Examples
//!
//! ```ignore
//! Agent::builder()
//!     .add_context_file("~/.config/myagent/system.md")  // Required
//!     .add_optional_context_file("AGENTS.md")           // Optional
//!     .add_context_files_glob("$CWD/.context/*.md")     // Glob pattern
//!     .build()
//!     .await?;
//! ```

use std::path::{Path, PathBuf};
use thiserror::Error;

/// Default maximum size for a single context file (1MB)
pub const DEFAULT_MAX_FILE_SIZE: usize = 1024 * 1024;

/// Default maximum total size for all context files (10MB)
pub const DEFAULT_MAX_TOTAL_SIZE: usize = 10 * 1024 * 1024;

/// Represents a context source with its configuration
#[derive(Debug, Clone)]
pub enum ContextSource {
    /// Literal string content
    Content {
        /// The content to include
        content: String,
    },
    /// A single file path
    File {
        /// Path with optional $CWD, $HOME, or ~ expansion
        path: String,
        /// Whether file must exist (true = error if missing)
        required: bool,
    },
    /// Multiple file paths (all-of semantics)
    Files {
        /// Paths with optional variable expansion
        paths: Vec<String>,
        /// Whether all files must exist (true = error if any missing)
        required: bool,
    },
    /// A glob pattern (always optional, 0 matches is OK)
    Glob {
        /// Glob pattern with optional variable expansion
        pattern: String,
    },
}

/// Result of resolving a context source at runtime
#[derive(Debug, Clone)]
pub struct ResolvedContext {
    /// Description of the source (path, pattern, or "inline content")
    pub source: String,
    /// The resolved absolute path (None for inline content)
    pub resolved_path: Option<PathBuf>,
    /// The content (UTF-8)
    pub content: String,
}

/// Information about loaded context for inspection
#[derive(Debug, Clone, Default)]
pub struct ContextLoadResult {
    /// All successfully loaded context in order
    pub files: Vec<ResolvedContext>,
    /// Any files that were skipped (optional files not found)
    pub skipped: Vec<String>,
    /// Total size in bytes
    pub total_bytes: usize,
}

/// Configuration for context resolution
#[derive(Debug, Clone)]
pub struct ContextConfig {
    /// Maximum file size in bytes (default: 1MB)
    pub max_file_size: usize,
    /// Maximum total size in bytes (default: 10MB)
    pub max_total_size: usize,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_file_size: DEFAULT_MAX_FILE_SIZE,
            max_total_size: DEFAULT_MAX_TOTAL_SIZE,
        }
    }
}

/// Variables available for path substitution
#[derive(Debug, Clone)]
pub struct PathVariables {
    /// Current working directory
    pub cwd: PathBuf,
    /// User's home directory
    pub home: PathBuf,
}

impl PathVariables {
    /// Create path variables from current environment
    pub fn current() -> Self {
        Self {
            cwd: std::env::current_dir().unwrap_or_default(),
            home: dirs::home_dir().unwrap_or_default(),
        }
    }
}

/// Errors that can occur during context file loading
#[derive(Debug, Error)]
pub enum ContextError {
    /// Required file not found
    #[error("required context file not found: {0}")]
    FileNotFound(String),

    /// File is not valid UTF-8
    #[error("context file is not valid UTF-8: {path}")]
    InvalidUtf8 {
        /// Path to the invalid file
        path: String,
    },

    /// File exceeds size limit
    #[error("context file exceeds size limit ({size} bytes > {limit} bytes): {path}")]
    FileTooLarge {
        /// Path to the file
        path: String,
        /// Actual file size
        size: usize,
        /// Configured limit
        limit: usize,
    },

    /// Total context exceeds size limit
    #[error("total context size exceeds limit ({size} bytes > {limit} bytes)")]
    TotalSizeTooLarge {
        /// Total size of all files
        size: usize,
        /// Configured limit
        limit: usize,
    },

    /// IO error reading file
    #[error("failed to read context file {path}: {message}")]
    IoError {
        /// Path to the file
        path: String,
        /// Error message
        message: String,
    },

    /// Invalid glob pattern
    #[error("invalid glob pattern: {0}")]
    InvalidPattern(String),
}

/// Expand variables in a path string
///
/// Supported syntax:
/// - `$CWD` or `$CWD/...` - current working directory
/// - `$HOME` or `$HOME/...` - user's home directory
/// - `~` or `~/...` - user's home directory (shell convention)
/// - Relative paths - resolved against CWD
fn expand_path(path: &str, vars: &PathVariables) -> String {
    let home_str = vars.home.to_str().unwrap_or("");
    let cwd_str = vars.cwd.to_str().unwrap_or("");

    // Handle ~ prefix (must be at start, optionally followed by /)
    if path == "~" {
        return home_str.to_string();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return format!("{}/{}", home_str, rest);
    }

    // Handle $HOME and $CWD variables
    let mut result = path.to_string();
    result = result.replace("$HOME", home_str);
    result = result.replace("$CWD", cwd_str);

    result
}

/// Load a single file with validation
fn load_file(
    path: &Path,
    config: &ContextConfig,
    total_bytes: &mut usize,
) -> Result<String, ContextError> {
    let metadata = std::fs::metadata(path).map_err(|e| ContextError::IoError {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;

    let size = metadata.len() as usize;

    // Check individual file size
    if size > config.max_file_size {
        return Err(ContextError::FileTooLarge {
            path: path.display().to_string(),
            size,
            limit: config.max_file_size,
        });
    }

    // Check total size
    if *total_bytes + size > config.max_total_size {
        return Err(ContextError::TotalSizeTooLarge {
            size: *total_bytes + size,
            limit: config.max_total_size,
        });
    }

    // Read and validate UTF-8
    let content = std::fs::read_to_string(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::InvalidData {
            ContextError::InvalidUtf8 {
                path: path.display().to_string(),
            }
        } else {
            ContextError::IoError {
                path: path.display().to_string(),
                message: e.to_string(),
            }
        }
    })?;

    *total_bytes += size;
    Ok(content)
}

/// Resolve context sources and load file contents
///
/// Files are loaded in declaration order. For glob patterns, matched files
/// are sorted alphabetically within each pattern.
pub fn resolve_context(
    sources: &[ContextSource],
    vars: &PathVariables,
    config: &ContextConfig,
) -> Result<ContextLoadResult, ContextError> {
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    let mut total_bytes = 0usize;

    for source in sources {
        match source {
            ContextSource::Content { content } => {
                let size = content.len();
                if total_bytes + size > config.max_total_size {
                    return Err(ContextError::TotalSizeTooLarge {
                        size: total_bytes + size,
                        limit: config.max_total_size,
                    });
                }
                total_bytes += size;
                files.push(ResolvedContext {
                    source: "inline content".to_string(),
                    resolved_path: None,
                    content: content.clone(),
                });
            }

            ContextSource::File { path, required } => {
                let expanded = expand_path(path, vars);
                let resolved = PathBuf::from(&expanded);

                if !resolved.exists() {
                    if *required {
                        return Err(ContextError::FileNotFound(expanded));
                    }
                    skipped.push(expanded);
                    continue;
                }

                let content = load_file(&resolved, config, &mut total_bytes)?;
                files.push(ResolvedContext {
                    source: path.clone(),
                    resolved_path: Some(resolved),
                    content,
                });
            }

            ContextSource::Files { paths, required } => {
                for path in paths {
                    let expanded = expand_path(path, vars);
                    let resolved = PathBuf::from(&expanded);

                    if !resolved.exists() {
                        if *required {
                            return Err(ContextError::FileNotFound(expanded));
                        }
                        skipped.push(expanded);
                        continue;
                    }

                    let content = load_file(&resolved, config, &mut total_bytes)?;
                    files.push(ResolvedContext {
                        source: path.clone(),
                        resolved_path: Some(resolved),
                        content,
                    });
                }
            }

            ContextSource::Glob { pattern } => {
                let expanded = expand_path(pattern, vars);
                let matches = glob::glob(&expanded)
                    .map_err(|e| ContextError::InvalidPattern(e.to_string()))?;

                let mut pattern_files: Vec<PathBuf> = matches
                    .filter_map(|r| r.ok())
                    .filter(|p| p.is_file())
                    .collect();

                // Sort for deterministic ordering within pattern
                pattern_files.sort();

                // Glob is always optional - 0 matches is OK
                for resolved in pattern_files {
                    let content = load_file(&resolved, config, &mut total_bytes)?;
                    files.push(ResolvedContext {
                        source: pattern.clone(),
                        resolved_path: Some(resolved),
                        content,
                    });
                }
            }
        }
    }

    Ok(ContextLoadResult {
        files,
        skipped,
        total_bytes,
    })
}

/// Build the effective system prompt by combining the base prompt with context
///
/// The ordering is:
/// 1. System prompt (if set)
/// 2. Context (in declaration order)
///
/// MCP tool instructions are added by the provider after this.
pub fn build_effective_prompt(
    system_prompt: Option<&str>,
    context: &ContextLoadResult,
) -> Option<String> {
    let mut parts = Vec::new();

    // System prompt first
    if let Some(prompt) = system_prompt {
        parts.push(prompt.to_string());
    }

    // Then context (in declaration order)
    for ctx in &context.files {
        let header = match &ctx.resolved_path {
            Some(path) => format!("<!-- Context from: {} -->", path.display()),
            None => "<!-- Inline context -->".to_string(),
        };
        parts.push(format!("\n---\n{}\n{}", header, ctx.content));
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_expand_path_cwd() {
        let vars = PathVariables {
            cwd: PathBuf::from("/workspace"),
            home: PathBuf::from("/home/user"),
        };

        assert_eq!(expand_path("$CWD/AGENTS.md", &vars), "/workspace/AGENTS.md");
    }

    #[test]
    fn test_expand_path_home_var() {
        let vars = PathVariables {
            cwd: PathBuf::from("/workspace"),
            home: PathBuf::from("/home/user"),
        };

        assert_eq!(
            expand_path("$HOME/.config/agent.md", &vars),
            "/home/user/.config/agent.md"
        );
    }

    #[test]
    fn test_expand_path_tilde() {
        let vars = PathVariables {
            cwd: PathBuf::from("/workspace"),
            home: PathBuf::from("/home/user"),
        };

        assert_eq!(
            expand_path("~/.config/agent.md", &vars),
            "/home/user/.config/agent.md"
        );
    }

    #[test]
    fn test_expand_path_tilde_alone() {
        let vars = PathVariables {
            cwd: PathBuf::from("/workspace"),
            home: PathBuf::from("/home/user"),
        };

        assert_eq!(expand_path("~", &vars), "/home/user");
    }

    #[test]
    fn test_expand_path_relative() {
        let vars = PathVariables {
            cwd: PathBuf::from("/workspace"),
            home: PathBuf::from("/home/user"),
        };

        // Relative paths are not expanded by expand_path itself,
        // they're resolved by the filesystem relative to CWD
        assert_eq!(expand_path("AGENTS.md", &vars), "AGENTS.md");
    }

    #[test]
    fn test_resolve_context_content() {
        let sources = vec![ContextSource::Content {
            content: "# Rules\nBe helpful.".to_string(),
        }];

        let vars = PathVariables::current();
        let config = ContextConfig::default();

        let result = resolve_context(&sources, &vars, &config).unwrap();

        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].content, "# Rules\nBe helpful.");
        assert!(result.files[0].resolved_path.is_none());
        assert_eq!(result.files[0].source, "inline content");
    }

    #[test]
    fn test_resolve_context_single_file() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("AGENTS.md");
        fs::write(&file_path, "# Agent Instructions\nBe helpful.").unwrap();

        let sources = vec![ContextSource::File {
            path: file_path.to_str().unwrap().to_string(),
            required: true,
        }];

        let vars = PathVariables::current();
        let config = ContextConfig::default();

        let result = resolve_context(&sources, &vars, &config).unwrap();

        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].content, "# Agent Instructions\nBe helpful.");
        assert!(result.skipped.is_empty());
    }

    #[test]
    fn test_resolve_context_optional_missing() {
        let sources = vec![ContextSource::File {
            path: "/nonexistent/file.md".to_string(),
            required: false,
        }];

        let vars = PathVariables::current();
        let config = ContextConfig::default();

        let result = resolve_context(&sources, &vars, &config).unwrap();

        assert!(result.files.is_empty());
        assert_eq!(result.skipped.len(), 1);
        assert_eq!(result.skipped[0], "/nonexistent/file.md");
    }

    #[test]
    fn test_resolve_context_required_missing() {
        let sources = vec![ContextSource::File {
            path: "/nonexistent/file.md".to_string(),
            required: true,
        }];

        let vars = PathVariables::current();
        let config = ContextConfig::default();

        let result = resolve_context(&sources, &vars, &config);

        assert!(matches!(result, Err(ContextError::FileNotFound(_))));
    }

    #[test]
    fn test_resolve_context_files_all_exist() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("a.md"), "File A").unwrap();
        fs::write(temp.path().join("b.md"), "File B").unwrap();

        let sources = vec![ContextSource::Files {
            paths: vec![
                temp.path().join("a.md").to_str().unwrap().to_string(),
                temp.path().join("b.md").to_str().unwrap().to_string(),
            ],
            required: true,
        }];

        let vars = PathVariables::current();
        let config = ContextConfig::default();

        let result = resolve_context(&sources, &vars, &config).unwrap();

        assert_eq!(result.files.len(), 2);
        assert_eq!(result.files[0].content, "File A");
        assert_eq!(result.files[1].content, "File B");
    }

    #[test]
    fn test_resolve_context_files_required_one_missing() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("a.md"), "File A").unwrap();

        let sources = vec![ContextSource::Files {
            paths: vec![
                temp.path().join("a.md").to_str().unwrap().to_string(),
                temp.path().join("missing.md").to_str().unwrap().to_string(),
            ],
            required: true,
        }];

        let vars = PathVariables::current();
        let config = ContextConfig::default();

        let result = resolve_context(&sources, &vars, &config);

        assert!(matches!(result, Err(ContextError::FileNotFound(_))));
    }

    #[test]
    fn test_resolve_context_files_optional_one_missing() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("a.md"), "File A").unwrap();

        let sources = vec![ContextSource::Files {
            paths: vec![
                temp.path().join("a.md").to_str().unwrap().to_string(),
                temp.path().join("missing.md").to_str().unwrap().to_string(),
            ],
            required: false,
        }];

        let vars = PathVariables::current();
        let config = ContextConfig::default();

        let result = resolve_context(&sources, &vars, &config).unwrap();

        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].content, "File A");
        assert_eq!(result.skipped.len(), 1);
    }

    #[test]
    fn test_resolve_context_glob() {
        let temp = TempDir::new().unwrap();

        // Create multiple markdown files
        fs::write(temp.path().join("a.md"), "File A").unwrap();
        fs::write(temp.path().join("b.md"), "File B").unwrap();
        fs::write(temp.path().join("c.txt"), "Not markdown").unwrap();

        let pattern = format!("{}/*.md", temp.path().display());
        let sources = vec![ContextSource::Glob { pattern }];

        let vars = PathVariables::current();
        let config = ContextConfig::default();

        let result = resolve_context(&sources, &vars, &config).unwrap();

        assert_eq!(result.files.len(), 2);
        // Should be sorted alphabetically
        assert!(result.files[0]
            .resolved_path
            .as_ref()
            .unwrap()
            .ends_with("a.md"));
        assert!(result.files[1]
            .resolved_path
            .as_ref()
            .unwrap()
            .ends_with("b.md"));
    }

    #[test]
    fn test_resolve_context_glob_no_matches() {
        let temp = TempDir::new().unwrap();

        let pattern = format!("{}/*.md", temp.path().display());
        let sources = vec![ContextSource::Glob { pattern }];

        let vars = PathVariables::current();
        let config = ContextConfig::default();

        // Glob with 0 matches should succeed (inherently optional)
        let result = resolve_context(&sources, &vars, &config).unwrap();
        assert!(result.files.is_empty());
    }

    #[test]
    fn test_resolve_context_file_too_large() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("large.md");

        // Create a file larger than limit
        let content = "x".repeat(1000);
        fs::write(&file_path, &content).unwrap();

        let sources = vec![ContextSource::File {
            path: file_path.to_str().unwrap().to_string(),
            required: true,
        }];

        let vars = PathVariables::current();
        let config = ContextConfig {
            max_file_size: 100, // Very small limit
            max_total_size: DEFAULT_MAX_TOTAL_SIZE,
        };

        let result = resolve_context(&sources, &vars, &config);

        assert!(matches!(result, Err(ContextError::FileTooLarge { .. })));
    }

    #[test]
    fn test_resolve_context_total_too_large() {
        let temp = TempDir::new().unwrap();

        // Create two files that together exceed total limit
        fs::write(temp.path().join("a.md"), "x".repeat(60)).unwrap();
        fs::write(temp.path().join("b.md"), "x".repeat(60)).unwrap();

        let pattern = format!("{}/*.md", temp.path().display());
        let sources = vec![ContextSource::Glob { pattern }];

        let vars = PathVariables::current();
        let config = ContextConfig {
            max_file_size: 100,
            max_total_size: 100, // Can fit one file but not two
        };

        let result = resolve_context(&sources, &vars, &config);

        assert!(matches!(
            result,
            Err(ContextError::TotalSizeTooLarge { .. })
        ));
    }

    #[test]
    fn test_resolve_context_declaration_order() {
        let temp = TempDir::new().unwrap();

        fs::write(temp.path().join("first.md"), "First").unwrap();
        fs::write(temp.path().join("second.md"), "Second").unwrap();

        let sources = vec![
            ContextSource::File {
                path: temp.path().join("second.md").to_str().unwrap().to_string(),
                required: true,
            },
            ContextSource::File {
                path: temp.path().join("first.md").to_str().unwrap().to_string(),
                required: true,
            },
        ];

        let vars = PathVariables::current();
        let config = ContextConfig::default();

        let result = resolve_context(&sources, &vars, &config).unwrap();

        // Declaration order: second.md first, then first.md
        assert_eq!(result.files.len(), 2);
        assert_eq!(result.files[0].content, "Second");
        assert_eq!(result.files[1].content, "First");
    }

    #[test]
    fn test_build_effective_prompt_system_only() {
        let context = ContextLoadResult::default();
        let result = build_effective_prompt(Some("You are helpful."), &context);

        assert_eq!(result, Some("You are helpful.".to_string()));
    }

    #[test]
    fn test_build_effective_prompt_context_only() {
        let context = ContextLoadResult {
            files: vec![ResolvedContext {
                source: "test.md".to_string(),
                resolved_path: Some(PathBuf::from("/path/to/test.md")),
                content: "Context content".to_string(),
            }],
            skipped: vec![],
            total_bytes: 15,
        };

        let result = build_effective_prompt(None, &context);

        assert!(result.is_some());
        let prompt = result.unwrap();
        assert!(prompt.contains("Context content"));
        assert!(prompt.contains("/path/to/test.md"));
    }

    #[test]
    fn test_build_effective_prompt_inline_content() {
        let context = ContextLoadResult {
            files: vec![ResolvedContext {
                source: "inline content".to_string(),
                resolved_path: None,
                content: "Inline rules".to_string(),
            }],
            skipped: vec![],
            total_bytes: 12,
        };

        let result = build_effective_prompt(None, &context);

        assert!(result.is_some());
        let prompt = result.unwrap();
        assert!(prompt.contains("Inline rules"));
        assert!(prompt.contains("Inline context"));
    }

    #[test]
    fn test_build_effective_prompt_combined() {
        let context = ContextLoadResult {
            files: vec![ResolvedContext {
                source: "test.md".to_string(),
                resolved_path: Some(PathBuf::from("/path/to/test.md")),
                content: "Context content".to_string(),
            }],
            skipped: vec![],
            total_bytes: 15,
        };

        let result = build_effective_prompt(Some("System prompt"), &context);

        assert!(result.is_some());
        let prompt = result.unwrap();
        // System prompt should come first
        assert!(prompt.starts_with("System prompt"));
        // Then context
        assert!(prompt.contains("Context content"));
    }

    #[test]
    fn test_build_effective_prompt_empty() {
        let context = ContextLoadResult::default();
        let result = build_effective_prompt(None, &context);

        assert!(result.is_none());
    }

    #[test]
    fn test_context_config_default() {
        let config = ContextConfig::default();

        assert_eq!(config.max_file_size, DEFAULT_MAX_FILE_SIZE);
        assert_eq!(config.max_total_size, DEFAULT_MAX_TOTAL_SIZE);
    }
}
