//! Test utilities for SQLite tools
//!
//! Provides helpers for creating isolated test databases that don't interfere
//! with each other when tests run in parallel.
//!
//! # Example
//!
//! ```ignore
//! use crate::sqlite::test_utils::{TestDatabase, unwrap_json};
//!
//! #[tokio::test]
//! async fn test_something() {
//!     // Create db with schema in one step
//!     let db = TestDatabase::with_schema("CREATE TABLE users (id INTEGER PRIMARY KEY)").await;
//!
//!     // Execute tool and unwrap result
//!     let result = some_tool.execute(input).await.unwrap();
//!     let json = unwrap_json(result);
//!     assert_eq!(json["status"], "success");
//! }
//! ```

use crate::sqlite::database::{OpenDatabaseInput, OpenDatabaseTool};
use crate::sqlite::manager::DATABASE_MANAGER;
use mixtape_core::tool::{Tool, ToolResult};
use std::path::PathBuf;
use tempfile::TempDir;

/// Unwraps a `ToolResult::Json` variant, panicking with a clear message if it's not JSON.
///
/// This eliminates the repetitive match pattern in tests:
/// ```ignore
/// // Before:
/// let json = match result {
///     ToolResult::Json(v) => v,
///     _ => panic!("Expected JSON result"),
/// };
///
/// // After:
/// let json = unwrap_json(result);
/// ```
pub fn unwrap_json(result: ToolResult) -> serde_json::Value {
    match result {
        ToolResult::Json(v) => v,
        other => panic!("Expected JSON result, got {:?}", other),
    }
}

/// A test database that is automatically cleaned up when dropped.
///
/// Each `TestDatabase` creates an isolated database file in a temporary directory.
/// Tests should use `self.key()` to explicitly reference their database instead of
/// relying on the global default, which enables parallel test execution.
///
/// # Example
///
/// ```ignore
/// use crate::sqlite::test_utils::TestDatabase;
///
/// #[tokio::test]
/// async fn test_something() {
///     let db = TestDatabase::new().await;
///
///     // Use db.key() explicitly instead of None
///     let input = SomeInput {
///         db_path: Some(db.key()),
///         // ...
///     };
/// }
/// ```
pub struct TestDatabase {
    #[allow(dead_code)]
    temp_dir: TempDir,
    key: String,
}

impl TestDatabase {
    /// Creates a new isolated test database.
    ///
    /// The database is created in a unique temporary directory and opened
    /// via the standard `OpenDatabaseTool`. The returned key can be used
    /// to reference this database in subsequent operations.
    pub async fn new() -> Self {
        Self::with_name("test.db").await
    }

    /// Creates a new isolated test database with a specific filename.
    pub async fn with_name(name: &str) -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let db_path = temp_dir.path().join(name);

        let tool = OpenDatabaseTool;
        let input = OpenDatabaseInput {
            db_path,
            create: true,
        };
        let result = tool
            .execute(input)
            .await
            .expect("Failed to open test database");

        // Extract the key from the tool's response - this is the canonical key
        // that the manager actually stored
        let key = match result {
            ToolResult::Json(json) => json["database"]
                .as_str()
                .expect("OpenDatabaseTool should return database key")
                .to_string(),
            other => panic!(
                "Expected JSON result from OpenDatabaseTool, got {:?}",
                other
            ),
        };

