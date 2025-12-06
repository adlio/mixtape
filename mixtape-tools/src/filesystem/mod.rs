//! Filesystem tools with path traversal protection.
//!
//! All tools in this module operate within a configured `base_path` directory,
//! preventing access to files outside this boundary. This security model protects
//! against directory traversal attacks where malicious input like `../../../etc/passwd`
//! attempts to escape the intended directory.
//!
//! # Security Model
//!
//! Every file operation validates paths using [`validate_path`] before execution:
//!
//! - Paths are resolved relative to `base_path` (or used directly if absolute)
//! - The resolved path is canonicalized to eliminate `..`, `.`, and symlinks
//! - The canonical path must start with the canonical `base_path`
//! - For non-existent paths, the nearest existing ancestor is validated instead
//!
//! This means symlinks that point outside `base_path` are rejected, and crafted
//! paths like `subdir/../../../etc/passwd` are caught after canonicalization.
//!
//! # Defense in Depth
//!
//! Path validation provides **guardrails for AI agents**, not a complete security
//! boundary. Error messages intentionally include path details to help agents
//! understand and correct invalid requests.
//!
//! For production deployments with untrusted input, use defense in depth:
//!
//! - **Docker isolation**: Run tools in containers with only necessary directories mounted
//! - **OS-level permissions**: Use a dedicated user with minimal filesystem access
//! - **Network isolation**: Restrict container network access where possible
//!
//! These tools are one layer in a security stack, not a standalone sandbox.
//!
//! # Available Tools
//!
//! | Tool | Description |
//! |------|-------------|
//! | [`ReadFileTool`] | Read file contents with optional offset/limit |
//! | [`ReadMultipleFilesTool`] | Read multiple files concurrently |
//! | [`WriteFileTool`] | Write or append to files |
//! | [`CreateDirectoryTool`] | Create directories (including parents) |
//! | [`ListDirectoryTool`] | List directory contents recursively |
//! | [`MoveFileTool`] | Move or rename files and directories |
//! | [`FileInfoTool`] | Get file metadata (size, timestamps, type) |
//!
//! # Building Custom Tools
//!
//! Use [`validate_path`] when building your own filesystem tools:
//!
//! ```
//! use mixtape_tools::filesystem::validate_path;
//! use std::path::Path;
//!
//! let base = Path::new("/app/data");
//! let user_input = Path::new("../etc/passwd");
//!
//! // This will return an error because the path escapes base
//! assert!(validate_path(base, user_input).is_err());
//! ```

mod create_directory;
mod file_info;
mod list_directory;
mod move_file;
mod read_file;
mod read_multiple_files;
mod write_file;

pub use create_directory::CreateDirectoryTool;
pub use file_info::FileInfoTool;
pub use list_directory::ListDirectoryTool;
pub use move_file::MoveFileTool;
pub use read_file::ReadFileTool;
pub use read_multiple_files::ReadMultipleFilesTool;
pub use write_file::WriteFileTool;

use mixtape_core::ToolError;
use std::path::{Path, PathBuf};

