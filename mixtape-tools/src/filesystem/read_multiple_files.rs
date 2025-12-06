use crate::filesystem::validate_path;
use crate::prelude::*;
use futures::stream::{self, StreamExt};
use std::path::PathBuf;

/// Result for a single file read operation
#[derive(Debug, Serialize, JsonSchema)]
pub struct FileReadResult {
    /// Path that was attempted
    pub path: String,
    /// Content of the file if successful
    pub content: Option<String>,
    /// Error message if failed
    pub error: Option<String>,
}

/// Input for reading multiple files
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadMultipleFilesInput {
    /// List of file paths to read (relative to base path)
    pub paths: Vec<PathBuf>,

    /// Maximum number of files to read concurrently (default: 10)
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
}

fn default_concurrency() -> usize {
    10
}

/// Tool for reading multiple files concurrently
pub struct ReadMultipleFilesTool {
    base_path: PathBuf,
}

impl Default for ReadMultipleFilesTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ReadMultipleFilesTool {
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

    async fn read_single_file(&self, path: PathBuf) -> FileReadResult {
        let path_str = path.display().to_string();

        match validate_path(&self.base_path, &path) {
            Ok(validated_path) => match tokio::fs::read_to_string(&validated_path).await {
                Ok(content) => FileReadResult {
                    path: path_str,
                    content: Some(content),
                    error: None,
                },
                Err(e) => FileReadResult {
                    path: path_str,
                    content: None,
                    error: Some(format!("Failed to read file: {}", e)),
                },
            },
            Err(e) => FileReadResult {
                path: path_str,
                content: None,
                error: Some(e.to_string()),
            },
        }
    }
}

impl Tool for ReadMultipleFilesTool {
    type Input = ReadMultipleFilesInput;

    fn name(&self) -> &str {
        "read_multiple_files"
    }

    fn description(&self) -> &str {
        "Read multiple files concurrently. Returns results for all files, including errors for files that couldn't be read."
    }

    fn format_output_plain(&self, result: &ToolResult) -> String {
        let output = result.as_text();
        let lines: Vec<&str> = output.lines().collect();
        if lines.is_empty() {
            return output.to_string();
        }

        let mut out = String::new();
        if let Some(header) = lines.first() {
            if header.starts_with("Read ") {
                out.push_str(&"─".repeat(50));
                out.push_str(&format!("\n  {}\n", header));
                out.push_str(&"─".repeat(50));
                out.push('\n');
            }
        }

        let mut in_file = false;
        for line in lines.iter().skip(1) {
            if let Some(path) = line.strip_prefix("✓ ") {
                if in_file {
                    out.push('\n');
                }
                out.push_str(&format!("[OK] {}\n", path));
                in_file = true;
            } else if let Some(path) = line.strip_prefix("✗ ") {
                if in_file {
                    out.push('\n');
                }
                out.push_str(&format!("[ERR] {}\n", path));
                in_file = true;
            } else if line.starts_with("Error:") {
                out.push_str(&format!("      {}\n", line));
            } else if !line.is_empty() {
                out.push_str(&format!("      │ {}\n", line));
            }
        }
        out
    }

