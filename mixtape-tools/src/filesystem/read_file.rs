use crate::filesystem::validate_path;
use crate::prelude::*;
use std::path::PathBuf;

/// Input for reading a file
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadFileInput {
    /// Path to the file to read (relative to base path or absolute)
    pub path: PathBuf,

    /// Starting line number (0-indexed, optional)
    #[serde(default)]
    pub offset: Option<usize>,

    /// Maximum number of lines to read (optional)
    #[serde(default)]
    pub length: Option<usize>,
}

/// Tool for reading file contents from the filesystem
pub struct ReadFileTool {
    base_path: PathBuf,
}

impl Default for ReadFileTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ReadFileTool {
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

impl Tool for ReadFileTool {
    type Input = ReadFileInput;

    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file from the filesystem. Supports reading entire files or specific line ranges."
    }

    fn format_output_plain(&self, result: &ToolResult) -> String {
        let content = result.as_text();
        if content.is_empty() {
            return "(empty file)".to_string();
        }

        let lines: Vec<&str> = content.lines().collect();
        let width = lines.len().to_string().len().max(3);

        let mut out = String::new();
        for (i, line) in lines.iter().enumerate() {
            out.push_str(&format!("{:>width$} │ {}\n", i + 1, line, width = width));
        }
        out
    }

    fn format_output_ansi(&self, result: &ToolResult) -> String {
        let content = result.as_text();
        if content.is_empty() {
            return "\x1b[2m(empty file)\x1b[0m".to_string();
        }

        let lines: Vec<&str> = content.lines().collect();
        let width = lines.len().to_string().len().max(3);

        let mut out = String::new();
        for (i, line) in lines.iter().enumerate() {
            out.push_str(&format!(
                "\x1b[36m{:>width$}\x1b[0m \x1b[2m│\x1b[0m {}\n",
                i + 1,
                line,
                width = width
            ));
        }
        out
    }

    fn format_output_markdown(&self, result: &ToolResult) -> String {
        let content = result.as_text();
        if content.is_empty() {
            return "*Empty file*".to_string();
        }
        format!("```\n{}\n```", content)
    }

    async fn execute(&self, input: Self::Input) -> std::result::Result<ToolResult, ToolError> {
        let path = validate_path(&self.base_path, &input.path)
            .map_err(|e| ToolError::from(e.to_string()))?;

        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| ToolError::from(format!("Failed to read file: {}", e)))?;

        let result = if input.offset.is_some() || input.length.is_some() {
            let lines: Vec<&str> = content.lines().collect();
            let offset = input.offset.unwrap_or(0);
            let length = input.length.unwrap_or(lines.len().saturating_sub(offset));

            let selected_lines: Vec<&str> =
                lines.iter().skip(offset).take(length).copied().collect();

            selected_lines.join("\n")
        } else {
            content
        };

        Ok(result.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_tool_metadata() {
        // Exercise Default, new(), name(), description()
        let tool: ReadFileTool = Default::default();
        assert_eq!(tool.name(), "read_file");
        assert!(!tool.description().is_empty());

        let tool2 = ReadFileTool::new();
        assert_eq!(tool2.name(), "read_file");
    }

    #[test]
    fn test_try_new() {
        let tool = ReadFileTool::try_new();
        assert!(tool.is_ok());
    }

    #[test]
    fn test_format_methods() {
        let tool = ReadFileTool::new();
        let params = serde_json::json!({"path": "test.txt"});

        assert!(!tool.format_input_plain(&params).is_empty());
        assert!(!tool.format_input_ansi(&params).is_empty());
        assert!(!tool.format_input_markdown(&params).is_empty());

        let result = ToolResult::from("file content");
        assert!(!tool.format_output_plain(&result).is_empty());
        assert!(!tool.format_output_ansi(&result).is_empty());
        assert!(!tool.format_output_markdown(&result).is_empty());
    }

    #[tokio::test]
    async fn test_read_file_full() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, "line1\nline2\nline3").unwrap();

        let tool = ReadFileTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ReadFileInput {
            path: PathBuf::from("test.txt"),
            offset: None,
            length: None,
        };

        let result = tool.execute(input).await.unwrap();
        assert_eq!(result.as_text(), "line1\nline2\nline3");
    }

    #[tokio::test]
    async fn test_read_file_with_offset() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, "line1\nline2\nline3\nline4").unwrap();

        let tool = ReadFileTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ReadFileInput {
            path: PathBuf::from("test.txt"),
            offset: Some(1),
            length: Some(2),
        };

        let result = tool.execute(input).await.unwrap();
        assert_eq!(result.as_text(), "line2\nline3");
    }

    #[tokio::test]
    async fn test_read_file_rejects_traversal() {
        let temp_dir = TempDir::new().unwrap();
        let tool = ReadFileTool::with_base_path(temp_dir.path().to_path_buf());

        let input = ReadFileInput {
            path: PathBuf::from("../../../etc/passwd"),
            offset: None,
            length: None,
        };

        let result = tool.execute(input).await;
        assert!(result.is_err());
        // The error should be about path traversal or canonicalization
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("canonicalize") || err.contains("escapes") || err.contains("Invalid path")
        );
    }

    // ===== Edge Case Tests =====

    #[tokio::test]
    async fn test_read_file_utf8_characters() {
        let temp_dir = TempDir::new().unwrap();
        let utf8_content = "Hello 世界! Ümläüts: äöü 🎵";
        fs::write(temp_dir.path().join("utf8.txt"), utf8_content).unwrap();

        let tool = ReadFileTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ReadFileInput {
            path: PathBuf::from("utf8.txt"),
            offset: None,
            length: None,
        };

        let result = tool.execute(input).await.unwrap();
        assert_eq!(result.as_text(), utf8_content);
    }

    #[tokio::test]
    async fn test_read_file_empty() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("empty.txt"), "").unwrap();

        let tool = ReadFileTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ReadFileInput {
            path: PathBuf::from("empty.txt"),
            offset: None,
            length: None,
        };

        let result = tool.execute(input).await.unwrap();
        assert_eq!(result.as_text(), "");
    }

    #[tokio::test]
    async fn test_read_file_preserves_line_endings() {
        let temp_dir = TempDir::new().unwrap();
        let crlf_content = "Line 1\r\nLine 2\r\nLine 3\r\n";
        std::fs::write(temp_dir.path().join("crlf.txt"), crlf_content).unwrap();

        let tool = ReadFileTool::with_base_path(temp_dir.path().to_path_buf());
        let input = ReadFileInput {
            path: PathBuf::from("crlf.txt"),
            offset: None,
            length: None,
        };

        let result = tool.execute(input).await.unwrap();
        let content = result.as_text();
        // Verify CRLF is preserved
        assert!(content.contains("\r\n"));
        assert_eq!(content, crlf_content);
    }

    #[tokio::test]
    async fn test_read_file_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let tool = ReadFileTool::with_base_path(temp_dir.path().to_path_buf());

        let input = ReadFileInput {
            path: PathBuf::from("nonexistent.txt"),
            offset: None,
            length: None,
        };

        let result = tool.execute(input).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Failed to read file") || err.contains("No such file"));
    }
}
