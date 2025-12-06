use crate::filesystem::validate_path;
use crate::prelude::*;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use tokio::fs;

/// Input for listing directory contents
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListDirectoryInput {
    /// Path to the directory to list (relative to base path)
    pub path: PathBuf,

    /// Maximum recursion depth (default: 2)
    #[serde(default = "default_depth")]
    pub depth: usize,

    /// Maximum lines in output. If omitted, returns all entries (up to internal hard limit).
    /// Use this to control output size for large directories.
    #[serde(default)]
    pub max_lines: Option<usize>,
}

fn default_depth() -> usize {
    2
}

/// Hard limit on output lines to prevent runaway memory usage
const HARD_MAX_LINES: usize = 10_000;

/// Entry info collected during scan
#[derive(Debug)]
struct EntryInfo {
    name: String,
    is_dir: bool,
    size: Option<u64>,
    children: Vec<EntryInfo>,
    child_count: usize, // Total count including nested
}

/// Tool for listing directory contents
pub struct ListDirectoryTool {
    base_path: PathBuf,
}

impl Default for ListDirectoryTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ListDirectoryTool {
    /// Creates a new tool using the current working directory as the base path.
    ///
    /// Equivalent to `Default::default()`.
    ///
    /// # Panics
    ///
    /// Panics if the current working directory cannot be determined.
    /// Use [`try_new`](Self::try_new) or [`with_base_path`](Self::with_base_path) instead.
    pub fn new() -> Self {
        Self {
            base_path: std::env::current_dir().expect("Failed to get current working directory"),
        }
    }

    /// Creates a new tool using the current working directory as the base path.
    ///
    /// Returns an error if the current working directory cannot be determined.
    pub fn try_new() -> std::io::Result<Self> {
        Ok(Self {
            base_path: std::env::current_dir()?,
        })
    }

    /// Creates a tool with a custom base directory.
    ///
    /// All file operations will be constrained to this directory.
    pub fn with_base_path(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    /// Scan directory and collect entry info (phase 1)
    fn scan_directory<'a>(
        &'a self,
        path: PathBuf,
        current_depth: usize,
        max_depth: usize,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<Vec<EntryInfo>, ToolError>> + Send + 'a>>
    {
        Box::pin(async move {
            let mut read_dir = fs::read_dir(&path)
                .await
                .map_err(|e| ToolError::from(format!("Failed to read directory: {}", e)))?;

            let mut dir_entries = Vec::new();
            while let Some(entry) = read_dir
                .next_entry()
                .await
                .map_err(|e| ToolError::from(format!("Failed to read directory entry: {}", e)))?
            {
                dir_entries.push(entry);
            }

            dir_entries.sort_by_key(|e| e.file_name());

            let mut entries = Vec::new();
            for entry in dir_entries {
                let file_name = entry.file_name();
                let file_name_str = file_name.to_string_lossy().to_string();

                if file_name_str == ".git" {
                    continue;
                }

                let metadata = entry
                    .metadata()
                    .await
                    .map_err(|e| ToolError::from(format!("Failed to read metadata: {}", e)))?;

                if metadata.is_dir() {
                    let (children, child_count) = if current_depth < max_depth {
                        let children = self
                            .scan_directory(entry.path(), current_depth + 1, max_depth)
                            .await?;
                        let count = children.iter().map(|c| 1 + c.child_count).sum();
                        (children, count)
                    } else {
                        // At max depth, count direct children
                        let mut count = 0;
                        if let Ok(mut rd) = fs::read_dir(entry.path()).await {
                            while let Ok(Some(_)) = rd.next_entry().await {
                                count += 1;
                            }
                        }
                        (vec![], count)
                    };

                    entries.push(EntryInfo {
                        name: file_name_str,
                        is_dir: true,
                        size: None,
                        children,
                        child_count,
                    });
                } else {
                    entries.push(EntryInfo {
                        name: file_name_str,
                        is_dir: false,
                        size: Some(metadata.len()),
                        children: vec![],
                        child_count: 0,
                    });
                }
            }

            Ok(entries)
        })
    }

    /// Format entries with fair budget allocation (phase 2)
    fn format_entries(entries: &[EntryInfo], prefix: &str, budget: usize) -> (Vec<String>, usize) {
        if budget == 0 || entries.is_empty() {
            return (vec![], 0);
        }

        let mut output = Vec::new();
        let mut used = 0;
        let remaining_budget = budget;

        // Calculate fair share per entry
        // Each entry needs at least 1 line, dirs with children need 2 (dir + "X more")
        let num_entries = entries.len();
        let budget_per_entry = (remaining_budget / num_entries).max(1);

        for (i, entry) in entries.iter().enumerate() {
            if used >= budget {
                let remaining = entries.len() - i;
                output.push(format!("{}[MORE] ... {} more entries", prefix, remaining));
                used += 1;
                break;
            }

            let entry_budget = if i == entries.len() - 1 {
                // Last entry gets remaining budget
                budget.saturating_sub(used)
            } else {
                budget_per_entry.min(budget.saturating_sub(used))
            };

            if entry.is_dir {
                output.push(format!("{}[DIR]  {}/", prefix, entry.name));
                used += 1;

                if entry_budget > 1 && !entry.children.is_empty() {
                    let child_prefix = format!("{}  ", prefix);
                    let child_budget = entry_budget - 1; // -1 for the dir line itself

                    let (child_output, child_used) =
                        Self::format_entries(&entry.children, &child_prefix, child_budget);
                    output.extend(child_output);
                    used += child_used;
                } else if !entry.children.is_empty() || entry.child_count > 0 {
                    // No budget for children, show count
                    let count = if entry.children.is_empty() {
                        entry.child_count
                    } else {
                        entry.children.len()
                    };
                    if count > 0 && used < budget {
                        output.push(format!("{}  [MORE] ... {} items", prefix, count));
                        used += 1;
                    }
                }
            } else {
                let size = entry.size.unwrap_or(0);
                let size_str = if size < 1024 {
                    format!("{} B", size)
                } else if size < 1024 * 1024 {
                    format!("{:.1} KB", size as f64 / 1024.0)
                } else {
                    format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
                };
                output.push(format!("{}[FILE] {} ({})", prefix, entry.name, size_str));
                used += 1;
            }
        }

        (output, used)
    }
}

impl Tool for ListDirectoryTool {
    type Input = ListDirectoryInput;