/// Validates that a path is within the base directory, preventing directory traversal attacks.
///
/// This function is the security foundation for all filesystem tools. It ensures that
/// user-provided paths cannot escape the configured base directory, even when using
/// tricks like `..` components, absolute paths, or symlinks.
///
/// # Arguments
///
/// * `base_path` - The root directory that all paths must stay within
/// * `target_path` - The user-provided path to validate (relative or absolute)
///
/// # Returns
///
/// * `Ok(PathBuf)` - The validated path, canonicalized if the file exists
/// * `Err(ToolError::PathValidation)` - If the path escapes the base directory
///
/// # Security Properties
///
/// - **Symlink resolution**: Symlinks are resolved via canonicalization, so a symlink
///   pointing outside `base_path` will be rejected
/// - **Parent traversal**: Paths like `foo/../../../etc` are caught after canonicalization
/// - **Absolute paths**: Absolute paths outside `base_path` are rejected
/// - **Non-existent paths**: For paths that don't exist yet (e.g., for write operations),
///   the nearest existing ancestor is validated instead
///
/// # Example
///
/// ```
/// use mixtape_tools::filesystem::validate_path;
/// use std::path::Path;
///
/// let base = Path::new("/home/user/documents");
///
/// // Relative path within base - OK
/// let result = validate_path(base, Path::new("report.txt"));
/// // Returns Ok with resolved path
///
/// // Traversal attempt - REJECTED
/// let result = validate_path(base, Path::new("../../../etc/passwd"));
/// assert!(result.is_err());
///
/// // Absolute path outside base - REJECTED
/// let result = validate_path(base, Path::new("/etc/passwd"));
/// assert!(result.is_err());
/// ```
pub fn validate_path(base_path: &Path, target_path: &Path) -> Result<PathBuf, ToolError> {
    let full_path = if target_path.is_absolute() {
        target_path.to_path_buf()
    } else {
        base_path.join(target_path)
    };

    // Try to canonicalize if the file exists
    if full_path.exists() {
        let canonical = full_path.canonicalize().map_err(|e| {
            ToolError::PathValidation(format!(
                "Failed to canonicalize '{}': {}",
                full_path.display(),
                e
            ))
        })?;

        // Canonicalize base path for comparison
        let canonical_base = base_path.canonicalize().map_err(|e| {
            ToolError::PathValidation(format!(
                "Failed to canonicalize base path '{}': {}",
                base_path.display(),
                e
            ))
        })?;

        if !canonical.starts_with(&canonical_base) {
            return Err(ToolError::PathValidation(format!(
                "Path '{}' escapes base directory '{}' (resolved to '{}')",
                target_path.display(),
                canonical_base.display(),
                canonical.display()
            )));
        }

        Ok(canonical)
    } else {
        // For non-existent paths, verify the parent is within base
        let mut check_path = full_path.clone();

        // Find the first existing ancestor
        while !check_path.exists() {
            match check_path.parent() {
                Some(parent) => check_path = parent.to_path_buf(),
                None => {
                    return Err(ToolError::PathValidation(format!(
                        "Invalid path '{}': no valid parent directory exists",
                        target_path.display()
                    )))
                }
            }
        }

        // Canonicalize the existing ancestor and verify it's within base
        let canonical_ancestor = check_path.canonicalize().map_err(|e| {
            ToolError::PathValidation(format!(
                "Failed to canonicalize ancestor '{}': {}",
                check_path.display(),
                e
            ))
        })?;

        let canonical_base = base_path.canonicalize().map_err(|e| {
            ToolError::PathValidation(format!(
                "Failed to canonicalize base path '{}': {}",
                base_path.display(),
                e
            ))
        })?;

        if !canonical_ancestor.starts_with(&canonical_base) {
            return Err(ToolError::PathValidation(format!(
                "Path '{}' escapes base directory '{}' (nearest ancestor '{}' is outside)",
                target_path.display(),
                canonical_base.display(),
                canonical_ancestor.display()
            )));
        }

        Ok(full_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_validate_path_accepts_relative_path_to_existing_file() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("test.txt"), "content").unwrap();

        let result = validate_path(temp_dir.path(), Path::new("test.txt"));
        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.ends_with("test.txt"));
    }

    #[test]
    fn test_validate_path_accepts_relative_path_to_nonexistent_file() {
        let temp_dir = TempDir::new().unwrap();

        let result = validate_path(temp_dir.path(), Path::new("new_file.txt"));
        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.ends_with("new_file.txt"));
    }

    #[test]
    fn test_validate_path_accepts_nested_nonexistent_path() {
        let temp_dir = TempDir::new().unwrap();
        fs::create_dir(temp_dir.path().join("subdir")).unwrap();

        let result = validate_path(temp_dir.path(), Path::new("subdir/new_file.txt"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_path_rejects_traversal_existing_file() {
        let temp_dir = TempDir::new().unwrap();
        let sibling_dir = TempDir::new().unwrap();
        fs::write(sibling_dir.path().join("secret.txt"), "secret").unwrap();

        // Try to escape via ..
        let evil_path = format!(
            "../{}/secret.txt",
            sibling_dir.path().file_name().unwrap().to_str().unwrap()
        );
        let result = validate_path(temp_dir.path(), Path::new(&evil_path));

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("escapes") || err.to_string().contains("Invalid"),
            "Error should mention path escape: {}",
            err
        );
    }

    #[test]
    fn test_validate_path_rejects_absolute_path_outside_base() {
        let temp_dir = TempDir::new().unwrap();
        let other_dir = TempDir::new().unwrap();
        fs::write(other_dir.path().join("file.txt"), "content").unwrap();

        let result = validate_path(temp_dir.path(), other_dir.path().join("file.txt").as_path());

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("escapes"));
    }

    #[test]
    fn test_validate_path_accepts_absolute_path_inside_base() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("file.txt"), "content").unwrap();

        let absolute_path = temp_dir.path().join("file.txt");
        let result = validate_path(temp_dir.path(), &absolute_path);

        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_path_rejects_nonexistent_with_traversal() {
        let temp_dir = TempDir::new().unwrap();

        // Path doesn't exist but tries to escape
        let result = validate_path(temp_dir.path(), Path::new("../../../etc/shadow"));

        assert!(result.is_err());
    }

    #[test]
    fn test_validate_path_handles_symlink_inside_base() {
        let temp_dir = TempDir::new().unwrap();
        let real_file = temp_dir.path().join("real.txt");
        let symlink = temp_dir.path().join("link.txt");

        fs::write(&real_file, "content").unwrap();

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&real_file, &symlink).unwrap();

            let result = validate_path(temp_dir.path(), Path::new("link.txt"));
            assert!(result.is_ok(), "Symlink within base should be allowed");
        }
    }

    #[test]
    fn test_validate_path_rejects_symlink_escaping_base() {
        let temp_dir = TempDir::new().unwrap();
        let outside_dir = TempDir::new().unwrap();
        let outside_file = outside_dir.path().join("secret.txt");
        fs::write(&outside_file, "secret").unwrap();

        let symlink = temp_dir.path().join("escape_link.txt");

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside_file, &symlink).unwrap();

            let result = validate_path(temp_dir.path(), Path::new("escape_link.txt"));
            // After canonicalization, the symlink resolves outside base
            assert!(result.is_err(), "Symlink escaping base should be rejected");
        }
    }

    #[test]
    fn test_validate_path_deep_nesting() {
        let temp_dir = TempDir::new().unwrap();
        fs::create_dir_all(temp_dir.path().join("a/b/c/d/e")).unwrap();
        fs::write(temp_dir.path().join("a/b/c/d/e/deep.txt"), "deep").unwrap();

        let result = validate_path(temp_dir.path(), Path::new("a/b/c/d/e/deep.txt"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_path_dot_components() {
        let temp_dir = TempDir::new().unwrap();
        fs::create_dir(temp_dir.path().join("subdir")).unwrap();
        fs::write(temp_dir.path().join("subdir/file.txt"), "content").unwrap();

        // Path with . component
        let result = validate_path(temp_dir.path(), Path::new("./subdir/./file.txt"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_path_nonexistent_with_ancestor_escaping_base() {
        // This tests the branch at lines 71-75: when a non-existent path's
        // existing ancestor is outside the base directory
        let base_dir = TempDir::new().unwrap();
        let outside_dir = TempDir::new().unwrap();

        // Create a subdirectory outside base that will be our existing ancestor
        fs::create_dir(outside_dir.path().join("existing_subdir")).unwrap();

        // Try to access a non-existent file inside outside_dir using an absolute path
        // The file doesn't exist, but its ancestor (outside_dir/existing_subdir) does
        // and is outside base_dir
        let nonexistent_file = outside_dir.path().join("existing_subdir/new_file.txt");

        let result = validate_path(base_dir.path(), &nonexistent_file);

        assert!(
            result.is_err(),
            "Non-existent path with ancestor outside base should be rejected"
        );
        assert!(
            result.unwrap_err().to_string().contains("escapes"),
            "Error should mention path escape"
        );
    }

    #[test]
    fn test_validate_path_deeply_nested_nonexistent() {
        // Test deeply nested non-existent path where we walk up multiple levels
        let temp_dir = TempDir::new().unwrap();

        // Only the base exists, but we're trying to access deeply nested non-existent path
        let result = validate_path(temp_dir.path(), Path::new("a/b/c/d/e/f/g/new_file.txt"));

        // Should succeed because ancestor (temp_dir) is within base
        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.ends_with("a/b/c/d/e/f/g/new_file.txt"));
    }

    #[test]
    fn test_validate_path_nonexistent_relative_traversal_to_outside() {
        // Test traversal that ends up with existing ancestor outside base
        let base_dir = TempDir::new().unwrap();
        let sibling_dir = TempDir::new().unwrap();

        // Create a subdir in sibling so it's the ancestor found
        fs::create_dir(sibling_dir.path().join("subdir")).unwrap();

        // Try: ../sibling_temp_name/subdir/nonexistent.txt
        // The existing ancestor will be sibling_dir/subdir which is outside base
        let evil_path = format!(
            "../{}/subdir/nonexistent.txt",
            sibling_dir.path().file_name().unwrap().to_str().unwrap()
        );

        let result = validate_path(base_dir.path(), Path::new(&evil_path));

        assert!(
            result.is_err(),
            "Traversal to outside ancestor should be rejected"
        );
    }

    #[test]
    fn test_validate_path_error_includes_path_details() {
        // Verify error messages include actionable details for debugging
        let temp_dir = TempDir::new().unwrap();
        let other_dir = TempDir::new().unwrap();
        fs::write(other_dir.path().join("file.txt"), "content").unwrap();

        let result = validate_path(temp_dir.path(), other_dir.path().join("file.txt").as_path());

        let err = result.unwrap_err();
        let err_msg = err.to_string();

        // Error should mention the attempted path
        assert!(
            err_msg.contains("file.txt"),
            "Error should include the target path: {}",
            err_msg
        );

        // Error should mention escaping
        assert!(
            err_msg.contains("escapes"),
            "Error should mention 'escapes': {}",
            err_msg
        );

        // Error should include "resolved to" showing the canonical path
        assert!(
            err_msg.contains("resolved to"),
            "Error should show resolved path: {}",
            err_msg
        );
    }
}