    fn format_output_ansi(&self, result: &ToolResult) -> String {
        let output = result.as_text();
        let lines: Vec<&str> = output.lines().collect();
        if lines.is_empty() {
            return output.to_string();
        }

        let mut out = String::new();
        if let Some(header) = lines.first() {
            if header.starts_with("Read ") {
                let mut success = 0;
                let mut failed = 0;
                if let Some(paren_start) = header.find('(') {
                    let stats = &header[paren_start..];
                    if let Some(s) = stats.split_whitespace().next() {
                        success = s.trim_start_matches('(').parse().unwrap_or(0);
                    }
                    if let Some(f_idx) = stats.find("failed") {
                        if let Some(num) = stats[..f_idx].split_whitespace().last() {
                            failed = num.parse().unwrap_or(0);
                        }
                    }
                }

                out.push_str(&format!(
                    "\x1b[2m{}\x1b[0m\n  \x1b[1mFiles Read\x1b[0m  ",
                    "─".repeat(50)
                ));
                if success > 0 {
                    out.push_str(&format!("\x1b[32m● {} ok\x1b[0m  ", success));
                }
                if failed > 0 {
                    out.push_str(&format!("\x1b[31m● {} failed\x1b[0m", failed));
                }
                out.push_str(&format!("\n\x1b[2m{}\x1b[0m\n", "─".repeat(50)));
            }
        }

        let mut in_file = false;
        for line in lines.iter().skip(1) {
            if let Some(path) = line.strip_prefix("✓ ") {
                if in_file {
                    out.push('\n');
                }
                out.push_str(&format!("\x1b[32m●\x1b[0m \x1b[36m{}\x1b[0m\n", path));
                in_file = true;
            } else if let Some(path) = line.strip_prefix("✗ ") {
                if in_file {
                    out.push('\n');
                }
                out.push_str(&format!("\x1b[31m●\x1b[0m \x1b[36m{}\x1b[0m\n", path));
                in_file = true;
            } else if line.starts_with("Error:") {
                out.push_str(&format!("  \x1b[31m{}\x1b[0m\n", line));
            } else if !line.is_empty() {
                out.push_str(&format!("  \x1b[2m│\x1b[0m {}\n", line));
            }
        }
        out
    }

    fn format_output_markdown(&self, result: &ToolResult) -> String {
        let output = result.as_text();
        let lines: Vec<&str> = output.lines().collect();
        if lines.is_empty() {
            return output.to_string();
        }

        let mut out = String::new();
        if let Some(header) = lines.first() {
            if header.starts_with("Read ") {
                out.push_str(&format!("### {}\n\n", header));
            }
        }

        let mut current_file: Option<&str> = None;
        let mut content_lines: Vec<&str> = Vec::new();

        for line in lines.iter().skip(1) {
            let (is_file_line, is_success, path) = if let Some(p) = line.strip_prefix("✓ ") {
                (true, true, p)
            } else if let Some(p) = line.strip_prefix("✗ ") {
                (true, false, p)
            } else {
                (false, false, "")
            };

            if is_file_line {
                if current_file.is_some() {
                    if !content_lines.is_empty() {
                        out.push_str(&format!("```\n{}\n```\n\n", content_lines.join("\n")));
                        content_lines.clear();
                    } else {
                        out.push('\n');
                    }
                }
                out.push_str(&format!(
                    "{} `{}`\n",
                    if is_success { "✅" } else { "❌" },
                    path
                ));
                current_file = Some(path);
            } else if line.starts_with("Error:") {
                out.push_str(&format!("> ⚠️ {}\n", line));
            } else if !line.is_empty() {
                content_lines.push(line);
            }
        }

        if current_file.is_some() && !content_lines.is_empty() {
            out.push_str(&format!("```\n{}\n```\n", content_lines.join("\n")));
        }
        out
    }