    fn name(&self) -> &str {
        "list_directory"
    }

    fn description(&self) -> &str {
        "List the contents of a directory recursively up to a specified depth. Shows files and subdirectories with sizes."
    }

    async fn execute(&self, input: Self::Input) -> std::result::Result<ToolResult, ToolError> {
        let path = validate_path(&self.base_path, &input.path)
            .map_err(|e| ToolError::from(e.to_string()))?;

        if !path.is_dir() {
            return Err(format!("{} is not a directory", input.path.display()).into());
        }

        // Validate max_lines doesn't exceed hard limit
        if let Some(max) = input.max_lines {
            if max > HARD_MAX_LINES {
                return Err(format!(
                    "max_lines ({}) exceeds maximum allowed value ({})",
                    max, HARD_MAX_LINES
                )
                .into());
            }
        }

        // Phase 1: Scan directory tree
        let entries = self.scan_directory(path, 0, input.depth).await?;

        // Phase 2: Format with fair budget allocation
        let budget = input.max_lines.unwrap_or(HARD_MAX_LINES);
        let (formatted, _used) = Self::format_entries(&entries, "", budget);

        let content = if formatted.is_empty() {
            format!("Directory {} is empty", input.path.display())
        } else {
            format!(
                "Contents of {} (depth={}):\n{}",
                input.path.display(),
                input.depth,
                formatted.join("\n")
            )
        };

        Ok(content.into())
    }

