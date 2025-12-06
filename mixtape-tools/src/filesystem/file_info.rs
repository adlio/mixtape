use crate::filesystem::validate_path;
use crate::prelude::*;
use std::path::PathBuf;
use tokio::fs;

/// Input for getting file information
#[derive(Debug, Deserialize, JsonSchema)]
pub struct FileInfoInput {
    /// Path to the file to get information about
    pub path: PathBuf,
}

/// Format a file size in human-readable form
fn format_size(size: u64) -> String {
    if size < 1024 {
        format!("{} bytes", size)
    } else if size < 1024 * 1024 {
        format!("{:.2} KB ({} bytes)", size as f64 / 1024.0, size)
    } else if size < 1024 * 1024 * 1024 {
        format!("{:.2} MB ({} bytes)", size as f64 / (1024.0 * 1024.0), size)
    } else {
        format!(
            "{:.2} GB ({} bytes)",
            size as f64 / (1024.0 * 1024.0 * 1024.0),
            size
        )
    }
}

/// Tool for retrieving file metadata
pub struct FileInfoTool {
    base_path: PathBuf,
}

impl Default for FileInfoTool {
    fn default() -> Self {
        Self::new()
    }
}

impl FileInfoTool {
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
}

impl Tool for FileInfoTool {
    type Input = FileInfoInput;

    fn name(&self) -> &str {
        "file_info"
    }

    fn description(&self) -> &str {
        "Get detailed information about a file including size, type, and modification time."
    }

    async fn execute(&self, input: Self::Input) -> std::result::Result<ToolResult, ToolError> {
        // Validate the path for security first (this catches path traversal attempts)
        let _validated_path = validate_path(&self.base_path, &input.path)
            .map_err(|e| ToolError::from(e.to_string()))?;

        // Build the full path before canonicalization to detect symlinks
        // We use the uncanonicalized path for symlink_metadata so we can detect symlinks
        let uncanonicalized_path = if input.path.is_absolute() {
            input.path.clone()
        } else {
            self.base_path.join(&input.path)
        };

        // Use symlink_metadata on the uncanonicalized path to detect symlinks
        let metadata = fs::symlink_metadata(&uncanonicalized_path)
            .await
            .map_err(|e| ToolError::from(format!("Failed to read file metadata: {}", e)))?;

        // Check symlink FIRST - a symlink to a directory would return true for both
        let file_type = if metadata.is_symlink() {
            "Symbolic Link"
        } else if metadata.is_dir() {
            "Directory"
        } else {
            "File"
        };

        let size_str = format_size(metadata.len());

        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| {
                use chrono::{DateTime, Utc};
                let datetime = DateTime::from_timestamp(duration.as_secs() as i64, 0)
                    .unwrap_or(DateTime::<Utc>::MIN_UTC);
                datetime.format("%Y-%m-%d %H:%M:%S UTC").to_string()
            })
            .unwrap_or_else(|| "Unknown".to_string());

        // Detect MIME type for regular files only (not symlinks or directories)
        let mime_type = if metadata.is_symlink() {
            "N/A".to_string()
        } else if metadata.is_file() {
            infer::get_from_path(&uncanonicalized_path)
                .ok()
                .flatten()
                .map(|kind| kind.mime_type().to_string())
                .or_else(|| {
                    mime_guess::from_path(&uncanonicalized_path)
                        .first()
                        .map(|m| m.to_string())
                })
                .unwrap_or_else(|| "application/octet-stream".to_string())
        } else {
            "N/A".to_string()
        };

        let readonly = metadata.permissions().readonly();

        // Read symlink target if this is a symlink
        let symlink_target = if metadata.is_symlink() {
            fs::read_link(&uncanonicalized_path)
                .await
                .ok()
                .map(|p| p.display().to_string())
        } else {
            None
        };

        let content = if let Some(target) = symlink_target {
            format!(
                "File Information: {}\n\
                Type: {}\n\
                Target: {}\n\
                Size: {}\n\
                MIME Type: {}\n\
                Modified: {}\n\
                Read-only: {}",
                input.path.display(),
                file_type,
                target,
                size_str,
                mime_type,
                modified,
                readonly
            )
        } else {
            format!(
                "File Information: {}\n\
                Type: {}\n\
                Size: {}\n\
                MIME Type: {}\n\
                Modified: {}\n\
                Read-only: {}",
                input.path.display(),
                file_type,
                size_str,
                mime_type,
                modified,
                readonly
            )
        };