    async fn execute(&self, input: Self::Input) -> std::result::Result<ToolResult, ToolError> {
        let concurrency = input.concurrency.min(50); // Cap at 50 to prevent resource exhaustion

        let results: Vec<FileReadResult> = stream::iter(input.paths)
            .map(|path| self.read_single_file(path))
            .buffer_unordered(concurrency)
            .collect()
            .await;

        let total = results.len();
        let successful = results.iter().filter(|r| r.content.is_some()).count();
        let failed = total - successful;

        let mut content = format!(
            "Read {} files ({} successful, {} failed):\n\n",
            total, successful, failed
        );

        for result in &results {
            match (&result.content, &result.error) {
                (Some(file_content), None) => {
                    let preview = if file_content.len() > 200 {
                        format!(
                            "{}... ({} bytes total)",
                            &file_content[..200],
                            file_content.len()
                        )
                    } else {
                        file_content.clone()
                    };
                    content.push_str(&format!("✓ {}\n{}\n\n", result.path, preview));
                }
                (None, Some(error)) => {
                    content.push_str(&format!("✗ {}\nError: {}\n\n", result.path, error));
                }
                _ => unreachable!(),
            }
        }

        Ok(content.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_format_methods() {
        let tool = ReadMultipleFilesTool::new();
        let params = serde_json::json!({"paths": ["file1.txt", "file2.txt"]});

        // All format methods should return non-empty strings
        assert!(!tool.format_input_plain(&params).is_empty());
        assert!(!tool.format_input_ansi(&params).is_empty());
        assert!(!tool.format_input_markdown(&params).is_empty());

        let result = ToolResult::from("Read 2 files");
        assert!(!tool.format_output_plain(&result).is_empty());
        assert!(!tool.format_output_ansi(&result).is_empty());
        assert!(!tool.format_output_markdown(&result).is_empty());
    }

    // ===== Execution Tests =====

    #[tokio::test]
    async fn test_read_multiple_files() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("file1.txt"), "content1").unwrap();
        fs::write(temp_dir.path().join("file2.txt"), "content2").unwrap();
        fs::write(temp_dir.path().join("file3.txt"), "content3").unwrap();

        let tool = ReadMultipleFilesTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ReadMultipleFilesInput {
            paths: vec![
                PathBuf::from("file1.txt"),
                PathBuf::from("file2.txt"),
                PathBuf::from("file3.txt"),
            ],
            concurrency: 10,
        };

        let result = tool.execute(input).await.unwrap();
        assert!(result.as_text().contains("3 successful, 0 failed"));
        assert!(result.as_text().contains("content1"));
        assert!(result.as_text().contains("content2"));
        assert!(result.as_text().contains("content3"));
    }

    #[test]
    fn test_tool_metadata() {
        let tool: ReadMultipleFilesTool = Default::default();
        assert_eq!(tool.name(), "read_multiple_files");
        assert!(!tool.description().is_empty());

        let tool2 = ReadMultipleFilesTool::new();
        assert_eq!(tool2.name(), "read_multiple_files");
    }

    #[test]
    fn test_try_new() {
        let tool = ReadMultipleFilesTool::try_new();
        assert!(tool.is_ok());
    }

    #[test]
    fn test_default_concurrency() {
        // Deserialize without specifying concurrency to trigger default_concurrency()
        let input: ReadMultipleFilesInput = serde_json::from_value(serde_json::json!({
            "paths": ["file.txt"]
        }))
        .unwrap();

        assert_eq!(input.concurrency, 10, "Default concurrency should be 10");
    }

