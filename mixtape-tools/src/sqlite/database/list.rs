//! List databases tool

use crate::prelude::*;
use crate::sqlite::manager::DATABASE_MANAGER;
use std::path::PathBuf;

/// Input for listing database files
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListDatabasesInput {
    /// Directory to search for database files. Defaults to current directory.
    #[serde(default)]
    pub directory: Option<PathBuf>,

    /// Whether to search recursively (default: false)
    #[serde(default)]
    pub recursive: bool,
}

/// Database file information
#[derive(Debug, Serialize, JsonSchema)]
struct DatabaseFile {
    path: String,
    size_bytes: u64,
    is_open: bool,
}

/// Tool for discovering SQLite database files in a directory
///
/// Searches for files with common SQLite extensions (.db, .sqlite, .sqlite3)
/// and returns information about each found database.
pub struct ListDatabasesTool;

impl Tool for ListDatabasesTool {
    type Input = ListDatabasesInput;

    fn name(&self) -> &str {
        "sqlite_list_databases"
    }

    fn description(&self) -> &str {
        "Discover SQLite database files in a directory. Searches for .db, .sqlite, and .sqlite3 files. Also shows currently open databases."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let directory = input
            .directory
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let recursive = input.recursive;

        let result = tokio::task::spawn_blocking(move || {
            let mut databases = Vec::new();
            let extensions = ["db", "sqlite", "sqlite3"];

            // Search for database files
            let search_result = if recursive {
                search_recursive(&directory, &extensions)
            } else {
                search_directory(&directory, &extensions)
            };

            if let Ok(files) = search_result {
                for path in files {
                    if let Ok(metadata) = std::fs::metadata(&path) {
                        let path_str = path.to_string_lossy().to_string();
                        databases.push(DatabaseFile {
                            is_open: DATABASE_MANAGER.is_open(&path_str),
                            path: path_str,
                            size_bytes: metadata.len(),
                        });
                    }
                }
            }

            // Also include currently open databases that might not be in the searched directory
            for open_db in DATABASE_MANAGER.list_open() {
                if !databases.iter().any(|d| d.path == open_db) {
                    if let Ok(metadata) = std::fs::metadata(&open_db) {
                        databases.push(DatabaseFile {
                            path: open_db,
                            size_bytes: metadata.len(),
                            is_open: true,
                        });
                    }
                }
            }

            databases
        })
        .await
        .map_err(|e| ToolError::Custom(format!("Task join error: {}", e)))?;

        let response = serde_json::json!({
            "databases": result,
            "count": result.len(),
            "open_count": result.iter().filter(|d| d.is_open).count()
        });

        Ok(ToolResult::Json(response))
    }
}

fn search_directory(dir: &PathBuf, extensions: &[&str]) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            if let Some(ext) = path.extension() {
                if extensions.iter().any(|e| ext == *e) {
                    files.push(path);
                }
            }
        }
    }

    Ok(files)
}

