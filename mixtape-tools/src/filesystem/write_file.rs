use crate::filesystem::validate_path;
use crate::prelude::*;
use std::path::PathBuf;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

/// Write mode for file operations
#[derive(Debug, Deserialize, Serialize, JsonSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum WriteMode {
    /// Overwrite the file if it exists, create if it doesn't
    #[default]
    Rewrite,
    /// Append to the end of the file if it exists, create if it doesn't
    Append,
}

/// Input for writing a file
#[derive(Debug, Deserialize, JsonSchema)]
pub struct WriteFileInput {
    /// Path to the file to write (relative to base path or absolute)
    pub path: PathBuf,

    /// Content to write to the file
    pub content: String,

    /// Write mode: 'rewrite' (default) or 'append'
    #[serde(default)]
    pub mode: WriteMode,
}

/// Tool for writing content to files
pub struct WriteFileTool {
    base_path: PathBuf,
}

impl Default for WriteFileTool {
    fn default() -> Self {
        Self::new()
    }
}

impl WriteFileTool {
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

impl Tool for WriteFileTool {
    type Input = WriteFileInput;

    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to a file. Can either overwrite the file or append to it."
    }

    async fn execute(&self, input: Self::Input) -> std::result::Result<ToolResult, ToolError> {
        // Validate path is within base directory
        let validated_path = validate_path(&self.base_path, &input.path)?;

        // Create parent directories if they don't exist
        if let Some(parent) = validated_path.parent() {
            if !parent.exists() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    ToolError::from(format!("Failed to create parent directories: {}", e))
                })?;
            }
        }

        let mut file = match input.mode {
            WriteMode::Rewrite => OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&validated_path)
                .await
                .map_err(|e| ToolError::from(format!("Failed to open file for writing: {}", e)))?,

            WriteMode::Append => OpenOptions::new()
                .write(true)
                .create(true)
                .append(true)
                .open(&validated_path)
                .await
                .map_err(|e| {
                    ToolError::from(format!("Failed to open file for appending: {}", e))
                })?,
        };

        file.write_all(input.content.as_bytes())
            .await
            .map_err(|e| ToolError::from(format!("Failed to write to file: {}", e)))?;

        file.flush()
            .await
            .map_err(|e| ToolError::from(format!("Failed to flush file: {}", e)))?;

        let bytes_written = input.content.len();
        let lines_written = input.content.lines().count();

        Ok(format!(
            "Successfully wrote {} bytes ({} lines) to {}",
            bytes_written,
            lines_written,
            input.path.display()
        )
        .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs;

    #[test]
    fn test_tool_metadata() {
        let tool: WriteFileTool = Default::default();
        assert_eq!(tool.name(), "write_file");
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_try_new() {
        let tool = WriteFileTool::try_new();
        assert!(tool.is_ok());
    }

    #[test]
    fn test_format_methods() {
        let tool = WriteFileTool::new();
        let params = serde_json::json!({"path": "test.txt", "content": "hello"});

        assert!(!tool.format_input_plain(&params).is_empty());
        assert!(!tool.format_input_ansi(&params).is_empty());
        assert!(!tool.format_input_markdown(&params).is_empty());

        let result = ToolResult::from("Successfully wrote");
        assert!(!tool.format_output_plain(&result).is_empty());
        assert!(!tool.format_output_ansi(&result).is_empty());
        assert!(!tool.format_output_markdown(&result).is_empty());
    }

    #[tokio::test]
    async fn test_write_file_create() {
        let temp_dir = TempDir::new().unwrap();
        let tool = WriteFileTool::with_base_path(temp_dir.path().to_path_buf());

        let input = WriteFileInput {
            path: PathBuf::from("test.txt"),
            content: "Hello, World!".to_string(),
            mode: WriteMode::Rewrite,
        };

        let result = tool.execute(input).await.unwrap();
        assert!(result.as_text().contains("13 bytes"));

        let content = fs::read_to_string(temp_dir.path().join("test.txt"))
            .await
            .unwrap();
        assert_eq!(content, "Hello, World!");
    }

    #[tokio::test]
    async fn test_write_file_overwrite() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, "Old content").await.unwrap();

        let tool = WriteFileTool::with_base_path(temp_dir.path().to_path_buf());
        let input = WriteFileInput {
            path: PathBuf::from("test.txt"),
            content: "New content".to_string(),
            mode: WriteMode::Rewrite,
        };

        tool.execute(input).await.unwrap();

        let content = fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, "New content");
    }

    #[tokio::test]
    async fn test_write_file_append() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, "Line 1\n").await.unwrap();

        let tool = WriteFileTool::with_base_path(temp_dir.path().to_path_buf());
        let input = WriteFileInput {
            path: PathBuf::from("test.txt"),
            content: "Line 2\n".to_string(),
            mode: WriteMode::Append,
        };

        tool.execute(input).await.unwrap();

        let content = fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, "Line 1\nLine 2\n");
    }

    // ===== Edge Case Tests =====

    #[tokio::test]
    async fn test_write_file_utf8_characters() {
        let temp_dir = TempDir::new().unwrap();
        let tool = WriteFileTool::with_base_path(temp_dir.path().to_path_buf());

        let utf8_content = "Hello 世界! Ümläüts: äöü 🎵";
        let input = WriteFileInput {
            path: PathBuf::from("utf8.txt"),
            content: utf8_content.to_string(),
            mode: WriteMode::Rewrite,
        };

        tool.execute(input).await.unwrap();

        let content = fs::read_to_string(temp_dir.path().join("utf8.txt"))
            .await
            .unwrap();
        assert_eq!(content, utf8_content);
    }

    #[tokio::test]
    async fn test_write_file_empty_content() {
        let temp_dir = TempDir::new().unwrap();
        let tool = WriteFileTool::with_base_path(temp_dir.path().to_path_buf());

        let input = WriteFileInput {
            path: PathBuf::from("empty.txt"),
            content: String::new(),
            mode: WriteMode::Rewrite,
        };

        let result = tool.execute(input).await.unwrap();
        assert!(result.as_text().contains("0 bytes"));

        let content = fs::read_to_string(temp_dir.path().join("empty.txt"))
            .await
            .unwrap();
        assert_eq!(content, "");
    }

    #[tokio::test]
    async fn test_write_file_preserves_crlf() {
        let temp_dir = TempDir::new().unwrap();
        let tool = WriteFileTool::with_base_path(temp_dir.path().to_path_buf());

        let crlf_content = "Line 1\r\nLine 2\r\nLine 3\r\n";
        let input = WriteFileInput {
            path: PathBuf::from("crlf.txt"),
            content: crlf_content.to_string(),
            mode: WriteMode::Rewrite,
        };

        tool.execute(input).await.unwrap();

        // Read as bytes to verify exact line endings
        let bytes = fs::read(temp_dir.path().join("crlf.txt")).await.unwrap();
        let content = String::from_utf8(bytes).unwrap();
        assert_eq!(content, crlf_content);
        assert!(content.contains("\r\n"));
    }

    #[tokio::test]
    async fn test_write_file_mixed_line_endings() {
        let temp_dir = TempDir::new().unwrap();
        let tool = WriteFileTool::with_base_path(temp_dir.path().to_path_buf());

        let mixed_content = "Line 1\nLine 2\r\nLine 3\rLine 4";
        let input = WriteFileInput {
            path: PathBuf::from("mixed.txt"),
            content: mixed_content.to_string(),
            mode: WriteMode::Rewrite,
        };

        tool.execute(input).await.unwrap();

        let bytes = fs::read(temp_dir.path().join("mixed.txt")).await.unwrap();
        let content = String::from_utf8(bytes).unwrap();
        assert_eq!(content, mixed_content);
    }

    #[tokio::test]
    async fn test_write_file_large_content() {
        let temp_dir = TempDir::new().unwrap();
        let tool = WriteFileTool::with_base_path(temp_dir.path().to_path_buf());

        // Create 1000 lines of content
        let large_content = (0..1000)
            .map(|i| format!("Line {} with some content", i))
            .collect::<Vec<_>>()
            .join("\n");

        let input = WriteFileInput {
            path: PathBuf::from("large.txt"),
            content: large_content.clone(),
            mode: WriteMode::Rewrite,
        };

        tool.execute(input).await.unwrap();

        let content = fs::read_to_string(temp_dir.path().join("large.txt"))
            .await
            .unwrap();
        assert_eq!(content, large_content);
        assert_eq!(content.lines().count(), 1000);
    }

    // ===== Error Path Tests =====

    #[tokio::test]
    async fn test_write_file_rejects_path_traversal() {
        let temp_dir = TempDir::new().unwrap();
        let tool = WriteFileTool::with_base_path(temp_dir.path().to_path_buf());

        let input = WriteFileInput {
            path: PathBuf::from("../../../tmp/evil.txt"),
            content: "malicious".to_string(),
            mode: WriteMode::Rewrite,
        };

        let result = tool.execute(input).await;
        assert!(result.is_err(), "Should reject path traversal");
    }

    #[tokio::test]
    async fn test_write_file_creates_parent_directories() {
        let temp_dir = TempDir::new().unwrap();
        let tool = WriteFileTool::with_base_path(temp_dir.path().to_path_buf());

        // Parent directories don't exist yet
        let input = WriteFileInput {
            path: PathBuf::from("nonexistent/subdir/file.txt"),
            content: "content".to_string(),
            mode: WriteMode::Rewrite,
        };

        let result = tool.execute(input).await;
        assert!(
            result.is_ok(),
            "Should create parent directories automatically"
        );

        // Verify the file was created with correct content
        let file_path = temp_dir.path().join("nonexistent/subdir/file.txt");
        assert!(file_path.exists(), "File should exist");
        let content = fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, "content");
    }
}