    fn format_output_plain(&self, result: &ToolResult) -> String {
        let output = result.as_text();
        let mut lines: Vec<&str> = output.lines().collect();
        if lines.is_empty() {
            return output.to_string();
        }

        let header = lines.remove(0);
        let mut out = String::new();
        out.push_str(header);
        out.push('\n');

        let entries: Vec<(usize, &str)> = lines
            .iter()
            .map(|line| {
                let depth = line.len() - line.trim_start().len();
                (depth / 2, line.trim())
            })
            .collect();

        for (i, (depth, content)) in entries.iter().enumerate() {
            let is_last_at_depth = entries
                .iter()
                .skip(i + 1)
                .find(|(d, _)| *d <= *depth)
                .map(|(d, _)| *d < *depth)
                .unwrap_or(true);

            let mut prefix = String::new();
            for d in 0..*depth {
                let has_more = entries.iter().skip(i + 1).any(|(dd, _)| *dd == d);
                prefix.push_str(if has_more { "│   " } else { "    " });
            }

            let connector = if is_last_at_depth {
                "└── "
            } else {
                "├── "
            };

            let formatted = if content.starts_with("[DIR]") {
                format!(
                    "{} {}",
                    connector,
                    content.trim_start_matches("[DIR]").trim()
                )
            } else if content.starts_with("[FILE]") {
                let rest = content.trim_start_matches("[FILE]").trim();
                if let Some(paren_idx) = rest.rfind(" (") {
                    format!(
                        "{} {} ({})",
                        connector,
                        &rest[..paren_idx],
                        &rest[paren_idx + 2..rest.len() - 1]
                    )
                } else {
                    format!("{} {}", connector, rest)
                }
            } else {
                format!("{} {}", connector, content)
            };

            out.push_str(&prefix);
            out.push_str(&formatted);
            out.push('\n');
        }
        out
    }

    fn format_output_ansi(&self, result: &ToolResult) -> String {
        let output = result.as_text();
        let mut lines: Vec<&str> = output.lines().collect();
        if lines.is_empty() {
            return output.to_string();
        }

        let header = lines.remove(0);
        let mut out = format!("\x1b[1m{}\x1b[0m\n", header);

        let entries: Vec<(usize, &str)> = lines
            .iter()
            .map(|line| {
                let depth = line.len() - line.trim_start().len();
                (depth / 2, line.trim())
            })
            .collect();

        for (i, (depth, content)) in entries.iter().enumerate() {
            let is_last_at_depth = entries
                .iter()
                .skip(i + 1)
                .find(|(d, _)| *d <= *depth)
                .map(|(d, _)| *d < *depth)
                .unwrap_or(true);

            let mut prefix = String::new();
            for d in 0..*depth {
                let has_more = entries.iter().skip(i + 1).any(|(dd, _)| *dd == d);
                prefix.push_str(if has_more {
                    "\x1b[2m│\x1b[0m   "
                } else {
                    "    "
                });
            }

            let connector = if is_last_at_depth {
                "\x1b[2m└──\x1b[0m "
            } else {
                "\x1b[2m├──\x1b[0m "
            };

            let formatted = if content.starts_with("[DIR]") {
                let name = content.trim_start_matches("[DIR]").trim();
                format!("{}\x1b[1;34m{}\x1b[0m", connector, name)
            } else if content.starts_with("[FILE]") {
                let rest = content.trim_start_matches("[FILE]").trim();
                if let Some(paren_idx) = rest.rfind(" (") {
                    let name = &rest[..paren_idx];
                    let size = &rest[paren_idx + 2..rest.len() - 1];
                    format!(
                        "{}{} \x1b[2m{}\x1b[0m",
                        connector,
                        colorize_filename(name),
                        size
                    )
                } else {
                    format!("{}{}", connector, colorize_filename(rest))
                }
            } else if content.starts_with("...") {
                format!("{}\x1b[2m{}\x1b[0m", connector, content)
            } else {
                format!("{}{}", connector, content)
            };

            out.push_str(&prefix);
            out.push_str(&formatted);
            out.push('\n');
        }
        out
    }