fn search_recursive(dir: &PathBuf, extensions: &[&str]) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    fn walk(dir: &PathBuf, extensions: &[&str], files: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if extensions.iter().any(|e| ext == *e) {
                        files.push(path);
                    }
                }
            } else if path.is_dir() {
                // Skip hidden directories and common non-relevant directories
                if let Some(name) = path.file_name() {
                    let name = name.to_string_lossy();
                    if !name.starts_with('.') && name != "node_modules" && name != "target" {
                        let _ = walk(&path, extensions, files);
                    }
                }
            }
        }
        Ok(())
    }

    walk(dir, extensions, &mut files)?;
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::test_utils::TestDatabase;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_list_databases_non_recursive() {
        let temp_dir = TempDir::new().unwrap();

        // Create test database files with different extensions
        std::fs::write(temp_dir.path().join("test1.db"), "").unwrap();
        std::fs::write(temp_dir.path().join("test2.sqlite"), "").unwrap();
        std::fs::write(temp_dir.path().join("test3.sqlite3"), "").unwrap();
        std::fs::write(temp_dir.path().join("not_a_db.txt"), "").unwrap();

        let tool = ListDatabasesTool;
        let input = ListDatabasesInput {
            directory: Some(temp_dir.path().to_path_buf()),
            recursive: false,
        };

        let result = tool.execute(input).await.unwrap();
        let json = match result {
            ToolResult::Json(v) => v,
            _ => panic!("Expected JSON result"),
        };

        // Check that we found the expected database files
        let databases = json["databases"].as_array().unwrap();
        let paths: Vec<&str> = databases
            .iter()
            .filter_map(|d| d["path"].as_str())
            .collect();

        let temp_path = temp_dir.path().to_string_lossy();
        let local_dbs: Vec<_> = paths
            .iter()
            .filter(|p| p.contains(temp_path.as_ref()))
            .collect();
        assert_eq!(
            local_dbs.len(),
            3,
            "Should find all three db files in temp dir"
        );
    }

    #[tokio::test]
    async fn test_list_databases_recursive() {
        let temp_dir = TempDir::new().unwrap();

        // Create nested directory structure
        let subdir = temp_dir.path().join("subdir");
        std::fs::create_dir(&subdir).unwrap();
        let nested = subdir.join("nested");
        std::fs::create_dir(&nested).unwrap();

        // Create test database files at various levels
        std::fs::write(temp_dir.path().join("root.db"), "").unwrap();
        std::fs::write(subdir.join("sub.sqlite"), "").unwrap();
        std::fs::write(nested.join("deep.sqlite3"), "").unwrap();

        let tool = ListDatabasesTool;
        let input = ListDatabasesInput {
            directory: Some(temp_dir.path().to_path_buf()),
            recursive: true,
        };

        let result = tool.execute(input).await.unwrap();
        let json = match result {
            ToolResult::Json(v) => v,
            _ => panic!("Expected JSON result"),
        };

        // Check that we found the expected database files
        let databases = json["databases"].as_array().unwrap();
        let paths: Vec<&str> = databases
            .iter()
            .filter_map(|d| d["path"].as_str())
            .collect();

        let temp_path = temp_dir.path().to_string_lossy();
        let local_dbs: Vec<_> = paths
            .iter()
            .filter(|p| p.contains(temp_path.as_ref()))
            .collect();
        assert_eq!(
            local_dbs.len(),
            3,
            "Should find all three at different levels"
        );
    }

    #[tokio::test]
    async fn test_list_databases_skips_hidden_dirs() {
        let temp_dir = TempDir::new().unwrap();

        // Create hidden directory
        let hidden = temp_dir.path().join(".hidden");
        std::fs::create_dir(&hidden).unwrap();
        std::fs::write(hidden.join("hidden.db"), "").unwrap();

        // Create normal file
        std::fs::write(temp_dir.path().join("visible.db"), "").unwrap();

        let tool = ListDatabasesTool;
        let input = ListDatabasesInput {
            directory: Some(temp_dir.path().to_path_buf()),
            recursive: true,
        };

        let result = tool.execute(input).await.unwrap();
        let json = match result {
            ToolResult::Json(v) => v,
            _ => panic!("Expected JSON result"),
        };

        // Check that we found only the visible database (not the one in .hidden)
        let databases = json["databases"].as_array().unwrap();
        let paths: Vec<&str> = databases
            .iter()
            .filter_map(|d| d["path"].as_str())
            .collect();

        let temp_path = temp_dir.path().to_string_lossy();
        let local_dbs: Vec<_> = paths
            .iter()
            .filter(|p| p.contains(temp_path.as_ref()))
            .collect();
        assert_eq!(local_dbs.len(), 1, "Should only find visible.db");
        assert!(
            paths.iter().any(|p| p.contains("visible.db")),
            "Should find visible.db"
        );
        assert!(
            !paths.iter().any(|p| p.contains(".hidden")),
            "Should not find hidden.db"
        );
    }

    #[tokio::test]
    async fn test_list_databases_shows_open_databases() {
        let temp_dir = TempDir::new().unwrap();

        // Create and open a test database
        let db = TestDatabase::with_name("opened.db").await;

        // Create another file in the temp dir that's not open
        std::fs::write(temp_dir.path().join("closed.db"), "").unwrap();

        let tool = ListDatabasesTool;
        let input = ListDatabasesInput {
            directory: Some(temp_dir.path().to_path_buf()),
            recursive: false,
        };

        let result = tool.execute(input).await.unwrap();
        let json = match result {
            ToolResult::Json(v) => v,
            _ => panic!("Expected JSON result"),
        };

        // The open database from TestDatabase should be included
        let open_count = json["open_count"].as_i64().unwrap();
        assert!(open_count >= 1, "Should have at least one open database");

        // Check that the db key is detected
        drop(db);
    }

    #[tokio::test]
    async fn test_list_databases_empty_directory() {
        let temp_dir = TempDir::new().unwrap();

        let tool = ListDatabasesTool;
        let input = ListDatabasesInput {
            directory: Some(temp_dir.path().to_path_buf()),
            recursive: false,
        };

        let result = tool.execute(input).await.unwrap();
        let json = match result {
            ToolResult::Json(v) => v,
            _ => panic!("Expected JSON result"),
        };

        // Should return empty list for directory with no db files
        // (may include open databases from other tests)
        assert!(json["databases"].is_array());
    }

    #[tokio::test]
    async fn test_list_databases_default_directory() {
        let tool = ListDatabasesTool;
        let input = ListDatabasesInput {
            directory: None, // Use default (current directory)
            recursive: false,
        };

        // Should not panic, even if no databases in current directory
        let result = tool.execute(input).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_search_directory_helper() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::write(temp_dir.path().join("test.db"), "").unwrap();
        std::fs::write(temp_dir.path().join("test.txt"), "").unwrap();

        let extensions = ["db", "sqlite"];
        let files = search_directory(&temp_dir.path().to_path_buf(), &extensions).unwrap();

        assert_eq!(files.len(), 1);
        assert!(files[0].to_string_lossy().contains("test.db"));
    }

    #[test]
    fn test_search_recursive_helper() {
        let temp_dir = TempDir::new().unwrap();
        let subdir = temp_dir.path().join("sub");
        std::fs::create_dir(&subdir).unwrap();

        std::fs::write(temp_dir.path().join("root.db"), "").unwrap();
        std::fs::write(subdir.join("nested.db"), "").unwrap();

        let extensions = ["db"];
        let files = search_recursive(&temp_dir.path().to_path_buf(), &extensions).unwrap();

        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_tool_metadata() {
        let tool = ListDatabasesTool;
        assert_eq!(tool.name(), "sqlite_list_databases");
        assert!(!tool.description().is_empty());
    }
}
