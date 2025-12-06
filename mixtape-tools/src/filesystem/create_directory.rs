use crate::filesystem::validate_path;
use crate::prelude::*;
use std::path::PathBuf;
use tokio::fs;

/// Input for creating a directory
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateDirectoryInput {
    /// Path to the directory to create (relative to base path)
    pub path: PathBuf,
}

/// Tool for creating directories
pub struct CreateDirectoryTool {
    base_path: PathBuf,
}

impl Default for CreateDirectoryTool {
    fn default() -> Self {
        Self::new()
    }
}

impl CreateDirectoryTool {
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

impl Tool for CreateDirectoryTool {
    type Input = CreateDirectoryInput;

    fn name(&self) -> &str {
        "create_directory"
    }

    fn description(&self) -> &str {
        "Create a new directory. Parent directories will be created automatically if they don't exist."
    }

    async fn execute(&self, input: Self::Input) -> std::result::Result<ToolResult, ToolError> {
        // Validate path is within base directory before creation
        let validated_path = validate_path(&self.base_path, &input.path)?;

        // Create the directory (and any missing parents)
        fs::create_dir_all(&validated_path)
            .await
            .map_err(|e| ToolError::from(format!("Failed to create directory: {}", e)))?;

        Ok(format!("Successfully created directory: {}", input.path.display()).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_tool_metadata() {
        let tool: CreateDirectoryTool = Default::default();
        assert_eq!(tool.name(), "create_directory");
        assert!(!tool.description().is_empty());

        let tool2 = CreateDirectoryTool::new();
        assert_eq!(tool2.name(), "create_directory");
    }

    #[test]
    fn test_try_new() {
        let tool = CreateDirectoryTool::try_new();
        assert!(tool.is_ok());
    }

    #[test]
    fn test_format_methods() {
        let tool = CreateDirectoryTool::new();
        let params = serde_json::json!({"path": "new_dir"});

        assert!(!tool.format_input_plain(&params).is_empty());
        assert!(!tool.format_input_ansi(&params).is_empty());
        assert!(!tool.format_input_markdown(&params).is_empty());

        let result = ToolResult::from("Created directory");
        assert!(!tool.format_output_plain(&result).is_empty());
        assert!(!tool.format_output_ansi(&result).is_empty());
        assert!(!tool.format_output_markdown(&result).is_empty());
    }

    #[tokio::test]
    async fn test_create_directory_rejects_absolute_path_without_side_effects() {
        // SECURITY TEST: Attempting to create a directory outside base_path
        // using an absolute path should fail WITHOUT creating the directory first.
        let temp_dir = TempDir::new().unwrap();
        let evil_target = TempDir::new().unwrap();
        let evil_dir = evil_target.path().join("should_not_exist");

        let tool = CreateDirectoryTool::with_base_path(temp_dir.path().to_path_buf());

        // Try to create a directory using an absolute path outside base_path
        let input = CreateDirectoryInput {
            path: evil_dir.clone(),
        };

        let result = tool.execute(input).await;

        // The operation should fail
        assert!(
            result.is_err(),
            "Absolute path outside base should be rejected"
        );

        // CRITICAL: The directory should NOT have been created
        assert!(
            !evil_dir.exists(),
            "Security bug: directory was created before validation! Path: {:?}",
            evil_dir
        );
    }

    #[tokio::test]
    async fn test_create_directory() {
        let temp_dir = TempDir::new().unwrap();
        let tool = CreateDirectoryTool::with_base_path(temp_dir.path().to_path_buf());

        let input = CreateDirectoryInput {
            path: PathBuf::from("test_dir"),
        };

        let result = tool.execute(input).await.unwrap();
        assert!(result.as_text().contains("Successfully created"));
        assert!(temp_dir.path().join("test_dir").exists());
    }

    #[tokio::test]
    async fn test_create_nested_directory() {
        let temp_dir = TempDir::new().unwrap();
        let tool = CreateDirectoryTool::with_base_path(temp_dir.path().to_path_buf());

        let input = CreateDirectoryInput {
            path: PathBuf::from("parent/child/grandchild"),
        };

        let result = tool.execute(input).await.unwrap();
        assert!(result.as_text().contains("Successfully created"));
        assert!(temp_dir.path().join("parent/child/grandchild").exists());
    }
}