    fn format_output_markdown(&self, result: &ToolResult) -> String {
        format!("```\n{}\n```", self.format_output_plain(result))
    }
}

/// Colorize filename based on extension (eza-inspired)
fn colorize_filename(name: &str) -> String {
    let ext = name.rsplit('.').next().unwrap_or("");
    match ext.to_lowercase().as_str() {
        // Source code - green
        "rs" | "py" | "js" | "ts" | "go" | "c" | "cpp" | "h" | "java" | "rb" | "php" => {
            format!("\x1b[32m{}\x1b[0m", name)
        }
        // Config/data - yellow
        "json" | "yaml" | "yml" | "toml" | "xml" | "ini" | "cfg" | "conf" => {
            format!("\x1b[33m{}\x1b[0m", name)
        }
        // Docs - cyan
        "md" | "txt" | "rst" | "doc" | "pdf" => {
            format!("\x1b[36m{}\x1b[0m", name)
        }
        // Archives - red
        "zip" | "tar" | "gz" | "bz2" | "xz" | "rar" | "7z" => {
            format!("\x1b[31m{}\x1b[0m", name)
        }
        // Images - magenta
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "ico" | "webp" => {
            format!("\x1b[35m{}\x1b[0m", name)
        }
        // Executables - bold green
        "sh" | "bash" | "zsh" | "exe" | "bin" => {
            format!("\x1b[1;32m{}\x1b[0m", name)
        }
        // Lock files - dim
        _ if name.ends_with(".lock") || name.ends_with("-lock.json") => {
            format!("\x1b[2m{}\x1b[0m", name)
        }
        _ => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // =========================================================================
    // Smoke test - basic functionality
    // =========================================================================

    #[tokio::test]
    async fn test_list_directory_basic() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("file1.txt"), "content").unwrap();
        fs::write(temp_dir.path().join("file2.txt"), "content").unwrap();
        fs::create_dir(temp_dir.path().join("subdir")).unwrap();

        let tool = ListDirectoryTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ListDirectoryInput {
            path: PathBuf::from("."),
            depth: 1,
            max_lines: None,
        };

        let result = tool.execute(input).await.unwrap();
        let output = result.as_text();

        assert!(output.contains("file1.txt"));
        assert!(output.contains("file2.txt"));
        assert!(output.contains("subdir"));
    }

    // =========================================================================
    // Constructor and metadata tests
    // =========================================================================

    #[test]
    fn test_tool_metadata() {
        let tool: ListDirectoryTool = Default::default();
        assert_eq!(tool.name(), "list_directory");
        assert!(!tool.description().is_empty());

        let tool2 = ListDirectoryTool::new();
        assert_eq!(tool2.name(), "list_directory");
    }

    #[test]
    fn test_try_new() {
        let tool = ListDirectoryTool::try_new();
        assert!(tool.is_ok());
    }

    // =========================================================================
    // Core functionality tests
    // =========================================================================

    #[tokio::test]
    async fn test_empty_directory() {
        let temp_dir = TempDir::new().unwrap();

        let tool = ListDirectoryTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ListDirectoryInput {
            path: PathBuf::from("."),
            depth: 1,
            max_lines: None,
        };

        let result = tool.execute(input).await.unwrap();
        assert!(result.as_text().contains("empty"));
    }

