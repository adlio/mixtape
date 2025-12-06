use crate::filesystem::validate_path;
use crate::prelude::*;
use std::path::PathBuf;
use tokio::fs;

/// Input for moving/renaming a file
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MoveFileInput {
    /// Source path (file or directory to move)
    pub source: PathBuf,

    /// Destination path (where to move the file/directory)
    pub destination: PathBuf,
}

/// Tool for moving or renaming files and directories
pub struct MoveFileTool {
    base_path: PathBuf,
}

impl Default for MoveFileTool {
    fn default() -> Self {
        Self::new()
    }
}

impl MoveFileTool {
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

impl Tool for MoveFileTool {
    type Input = MoveFileInput;

    fn name(&self) -> &str {
        "move_file"
    }

    fn description(&self) -> &str {
        "Move or rename a file or directory to a new location."
    }

    async fn execute(&self, input: Self::Input) -> std::result::Result<ToolResult, ToolError> {
        // Validate both source and destination are within base directory
        let source_path = validate_path(&self.base_path, &input.source)?;
        let dest_path = validate_path(&self.base_path, &input.destination)?;

        // Create parent directories for destination if they don't exist
        if let Some(parent) = dest_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).await.map_err(|e| {
                    ToolError::from(format!("Failed to create parent directories: {}", e))
                })?;
            }
        }

        fs::rename(&source_path, &dest_path)
            .await
            .map_err(|e| ToolError::from(format!("Failed to move file: {}", e)))?;

        Ok(format!(
            "Successfully moved {} to {}",
            input.source.display(),
            input.destination.display()
        )
        .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_tool_metadata() {
        let tool: MoveFileTool = Default::default();
        assert_eq!(tool.name(), "move_file");
        assert!(!tool.description().is_empty());

        let tool2 = MoveFileTool::new();
        assert_eq!(tool2.name(), "move_file");
    }

    #[test]
    fn test_try_new() {
        let tool = MoveFileTool::try_new();
        assert!(tool.is_ok());
    }

    #[test]
    fn test_format_methods() {
        let tool = MoveFileTool::new();
        let params = serde_json::json!({"source": "a.txt", "destination": "b.txt"});

        assert!(!tool.format_input_plain(&params).is_empty());
        assert!(!tool.format_input_ansi(&params).is_empty());
        assert!(!tool.format_input_markdown(&params).is_empty());

        let result = ToolResult::from("Successfully moved");
        assert!(!tool.format_output_plain(&result).is_empty());
        assert!(!tool.format_output_ansi(&result).is_empty());
        assert!(!tool.format_output_markdown(&result).is_empty());
    }

    #[tokio::test]
    async fn test_move_file() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("source.txt");
        fs::write(&source, "content").unwrap();

        let tool = MoveFileTool::with_base_path(temp_dir.path().to_path_buf());
        let input = MoveFileInput {
            source: PathBuf::from("source.txt"),
            destination: PathBuf::from("dest.txt"),
        };

        let result = tool.execute(input).await.unwrap();
        assert!(result.as_text().contains("Successfully moved"));
        assert!(!source.exists());
        assert!(temp_dir.path().join("dest.txt").exists());
    }

    #[tokio::test]
    async fn test_rename_directory() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("old_dir");
        fs::create_dir(&source).unwrap();

        let tool = MoveFileTool::with_base_path(temp_dir.path().to_path_buf());
        let input = MoveFileInput {
            source: PathBuf::from("old_dir"),
            destination: PathBuf::from("new_dir"),
        };

        let result = tool.execute(input).await.unwrap();
        assert!(result.as_text().contains("Successfully moved"));
        assert!(!source.exists());
        assert!(temp_dir.path().join("new_dir").exists());
    }

    // ===== Error Path Tests =====

    #[tokio::test]
    async fn test_move_file_source_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let tool = MoveFileTool::with_base_path(temp_dir.path().to_path_buf());

        let input = MoveFileInput {
            source: PathBuf::from("nonexistent.txt"),
            destination: PathBuf::from("dest.txt"),
        };

        let result = tool.execute(input).await;
        assert!(result.is_err(), "Should fail when source doesn't exist");
    }

    #[tokio::test]
    async fn test_move_file_rejects_source_path_traversal() {
        let temp_dir = TempDir::new().unwrap();
        let tool = MoveFileTool::with_base_path(temp_dir.path().to_path_buf());

        let input = MoveFileInput {
            source: PathBuf::from("../../../etc/passwd"),
            destination: PathBuf::from("stolen.txt"),
        };

        let result = tool.execute(input).await;
        assert!(result.is_err(), "Should reject source path traversal");
    }

    #[tokio::test]
    async fn test_move_file_rejects_dest_path_traversal() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("source.txt"), "content").unwrap();

        let tool = MoveFileTool::with_base_path(temp_dir.path().to_path_buf());

        let input = MoveFileInput {
            source: PathBuf::from("source.txt"),
            destination: PathBuf::from("../../../tmp/escaped.txt"),
        };

        let result = tool.execute(input).await;
        assert!(result.is_err(), "Should reject destination path traversal");
    }

    #[tokio::test]
    async fn test_move_file_with_absolute_paths_inside_base() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("source.txt");
        let dest = temp_dir.path().join("dest.txt");
        fs::write(&source, "content").unwrap();

        let tool = MoveFileTool::with_base_path(temp_dir.path().to_path_buf());

        let input = MoveFileInput {
            source: source.clone(),
            destination: dest.clone(),
        };

        let result = tool.execute(input).await;
        assert!(result.is_ok(), "Should allow absolute paths within base");
        assert!(!source.exists());
        assert!(dest.exists());
    }

    #[tokio::test]
    async fn test_move_file_rejects_absolute_dest_outside_base() {
        // Security test: absolute destination path escaping base directory
        let temp_dir = TempDir::new().unwrap();
        let other_dir = TempDir::new().unwrap();

        let source = temp_dir.path().join("source.txt");
        fs::write(&source, "content").unwrap();

        let tool = MoveFileTool::with_base_path(temp_dir.path().to_path_buf());

        // Try to move to an absolute path outside base
        let input = MoveFileInput {
            source: PathBuf::from("source.txt"),
            destination: other_dir.path().join("stolen.txt"),
        };

        let result = tool.execute(input).await;
        assert!(
            result.is_err(),
            "Should reject absolute destination outside base"
        );
        assert!(
            result.unwrap_err().to_string().contains("escapes"),
            "Error should mention escaping base directory"
        );

        // Source should still exist (move was rejected)
        assert!(source.exists(), "Source should not be moved");
    }

    #[tokio::test]
    async fn test_move_file_to_existing_subdir() {
        // Test destination with existing parent that's within base
        let temp_dir = TempDir::new().unwrap();
        fs::create_dir(temp_dir.path().join("subdir")).unwrap();
        fs::write(temp_dir.path().join("source.txt"), "content").unwrap();

        let tool = MoveFileTool::with_base_path(temp_dir.path().to_path_buf());

        let input = MoveFileInput {
            source: PathBuf::from("source.txt"),
            destination: PathBuf::from("subdir/moved.txt"),
        };

        let result = tool.execute(input).await;
        assert!(result.is_ok(), "Should allow moving to existing subdir");
        assert!(temp_dir.path().join("subdir/moved.txt").exists());
    }

    #[tokio::test]
    async fn test_move_file_creates_parent_directories() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("source.txt"), "content").unwrap();

        let tool = MoveFileTool::with_base_path(temp_dir.path().to_path_buf());

        // Destination has non-existent parent directories
        let input = MoveFileInput {
            source: PathBuf::from("source.txt"),
            destination: PathBuf::from("nonexistent/subdir/moved.txt"),
        };

        let result = tool.execute(input).await;
        assert!(
            result.is_ok(),
            "Should create parent directories automatically"
        );

        // Verify the file was moved
        let dest_path = temp_dir.path().join("nonexistent/subdir/moved.txt");
        assert!(dest_path.exists(), "File should exist at destination");
        assert!(
            !temp_dir.path().join("source.txt").exists(),
            "Source should no longer exist"
        );

        // Verify content is preserved
        let content = std::fs::read_to_string(&dest_path).unwrap();
        assert_eq!(content, "content");
    }
}