        Ok(content.into())
    }

    fn format_output_plain(&self, result: &ToolResult) -> String {
        let output = result.as_text();
        let fields = parse_file_info(&output);
        if fields.is_empty() {
            return output.to_string();
        }

        let mut out = String::new();
        for (key, value) in &fields {
            let icon = match *key {
                "File Information" => "",
                "Type" => match *value {
                    "Directory" => "[D]",
                    "Symbolic Link" => "[L]",
                    _ => "[F]",
                },
                "Target" => "[→]",
                "Size" => "[#]",
                "MIME Type" => "[M]",
                "Modified" => "[T]",
                "Read-only" => "[R]",
                _ => "   ",
            };

            if *key == "File Information" {
                out.push_str(&format!("{}\n", value));
                out.push_str(&"─".repeat(value.len().min(40)));
                out.push('\n');
            } else {
                out.push_str(&format!("{} {:12} {}\n", icon, key, value));
            }
        }
        out
    }

    fn format_output_ansi(&self, result: &ToolResult) -> String {
        let output = result.as_text();
        let fields = parse_file_info(&output);
        if fields.is_empty() {
            return output.to_string();
        }

        let mut out = String::new();
        for (key, value) in &fields {
            if *key == "File Information" {
                out.push_str(&format!("\x1b[1;36m{}\x1b[0m\n", value));
                out.push_str(&format!(
                    "\x1b[2m{}\x1b[0m\n",
                    "─".repeat(value.len().min(40))
                ));
            } else {
                let (icon, color) = match *key {
                    "Type" => match *value {
                        "Directory" => ("\x1b[34m󰉋\x1b[0m", "\x1b[34m"),
                        "Symbolic Link" => ("\x1b[36m󰌷\x1b[0m", "\x1b[36m"),
                        _ => ("\x1b[32m󰈔\x1b[0m", "\x1b[0m"),
                    },
                    "Target" => ("\x1b[36m󰌹\x1b[0m", "\x1b[36m"),
                    "Size" => ("\x1b[33m󰋊\x1b[0m", "\x1b[33m"),
                    "MIME Type" => ("\x1b[35m󰈙\x1b[0m", "\x1b[35m"),
                    "Modified" => ("\x1b[36m󰃰\x1b[0m", "\x1b[2m"),
                    "Read-only" => {
                        if *value == "true" {
                            ("\x1b[31m󰌾\x1b[0m", "\x1b[31m")
                        } else {
                            ("\x1b[32m󰌿\x1b[0m", "\x1b[32m")
                        }
                    }
                    _ => ("  ", "\x1b[0m"),
                };
                out.push_str(&format!(
                    "{} \x1b[2m{:12}\x1b[0m {}{}\x1b[0m\n",
                    icon, key, color, value
                ));
            }
        }
        out
    }

    fn format_output_markdown(&self, result: &ToolResult) -> String {
        let output = result.as_text();
        let fields = parse_file_info(&output);
        if fields.is_empty() {
            return output.to_string();
        }

        let mut out = String::new();
        for (key, value) in &fields {
            if *key == "File Information" {
                out.push_str(&format!("### `{}`\n\n", value));
                out.push_str("| Property | Value |\n");
                out.push_str("|----------|-------|\n");
            } else {
                out.push_str(&format!("| {} | `{}` |\n", key, value));
            }
        }
        out
    }
}