    #[tokio::test]
    async fn test_skips_git_directory() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("file.txt"), "content").unwrap();
        fs::create_dir(temp_dir.path().join(".git")).unwrap();
        fs::write(temp_dir.path().join(".git/config"), "git config").unwrap();

        let tool = ListDirectoryTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ListDirectoryInput {
            path: PathBuf::from("."),
            depth: 2,
            max_lines: None,
        };

        let result = tool.execute(input).await.unwrap();
        let output = result.as_text();

        assert!(output.contains("file.txt"), "Should show regular files");
        assert!(!output.contains(".git"), "Should skip .git directory");
    }

    #[tokio::test]
    async fn test_respects_depth_limit() {
        let temp_dir = TempDir::new().unwrap();
        fs::create_dir_all(temp_dir.path().join("a/b/c/d")).unwrap();
        fs::write(temp_dir.path().join("a/b/c/d/deep.txt"), "deep").unwrap();

        let tool = ListDirectoryTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ListDirectoryInput {
            path: PathBuf::from("."),
            depth: 2,
            max_lines: None,
        };

        let result = tool.execute(input).await.unwrap();
        let output = result.as_text();

        assert!(output.contains("a/"), "Should show first level");
        assert!(output.contains("b/"), "Should show second level");
        assert!(
            !output.contains("deep.txt"),
            "Should not show files beyond depth limit"
        );
    }

    #[tokio::test]
    async fn test_sorts_entries_alphabetically() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("zebra.txt"), "").unwrap();
        fs::write(temp_dir.path().join("alpha.txt"), "").unwrap();
        fs::write(temp_dir.path().join("beta.txt"), "").unwrap();

        let tool = ListDirectoryTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ListDirectoryInput {
            path: PathBuf::from("."),
            depth: 1,
            max_lines: None,
        };

        let result = tool.execute(input).await.unwrap();
        let output = result.as_text();

        let alpha_pos = output.find("alpha.txt").unwrap();
        let beta_pos = output.find("beta.txt").unwrap();
        let zebra_pos = output.find("zebra.txt").unwrap();

        assert!(
            alpha_pos < beta_pos && beta_pos < zebra_pos,
            "Entries should be sorted alphabetically"
        );
    }

    // =========================================================================
    // Size formatting tests (consolidated)
    // =========================================================================

    #[tokio::test]
    async fn test_size_formatting() {
        let temp_dir = TempDir::new().unwrap();

        // Create files of different sizes
        fs::write(temp_dir.path().join("tiny.txt"), "hi").unwrap(); // 2 bytes
        fs::write(temp_dir.path().join("medium.txt"), "x".repeat(2048)).unwrap(); // 2 KB
        fs::write(
            temp_dir.path().join("large.txt"),
            "x".repeat(1024 * 1024 + 1),
        )
        .unwrap(); // 1+ MB

        let tool = ListDirectoryTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ListDirectoryInput {
            path: PathBuf::from("."),
            depth: 1,
            max_lines: None,
        };

        let result = tool.execute(input).await.unwrap();
        let output = result.as_text();

        assert!(output.contains("2 B"), "Should show bytes for tiny files");
        assert!(output.contains("KB"), "Should show KB for medium files");
        assert!(output.contains("MB"), "Should show MB for large files");
    }

    // =========================================================================
    // max_lines parameter tests
    // =========================================================================

    #[tokio::test]
    async fn test_max_lines_limits_output() {
        let temp_dir = TempDir::new().unwrap();
        for i in 0..50 {
            fs::write(temp_dir.path().join(format!("file{:03}.txt", i)), "x").unwrap();
        }

        let tool = ListDirectoryTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ListDirectoryInput {
            path: PathBuf::from("."),
            depth: 1,
            max_lines: Some(10),
        };

        let result = tool.execute(input).await.unwrap();
        let output = result.as_text();

        let file_count = output.matches("[FILE]").count();
        assert!(file_count <= 10, "Should respect max_lines limit");
        assert!(output.contains("[MORE]"), "Should indicate truncation");
    }

    #[tokio::test]
    async fn test_max_lines_none_returns_all() {
        let temp_dir = TempDir::new().unwrap();
        for i in 0..100 {
            fs::write(temp_dir.path().join(format!("file{:03}.txt", i)), "x").unwrap();
        }

        let tool = ListDirectoryTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ListDirectoryInput {
            path: PathBuf::from("."),
            depth: 1,
            max_lines: None, // No limit
        };

        let result = tool.execute(input).await.unwrap();
        let output = result.as_text();

        let file_count = output.matches("[FILE]").count();
        assert_eq!(
            file_count, 100,
            "Should show all files when max_lines is None"
        );
        assert!(!output.contains("[MORE]"), "Should not truncate");
    }

    #[tokio::test]
    async fn test_max_lines_boundary_cases() {
        let temp_dir = TempDir::new().unwrap();
        for i in 0..20 {
            fs::write(temp_dir.path().join(format!("file{:02}.txt", i)), "x").unwrap();
        }

        let tool = ListDirectoryTool::with_base_path(temp_dir.path().to_path_buf());

        // Exactly at limit - no truncation
        let input = ListDirectoryInput {
            path: PathBuf::from("."),
            depth: 1,
            max_lines: Some(20),
        };
        let result = tool.execute(input).await.unwrap();
        assert!(
            !result.as_text().contains("[MORE]"),
            "Should not truncate at exact boundary"
        );

        // One under limit - truncates
        let input = ListDirectoryInput {
            path: PathBuf::from("."),
            depth: 1,
            max_lines: Some(19),
        };
        let result = tool.execute(input).await.unwrap();
        assert!(
            result.as_text().contains("[MORE]"),
            "Should truncate when over limit"
        );
    }

    // =========================================================================
    // Fair budget allocation tests
    // =========================================================================

    #[tokio::test]
    async fn test_fair_allocation_across_directories() {
        let temp_dir = TempDir::new().unwrap();

        // Create 5 directories, each with many files
        for d in 0..5 {
            let dir_path = temp_dir.path().join(format!("dir{}", d));
            fs::create_dir(&dir_path).unwrap();
            for f in 0..50 {
                fs::write(dir_path.join(format!("file{:02}.txt", f)), "x").unwrap();
            }
        }

        let tool = ListDirectoryTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ListDirectoryInput {
            path: PathBuf::from("."),
            depth: 2,
            max_lines: Some(30),
        };

        let result = tool.execute(input).await.unwrap();
        let output = result.as_text();

        // All 5 directories should be visible (fair allocation)
        let dir_count = output.matches("[DIR]").count();
        assert_eq!(dir_count, 5, "All directories should be visible");

        // Each directory should show some files (not first-dir-takes-all)
        let file_count = output.matches("[FILE]").count();
        assert!(
            file_count >= 5,
            "Should show files from multiple directories"
        );
    }

    #[tokio::test]
    async fn test_asymmetric_directories() {
        let temp_dir = TempDir::new().unwrap();

        // Big directory with 100 files
        let big = temp_dir.path().join("dir_big");
        fs::create_dir(&big).unwrap();
        for f in 0..100 {
            fs::write(big.join(format!("f{:03}.txt", f)), "x").unwrap();
        }

        // Small directory with 2 files
        let small = temp_dir.path().join("dir_small");
        fs::create_dir(&small).unwrap();
        fs::write(small.join("a.txt"), "x").unwrap();
        fs::write(small.join("b.txt"), "x").unwrap();

        let tool = ListDirectoryTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ListDirectoryInput {
            path: PathBuf::from("."),
            depth: 2,
            max_lines: Some(20),
        };

        let result = tool.execute(input).await.unwrap();
        let output = result.as_text();

        // Both directories should be visible
        assert!(output.contains("dir_big/"));
        assert!(output.contains("dir_small/"));

        // Small dir should show all its files
        assert!(output.contains("a.txt"));
        assert!(output.contains("b.txt"));
    }

    // =========================================================================
    // Error handling tests
    // =========================================================================

    #[tokio::test]
    async fn test_rejects_path_traversal() {
        let temp_dir = TempDir::new().unwrap();
        let tool = ListDirectoryTool::with_base_path(temp_dir.path().to_path_buf());

        let input = ListDirectoryInput {
            path: PathBuf::from("../../../etc"),
            depth: 1,
            max_lines: None,
        };

        let result = tool.execute(input).await;
        assert!(result.is_err(), "Should reject path traversal");
    }

    #[tokio::test]
    async fn test_rejects_non_directory() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("file.txt"), "content").unwrap();

        let tool = ListDirectoryTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ListDirectoryInput {
            path: PathBuf::from("file.txt"),
            depth: 1,
            max_lines: None,
        };

        let result = tool.execute(input).await;
        assert!(result.is_err(), "Should reject non-directory path");
        assert!(
            result.unwrap_err().to_string().contains("not a directory"),
            "Error should mention 'not a directory'"
        );
    }

    #[tokio::test]
    async fn test_rejects_max_lines_exceeding_hard_limit() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("file.txt"), "x").unwrap();

        let tool = ListDirectoryTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ListDirectoryInput {
            path: PathBuf::from("."),
            depth: 1,
            max_lines: Some(50_000), // Exceeds HARD_MAX_LINES (10,000)
        };

        let result = tool.execute(input).await;
        assert!(result.is_err(), "Should reject max_lines > HARD_MAX_LINES");

        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("50000") && err_msg.contains("10000"),
            "Error should mention both requested and max values: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_zero_max_lines_returns_empty() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("file.txt"), "content").unwrap();

        let tool = ListDirectoryTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ListDirectoryInput {
            path: PathBuf::from("."),
            depth: 1,
            max_lines: Some(0),
        };

        let result = tool.execute(input).await.unwrap();
        let output = result.as_text();

        // With zero budget, format_entries returns empty vec, so we get "empty" message
        assert!(
            output.contains("empty"),
            "Zero max_lines should report directory as empty"
        );
    }
}