        Self { temp_dir, key }
    }

    /// Returns the database key to use in tool inputs.
    ///
    /// Always use this instead of `None` to ensure test isolation.
    pub fn key(&self) -> String {
        self.key.clone()
    }

    /// Returns the database path.
    pub fn path(&self) -> PathBuf {
        PathBuf::from(&self.key)
    }

    /// Creates a new test database and executes the given schema SQL.
    ///
    /// This is a convenience method that combines `new()` with `execute()`:
    /// ```ignore
    /// // Before:
    /// let db = TestDatabase::new().await;
    /// {
    ///     let conn = DATABASE_MANAGER.get(Some(&db.key())).unwrap();
    ///     let conn = conn.lock().unwrap();
    ///     conn.execute_batch("CREATE TABLE users (id INTEGER PRIMARY KEY);").unwrap();
    /// }
    ///
    /// // After:
    /// let db = TestDatabase::with_schema("CREATE TABLE users (id INTEGER PRIMARY KEY);").await;
    /// ```
    pub async fn with_schema(schema: &str) -> Self {
        let db = Self::new().await;
        db.execute(schema);
        db
    }

    /// Executes SQL on this test database.
    ///
    /// Panics if the SQL fails to execute (appropriate for tests).
    /// ```ignore
    /// let db = TestDatabase::new().await;
    /// db.execute("CREATE TABLE users (id INTEGER PRIMARY KEY)");
    /// db.execute("INSERT INTO users VALUES (1), (2), (3)");
    /// ```
    pub fn execute(&self, sql: &str) {
        let conn = DATABASE_MANAGER
            .get(Some(&self.key))
            .expect("Failed to get test database connection");
        let conn = conn.lock().unwrap();
        conn.execute_batch(sql)
            .expect("Failed to execute SQL in test database");
    }

    /// Executes a query and returns all rows as a vector of JSON values.
    ///
    /// Useful for verifying test data without going through a tool.
    /// ```ignore
    /// let db = TestDatabase::with_schema("CREATE TABLE users (id INTEGER, name TEXT)").await;
    /// db.execute("INSERT INTO users VALUES (1, 'Alice'), (2, 'Bob')");
    /// let rows = db.query("SELECT * FROM users ORDER BY id");
    /// assert_eq!(rows.len(), 2);
    /// ```
    pub fn query(&self, sql: &str) -> Vec<Vec<serde_json::Value>> {
        let conn = DATABASE_MANAGER
            .get(Some(&self.key))
            .expect("Failed to get test database connection");
        let conn = conn.lock().unwrap();

        let mut stmt = conn.prepare(sql).expect("Failed to prepare query");
        let column_count = stmt.column_count();

        let rows: Vec<Vec<serde_json::Value>> = stmt
            .query_map([], |row| {
                let mut values = Vec::with_capacity(column_count);
                for i in 0..column_count {
                    let value = match row.get_ref(i)? {
                        rusqlite::types::ValueRef::Null => serde_json::Value::Null,
                        rusqlite::types::ValueRef::Integer(n) => serde_json::json!(n),
                        rusqlite::types::ValueRef::Real(f) => serde_json::json!(f),
                        rusqlite::types::ValueRef::Text(s) => {
                            serde_json::json!(String::from_utf8_lossy(s))
                        }
                        rusqlite::types::ValueRef::Blob(b) => {
                            serde_json::json!(base64::Engine::encode(
                                &base64::engine::general_purpose::STANDARD,
                                b
                            ))
                        }
                    };
                    values.push(value);
                }
                Ok(values)
            })
            .expect("Failed to execute query")
            .filter_map(|r| r.ok())
            .collect();

        rows
    }

    /// Counts rows in a table. Convenience for common test assertion.
    /// ```ignore
    /// let db = TestDatabase::with_schema("CREATE TABLE users (id INTEGER)").await;
    /// db.execute("INSERT INTO users VALUES (1), (2)");
    /// assert_eq!(db.count("users"), 2);
    /// ```
    pub fn count(&self, table: &str) -> i64 {
        let conn = DATABASE_MANAGER
            .get(Some(&self.key))
            .expect("Failed to get test database connection");
        let conn = conn.lock().unwrap();
        conn.query_row(&format!("SELECT COUNT(*) FROM \"{}\"", table), [], |row| {
            row.get(0)
        })
        .expect("Failed to count rows")
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        // Close this specific database, ignoring errors during cleanup
        let _ = DATABASE_MANAGER.close(&self.key);
    }
}