    #[tokio::test]
    async fn test_read_multiple_files_with_errors() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("exists.txt"), "content").unwrap();

        let tool = ReadMultipleFilesTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ReadMultipleFilesInput {
            paths: vec![PathBuf::from("exists.txt"), PathBuf::from("missing.txt")],
            concurrency: 10,
        };

        let result = tool.execute(input).await.unwrap();
        assert!(result.as_text().contains("1 successful, 1 failed"));
        assert!(result.as_text().contains("content"));
        assert!(result.as_text().contains("✗ missing.txt"));
    }

    // ===== Coverage Gap Tests =====

    #[tokio::test]
    async fn test_concurrency_capped_at_50() {
        // Test that concurrency is capped at 50 to prevent resource exhaustion
        // even when a much larger value is requested
        let temp_dir = TempDir::new().unwrap();

        // Create 100 small files
        for i in 0..100 {
            fs::write(temp_dir.path().join(format!("file{}.txt", i)), "content").unwrap();
        }

        let paths: Vec<PathBuf> = (0..100)
            .map(|i| PathBuf::from(format!("file{}.txt", i)))
            .collect();

        let tool = ReadMultipleFilesTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ReadMultipleFilesInput {
            paths,
            concurrency: 10000, // Request absurdly high concurrency
        };

        // Should complete successfully without panicking or exhausting resources
        let result = tool.execute(input).await.unwrap();
        assert!(result.as_text().contains("100 successful, 0 failed"));
    }

    #[tokio::test]
    async fn test_large_file_content_truncation() {
        // Test that file content longer than 200 characters is truncated
        // in the output with a byte count indicator
        let temp_dir = TempDir::new().unwrap();
        let large_content = "x".repeat(500);
        fs::write(temp_dir.path().join("large.txt"), &large_content).unwrap();

        let tool = ReadMultipleFilesTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ReadMultipleFilesInput {
            paths: vec![PathBuf::from("large.txt")],
            concurrency: 10,
        };

        let result = tool.execute(input).await.unwrap();
        let text = result.as_text();

        // Should show truncation marker and total byte count
        assert!(text.contains("... (500 bytes total)"));

        // Verify content is actually truncated (contains first 200 chars but not beyond)
        assert!(text.contains(&"x".repeat(200)));
        assert!(!text.contains(&"x".repeat(300)));
    }

    #[tokio::test]
    async fn test_path_validation_errors_reported() {
        // Test that path validation failures (directory traversal attempts)
        // are properly caught and reported in the results
        let temp_dir = TempDir::new().unwrap();

        let tool = ReadMultipleFilesTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ReadMultipleFilesInput {
            paths: vec![
                PathBuf::from("../../etc/passwd"),
                PathBuf::from("../../../secret.txt"),
            ],
            concurrency: 10,
        };

        let result = tool.execute(input).await.unwrap();
        let text = result.as_text();

        // Both paths should fail validation
        assert!(text.contains("0 successful, 2 failed"));
        assert!(text.contains("✗ ../../etc/passwd"));
        assert!(text.contains("✗ ../../../secret.txt"));

        // Error messages should indicate path validation failure
        assert!(text.contains("escapes") || text.contains("Path"));
    }

    #[tokio::test]
    async fn test_empty_file_list() {
        // Test edge case of reading zero files - should not panic or produce
        // invalid output
        let temp_dir = TempDir::new().unwrap();

        let tool = ReadMultipleFilesTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ReadMultipleFilesInput {
            paths: vec![],
            concurrency: 10,
        };

        let result = tool.execute(input).await.unwrap();
        let text = result.as_text();

        assert!(text.contains("Read 0 files (0 successful, 0 failed)"));
    }

    #[tokio::test]
    async fn test_formatter_handles_mixed_results() {
        // Test that formatters correctly handle and display both successful
        // and failed file reads with appropriate visual indicators
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("exists.txt"), "content").unwrap();

        let tool = ReadMultipleFilesTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ReadMultipleFilesInput {
            paths: vec![PathBuf::from("exists.txt"), PathBuf::from("missing.txt")],
            concurrency: 10,
        };

        let result = tool.execute(input).await.unwrap();

        // Test ANSI formatter includes color codes and stats
        let ansi = tool.format_output_ansi(&result);
        assert!(ansi.contains("\x1b[32m")); // Green for success
        assert!(ansi.contains("\x1b[31m")); // Red for failure
        assert!(ansi.contains("1 ok"));
        assert!(ansi.contains("1 failed"));

        // Test Markdown formatter uses appropriate emoji and code blocks
        let markdown = tool.format_output_markdown(&result);
        assert!(markdown.contains("✅"));
        assert!(markdown.contains("❌"));
        assert!(markdown.contains("```"));

        // Test plain formatter
        let plain = tool.format_output_plain(&result);
        assert!(plain.contains("[OK]"));
        assert!(plain.contains("[ERR]"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_symlink_inside_base() {
        // Test that symlinks pointing to files within the base directory
        // are allowed and properly dereferenced
        let temp_dir = TempDir::new().unwrap();
        let real_file = temp_dir.path().join("real.txt");
        let symlink = temp_dir.path().join("link.txt");

        fs::write(&real_file, "real content").unwrap();
        std::os::unix::fs::symlink(&real_file, &symlink).unwrap();

        let tool = ReadMultipleFilesTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ReadMultipleFilesInput {
            paths: vec![PathBuf::from("link.txt")],
            concurrency: 10,
        };

        let result = tool.execute(input).await.unwrap();
        let text = result.as_text();

        assert!(text.contains("1 successful, 0 failed"));
        assert!(text.contains("real content"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_symlink_escaping_base_rejected() {
        // Test that symlinks pointing outside the base directory are rejected
        // for security reasons
        let temp_dir = TempDir::new().unwrap();
        let outside_dir = TempDir::new().unwrap();
        let outside_file = outside_dir.path().join("secret.txt");
        fs::write(&outside_file, "secret").unwrap();

        let symlink = temp_dir.path().join("escape_link.txt");
        std::os::unix::fs::symlink(&outside_file, &symlink).unwrap();

        let tool = ReadMultipleFilesTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ReadMultipleFilesInput {
            paths: vec![PathBuf::from("escape_link.txt")],
            concurrency: 10,
        };

        let result = tool.execute(input).await.unwrap();
        let text = result.as_text();

        // Should fail validation
        assert!(text.contains("0 successful, 1 failed"));
        assert!(text.contains("✗ escape_link.txt"));
        assert!(text.contains("escapes"));
    }

    #[tokio::test]
    async fn test_relative_path_with_dots() {
        // Test that paths with . and .. components are properly validated
        let temp_dir = TempDir::new().unwrap();
        fs::create_dir(temp_dir.path().join("subdir")).unwrap();
        fs::write(temp_dir.path().join("subdir/file.txt"), "content").unwrap();

        let tool = ReadMultipleFilesTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ReadMultipleFilesInput {
            paths: vec![PathBuf::from("./subdir/../subdir/./file.txt")],
            concurrency: 10,
        };

        let result = tool.execute(input).await.unwrap();
        assert!(result.as_text().contains("1 successful, 0 failed"));
        assert!(result.as_text().contains("content"));
    }

    #[tokio::test]
    async fn test_batch_read_with_permission_errors() {
        // Test handling of permission denied errors during batch reads
        // Note: This test is platform-specific and may behave differently on Windows
        #[cfg(unix)]
        {
            let temp_dir = TempDir::new().unwrap();
            let unreadable = temp_dir.path().join("unreadable.txt");
            fs::write(&unreadable, "secret").unwrap();

            // Remove read permissions
            let mut perms = fs::metadata(&unreadable).unwrap().permissions();
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(0o000);
            fs::set_permissions(&unreadable, perms).unwrap();

            let tool = ReadMultipleFilesTool::with_base_path(temp_dir.path().to_path_buf());
            let input = ReadMultipleFilesInput {
                paths: vec![PathBuf::from("unreadable.txt")],
                concurrency: 10,
            };

            let result = tool.execute(input).await.unwrap();
            let text = result.as_text();

            // Should fail to read with permission error
            assert!(text.contains("0 successful, 1 failed"));
            assert!(text.contains("✗ unreadable.txt"));
            assert!(text.contains("Failed to read file") || text.contains("Permission denied"));

            // Clean up: restore permissions so temp_dir can be deleted
            let mut perms = fs::metadata(&unreadable).unwrap().permissions();
            perms.set_mode(0o644);
            fs::set_permissions(&unreadable, perms).unwrap();
        }
    }

    #[tokio::test]
    async fn test_mixed_success_and_validation_errors() {
        // Test that both successful reads and validation errors can occur
        // in the same batch
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("good1.txt"), "content1").unwrap();
        fs::write(temp_dir.path().join("good2.txt"), "content2").unwrap();

        let tool = ReadMultipleFilesTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ReadMultipleFilesInput {
            paths: vec![
                PathBuf::from("good1.txt"),
                PathBuf::from("../../etc/passwd"),
                PathBuf::from("good2.txt"),
                PathBuf::from("missing.txt"),
            ],
            concurrency: 10,
        };

        let result = tool.execute(input).await.unwrap();
        let text = result.as_text();

        assert!(text.contains("2 successful, 2 failed"));
        assert!(text.contains("✓ good1.txt"));
        assert!(text.contains("✓ good2.txt"));
        assert!(text.contains("✗ ../../etc/passwd"));
        assert!(text.contains("✗ missing.txt"));
    }
}