/// Parse file info output into fields
fn parse_file_info(output: &str) -> Vec<(&str, &str)> {
    output
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(2, ": ").collect();
            if parts.len() == 2 {
                Some((parts[0], parts[1]))
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_tool_metadata() {
        let tool: FileInfoTool = Default::default();
        assert_eq!(tool.name(), "file_info");
        assert!(!tool.description().is_empty());

        let tool2 = FileInfoTool::new();
        assert_eq!(tool2.name(), "file_info");
    }

    #[test]
    fn test_try_new() {
        let tool = FileInfoTool::try_new();
        assert!(tool.is_ok());
    }

    #[test]
    fn test_format_methods() {
        let tool = FileInfoTool::new();
        let params = serde_json::json!({"path": "test.txt"});

        // All format methods should return non-empty strings
        assert!(!tool.format_input_plain(&params).is_empty());
        assert!(!tool.format_input_ansi(&params).is_empty());
        assert!(!tool.format_input_markdown(&params).is_empty());

        let result = ToolResult::from("Type: File\nSize: 100 bytes");
        assert!(!tool.format_output_plain(&result).is_empty());
        assert!(!tool.format_output_ansi(&result).is_empty());
        assert!(!tool.format_output_markdown(&result).is_empty());
    }

    // ===== Execution Tests =====

    #[tokio::test]
    async fn test_file_info() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, "Hello, World!").unwrap();

        let tool = FileInfoTool::with_base_path(temp_dir.path().to_path_buf());
        let input = FileInfoInput {
            path: PathBuf::from("test.txt"),
        };

        let result = tool.execute(input).await.unwrap();
        assert!(result.as_text().contains("Type: File"));
        assert!(result.as_text().contains("13 bytes"));
        assert!(result.as_text().contains("text/plain"));
    }

    // ===== format_size Tests =====

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(0), "0 bytes");
        assert_eq!(format_size(1), "1 bytes");
        assert_eq!(format_size(512), "512 bytes");
        assert_eq!(format_size(1023), "1023 bytes");
    }

    #[test]
    fn test_format_size_kilobytes() {
        assert_eq!(format_size(1024), "1.00 KB (1024 bytes)");
        assert_eq!(format_size(1536), "1.50 KB (1536 bytes)");
        assert_eq!(format_size(1024 * 1024 - 1), "1024.00 KB (1048575 bytes)");
    }

    #[test]
    fn test_format_size_megabytes() {
        assert_eq!(format_size(1024 * 1024), "1.00 MB (1048576 bytes)");
        assert_eq!(
            format_size(1024 * 1024 * 500),
            "500.00 MB (524288000 bytes)"
        );
        assert_eq!(
            format_size(1024 * 1024 * 1024 - 1),
            "1024.00 MB (1073741823 bytes)"
        );
    }

    #[test]
    fn test_format_size_gigabytes() {
        assert_eq!(
            format_size(1024 * 1024 * 1024),
            "1.00 GB (1073741824 bytes)"
        );
        assert_eq!(
            format_size(1024 * 1024 * 1024 * 5),
            "5.00 GB (5368709120 bytes)"
        );
    }

    #[test]
    fn test_format_size_boundaries() {
        // Test exact boundaries between units to ensure proper formatting
        let cases = [
            (1023, "1023 bytes"),
            (1024, "1.00 KB (1024 bytes)"),
            (1024 * 1024 - 1, "1024.00 KB (1048575 bytes)"),
            (1024 * 1024, "1.00 MB (1048576 bytes)"),
            (1024 * 1024 * 1024 - 1, "1024.00 MB (1073741823 bytes)"),
            (1024 * 1024 * 1024, "1.00 GB (1073741824 bytes)"),
        ];

        for (size, expected) in cases {
            assert_eq!(
                format_size(size),
                expected,
                "Size {} formatted incorrectly",
                size
            );
        }
    }

    // ===== Coverage Gap Tests =====

    #[tokio::test]
    async fn test_file_info_directory() {
        // Test that directory metadata is correctly identified and MIME type is N/A
        let temp_dir = TempDir::new().unwrap();
        let subdir = temp_dir.path().join("testdir");
        fs::create_dir(&subdir).unwrap();

        let tool = FileInfoTool::with_base_path(temp_dir.path().to_path_buf());
        let input = FileInfoInput {
            path: PathBuf::from("testdir"),
        };

        let result = tool.execute(input).await.unwrap();
        let text = result.as_text();

        assert!(text.contains("Type: Directory"));
        assert!(text.contains("MIME Type: N/A"));
        assert!(text.contains("testdir"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_file_info_symlink() {
        // Symlinks are correctly detected using fs::symlink_metadata()
        let temp_dir = TempDir::new().unwrap();
        let real_file = temp_dir.path().join("real.txt");
        let symlink = temp_dir.path().join("link.txt");

        fs::write(&real_file, "target content").unwrap();
        std::os::unix::fs::symlink(&real_file, &symlink).unwrap();

        let tool = FileInfoTool::with_base_path(temp_dir.path().to_path_buf());
        let input = FileInfoInput {
            path: PathBuf::from("link.txt"),
        };

        let result = tool.execute(input).await.unwrap();
        let text = result.as_text();

        // Symlink is correctly identified
        assert!(text.contains("Type: Symbolic Link"));

        // Target path is shown
        assert!(text.contains("Target:"));
        assert!(text.contains("real.txt"));

        // MIME type should be N/A for symlinks
        assert!(text.contains("MIME Type: N/A"));

        // Size is the symlink size, not target size (should NOT contain target content length)
        assert!(
            !text.contains("14 bytes"),
            "Should show symlink size, not target size"
        );
    }

    #[tokio::test]
    async fn test_file_info_nonexistent() {
        // Test error handling when file doesn't exist
        let temp_dir = TempDir::new().unwrap();

        let tool = FileInfoTool::with_base_path(temp_dir.path().to_path_buf());
        let input = FileInfoInput {
            path: PathBuf::from("does_not_exist.txt"),
        };

        let result = tool.execute(input).await;
        assert!(result.is_err());

        let err = result.unwrap_err().to_string();
        assert!(err.contains("Failed to read file metadata"));
    }

    #[tokio::test]
    async fn test_file_info_rejects_path_traversal() {
        // Test that directory traversal attempts are rejected
        let temp_dir = TempDir::new().unwrap();

        let tool = FileInfoTool::with_base_path(temp_dir.path().to_path_buf());
        let input = FileInfoInput {
            path: PathBuf::from("../../etc/passwd"),
        };

        let result = tool.execute(input).await;
        assert!(result.is_err());

        let err = result.unwrap_err().to_string();
        assert!(err.contains("escapes") || err.contains("Path"));
    }

    #[tokio::test]
    async fn test_file_info_mime_by_content() {
        // Test MIME type detection via content inspection (magic bytes)
        let temp_dir = TempDir::new().unwrap();

        // PNG magic bytes: 89 50 4E 47 0D 0A 1A 0A
        let png_bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        fs::write(temp_dir.path().join("image.png"), png_bytes).unwrap();

        let tool = FileInfoTool::with_base_path(temp_dir.path().to_path_buf());
        let input = FileInfoInput {
            path: PathBuf::from("image.png"),
        };

        let result = tool.execute(input).await.unwrap();
        assert!(result.as_text().contains("image/png"));
    }

    #[tokio::test]
    async fn test_file_info_mime_by_extension() {
        // Test MIME type detection via file extension when content doesn't match
        let temp_dir = TempDir::new().unwrap();

        // JavaScript file with no magic bytes
        fs::write(temp_dir.path().join("script.js"), "console.log('hi')").unwrap();

        let tool = FileInfoTool::with_base_path(temp_dir.path().to_path_buf());
        let input = FileInfoInput {
            path: PathBuf::from("script.js"),
        };

        let result = tool.execute(input).await.unwrap();
        let text = result.as_text();

        // Different MIME implementations may return different values
        assert!(
            text.contains("text/javascript") || text.contains("application/javascript"),
            "Unexpected MIME type in: {}",
            text
        );
    }

    #[tokio::test]
    async fn test_file_info_unknown_mime_type() {
        // Test fallback to application/octet-stream for unknown file types
        let temp_dir = TempDir::new().unwrap();

        // Random bytes with unknown extension
        fs::write(
            temp_dir.path().join("mystery.xyz999"),
            vec![0xFF, 0xAB, 0xCD, 0xEF],
        )
        .unwrap();

        let tool = FileInfoTool::with_base_path(temp_dir.path().to_path_buf());
        let input = FileInfoInput {
            path: PathBuf::from("mystery.xyz999"),
        };

        let result = tool.execute(input).await.unwrap();
        assert!(result.as_text().contains("application/octet-stream"));
    }

    #[tokio::test]
    async fn test_file_info_readonly() {
        // Test detection of read-only file permissions
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("readonly.txt");
        fs::write(&file_path, "content").unwrap();

        // Set read-only
        let mut perms = fs::metadata(&file_path).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&file_path, perms).unwrap();

        let tool = FileInfoTool::with_base_path(temp_dir.path().to_path_buf());
        let input = FileInfoInput {
            path: PathBuf::from("readonly.txt"),
        };

        let result = tool.execute(input).await.unwrap();
        assert!(result.as_text().contains("Read-only: true"));

        // Clean up: restore write permissions so temp dir can be deleted
        let mut perms = fs::metadata(&file_path).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)] // Test cleanup, temp file
        perms.set_readonly(false);
        fs::set_permissions(&file_path, perms).unwrap();
    }

    #[tokio::test]
    async fn test_file_info_writable_file() {
        // Test that writable files show Read-only: false
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("writable.txt");
        fs::write(&file_path, "content").unwrap();

        let tool = FileInfoTool::with_base_path(temp_dir.path().to_path_buf());
        let input = FileInfoInput {
            path: PathBuf::from("writable.txt"),
        };

        let result = tool.execute(input).await.unwrap();
        assert!(result.as_text().contains("Read-only: false"));
    }

    #[test]
    fn test_parse_file_info_structure() {
        // Test the parse_file_info helper function
        let output = "File Information: test.txt\nType: File\nSize: 100 bytes\nMIME Type: text/plain\nModified: 2024-01-01\nRead-only: false";
        let fields = parse_file_info(output);

        assert_eq!(fields.len(), 6);
        assert_eq!(fields[0], ("File Information", "test.txt"));
        assert_eq!(fields[1], ("Type", "File"));
        assert_eq!(fields[2], ("Size", "100 bytes"));
        assert_eq!(fields[3], ("MIME Type", "text/plain"));
        assert_eq!(fields[4], ("Modified", "2024-01-01"));
        assert_eq!(fields[5], ("Read-only", "false"));
    }

    #[test]
    fn test_parse_file_info_empty() {
        // Test parsing empty string
        let fields = parse_file_info("");
        assert_eq!(fields.len(), 0);
    }

    #[test]
    fn test_parse_file_info_malformed() {
        // Test parsing lines without colon separator
        let output = "NoColonHere\nAlso no colon";
        let fields = parse_file_info(output);
        assert_eq!(fields.len(), 0);
    }

    #[tokio::test]
    async fn test_format_output_ansi_directory_icon() {
        // Test that ANSI formatter uses appropriate colors for directories
        let temp_dir = TempDir::new().unwrap();
        fs::create_dir(temp_dir.path().join("mydir")).unwrap();

        let tool = FileInfoTool::with_base_path(temp_dir.path().to_path_buf());
        let input = FileInfoInput {
            path: PathBuf::from("mydir"),
        };

        let result = tool.execute(input).await.unwrap();
        let ansi = tool.format_output_ansi(&result);

        // Should use blue color for directory
        assert!(ansi.contains("\x1b[34m"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_format_output_ansi_symlink_icon() {
        // Symlinks are correctly detected and formatted with cyan color
        let temp_dir = TempDir::new().unwrap();
        let real_file = temp_dir.path().join("real.txt");
        let symlink = temp_dir.path().join("link.txt");

        fs::write(&real_file, "content").unwrap();
        std::os::unix::fs::symlink(&real_file, &symlink).unwrap();

        let tool = FileInfoTool::with_base_path(temp_dir.path().to_path_buf());
        let input = FileInfoInput {
            path: PathBuf::from("link.txt"),
        };

        let result = tool.execute(input).await.unwrap();
        let ansi = tool.format_output_ansi(&result);

        // Symlink is correctly formatted with cyan color
        assert!(
            ansi.contains("\x1b[36m"),
            "Symlinks should be formatted with cyan color"
        );

        // Should contain the symlink icon (󰌷)
        assert!(ansi.contains("󰌷"), "Should show symlink icon");
    }

    #[tokio::test]
    async fn test_format_output_ansi_readonly_colors() {
        // Test that read-only status affects ANSI color coding
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("readonly.txt");
        fs::write(&file_path, "content").unwrap();

        let mut perms = fs::metadata(&file_path).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&file_path, perms).unwrap();

        let tool = FileInfoTool::with_base_path(temp_dir.path().to_path_buf());
        let input = FileInfoInput {
            path: PathBuf::from("readonly.txt"),
        };

        let result = tool.execute(input).await.unwrap();
        let ansi = tool.format_output_ansi(&result);

        // Should have red color for read-only true
        assert!(ansi.contains("\x1b[31m"));

        // Clean up: restore write permissions so temp dir can be deleted
        let mut perms = fs::metadata(&file_path).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)] // Test cleanup, temp file
        perms.set_readonly(false);
        fs::set_permissions(&file_path, perms).unwrap();
    }

    #[tokio::test]
    async fn test_format_output_markdown_structure() {
        // Test that markdown formatter creates proper table structure
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("test.txt"), "content").unwrap();

        let tool = FileInfoTool::with_base_path(temp_dir.path().to_path_buf());
        let input = FileInfoInput {
            path: PathBuf::from("test.txt"),
        };

        let result = tool.execute(input).await.unwrap();
        let markdown = tool.format_output_markdown(&result);

        // Should contain markdown table elements
        assert!(markdown.contains("###"));
        assert!(markdown.contains("| Property | Value |"));
        assert!(markdown.contains("|----------|-------|"));
        assert!(markdown.contains("| Type |"));
        assert!(markdown.contains("| Size |"));
    }

    #[tokio::test]
    async fn test_format_output_plain_structure() {
        // Test plain formatter output structure
        let temp_dir = TempDir::new().unwrap();
        fs::create_dir(temp_dir.path().join("testdir")).unwrap();

        let tool = FileInfoTool::with_base_path(temp_dir.path().to_path_buf());
        let input = FileInfoInput {
            path: PathBuf::from("testdir"),
        };

        let result = tool.execute(input).await.unwrap();
        let plain = tool.format_output_plain(&result);

        // Should have directory marker
        assert!(plain.contains("[D]"));
        // Should have separator line
        assert!(plain.contains("─"));
    }

    #[tokio::test]
    async fn test_empty_file() {
        // Test metadata for empty (0 byte) file
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("empty.txt"), "").unwrap();

        let tool = FileInfoTool::with_base_path(temp_dir.path().to_path_buf());
        let input = FileInfoInput {
            path: PathBuf::from("empty.txt"),
        };

        let result = tool.execute(input).await.unwrap();
        let text = result.as_text();

        assert!(text.contains("Type: File"));
        assert!(text.contains("0 bytes"));
    }

    #[tokio::test]
    async fn test_large_file_size_display() {
        // Test that large files display size in appropriate units
        let temp_dir = TempDir::new().unwrap();

        // Create a 2MB file
        let large_content = vec![0u8; 2 * 1024 * 1024];
        fs::write(temp_dir.path().join("large.bin"), large_content).unwrap();

        let tool = FileInfoTool::with_base_path(temp_dir.path().to_path_buf());
        let input = FileInfoInput {
            path: PathBuf::from("large.bin"),
        };

        let result = tool.execute(input).await.unwrap();
        let text = result.as_text();

        // Should display in MB
        assert!(text.contains("2.00 MB"));
        assert!(text.contains("2097152 bytes"));
    }

    #[tokio::test]
    async fn test_binary_file_mime_detection() {
        // Test MIME detection for common binary file types
        let temp_dir = TempDir::new().unwrap();

        // JPEG magic bytes: FF D8 FF
        let jpeg_bytes = vec![0xFF, 0xD8, 0xFF, 0xE0];
        fs::write(temp_dir.path().join("photo.jpg"), jpeg_bytes).unwrap();

        let tool = FileInfoTool::with_base_path(temp_dir.path().to_path_buf());
        let input = FileInfoInput {
            path: PathBuf::from("photo.jpg"),
        };

        let result = tool.execute(input).await.unwrap();
        assert!(result.as_text().contains("image/jpeg"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_file_info_permission_denied() {
        // Test error handling when file exists but can't read metadata due to permissions
        // This is tricky to test as metadata reading typically doesn't fail even without
        // read permissions, but we can try with a directory we create and lock down
        let temp_dir = TempDir::new().unwrap();
        let locked_dir = temp_dir.path().join("locked");
        fs::create_dir(&locked_dir).unwrap();
        let secret_file = locked_dir.join("secret.txt");
        fs::write(&secret_file, "secret").unwrap();

        // Remove all permissions from parent directory
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&locked_dir).unwrap().permissions();
        perms.set_mode(0o000);
        fs::set_permissions(&locked_dir, perms).unwrap();

        let tool = FileInfoTool::with_base_path(temp_dir.path().to_path_buf());
        let input = FileInfoInput {
            path: PathBuf::from("locked/secret.txt"),
        };

        let result = tool.execute(input).await;

        // Should fail (either during validation or metadata read)
        assert!(result.is_err());

        // Clean up: restore permissions
        let mut perms = fs::metadata(&locked_dir).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&locked_dir, perms).unwrap();
    }

    #[tokio::test]
    async fn test_file_with_no_extension() {
        // Test MIME type detection for files without extensions
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("Makefile"), "all:\n\techo hello").unwrap();

        let tool = FileInfoTool::with_base_path(temp_dir.path().to_path_buf());
        let input = FileInfoInput {
            path: PathBuf::from("Makefile"),
        };

        let result = tool.execute(input).await.unwrap();
        // Should succeed and show some MIME type
        assert!(result.as_text().contains("MIME Type:"));
    }
}
