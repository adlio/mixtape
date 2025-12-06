//! Write query tool

use crate::prelude::*;
use crate::sqlite::error::SqliteToolError;
use crate::sqlite::manager::with_connection;
use crate::sqlite::types::json_to_sql;

/// Input for write query execution
#[derive(Debug, Deserialize, JsonSchema)]
pub struct WriteQueryInput {
    /// SQL query to execute (INSERT, UPDATE, DELETE)
    pub query: String,

    /// Query parameters for prepared statements
    #[serde(default)]
    pub params: Vec<serde_json::Value>,

    /// Database file path. If not specified, uses the default database.
    #[serde(default)]
    pub db_path: Option<String>,
}

/// Write query result
#[derive(Debug, Serialize, JsonSchema)]
struct WriteResult {
    status: String,
    rows_affected: usize,
    last_insert_rowid: Option<i64>,
}

/// Tool for executing data modification queries (DESTRUCTIVE)
///
/// Executes INSERT, UPDATE, and DELETE queries.
/// Returns the number of rows affected and last insert rowid (for INSERT).
pub struct WriteQueryTool;

impl WriteQueryTool {
    /// Validates that a query is a write operation
    fn is_write_query(sql: &str) -> bool {
        let normalized = sql.trim().to_uppercase();
        let write_prefixes = ["INSERT", "UPDATE", "DELETE", "REPLACE"];
        write_prefixes
            .iter()
            .any(|prefix| normalized.starts_with(prefix))
    }
}

impl Tool for WriteQueryTool {
    type Input = WriteQueryInput;

    fn name(&self) -> &str {
        "sqlite_write_query"
    }

    fn description(&self) -> &str {
        "Execute a data modification SQL query (INSERT, UPDATE, DELETE). Returns the number of rows affected."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        // Validate query is a write operation
        if !Self::is_write_query(&input.query) {
            return Err(SqliteToolError::InvalidQuery(
                "Only INSERT, UPDATE, DELETE, and REPLACE queries are allowed. Use sqlite_read_query for SELECT or sqlite_schema_query for DDL.".to_string()
            ).into());
        }

        let query = input.query;
        let params = input.params;

        let result = with_connection(input.db_path, move |conn| {
            // Convert params to rusqlite values
            let params_ref: Vec<Box<dyn rusqlite::ToSql>> =
                params.iter().map(|v| json_to_sql(v)).collect();

            let params_slice: Vec<&dyn rusqlite::ToSql> =
                params_ref.iter().map(|b| b.as_ref()).collect();

            let rows_affected = conn.execute(&query, params_slice.as_slice())?;

            // Get last insert rowid for INSERT queries
            let last_insert_rowid = if query.trim().to_uppercase().starts_with("INSERT") {
                Some(conn.last_insert_rowid())
            } else {
                None
            };

            Ok(WriteResult {
                status: "success".to_string(),
                rows_affected,
                last_insert_rowid,
            })
        })
        .await?;

        Ok(ToolResult::Json(serde_json::to_value(result)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::test_utils::{unwrap_json, TestDatabase};
    use mixtape_core::tool::Tool;

    #[tokio::test]
    async fn test_write_query_insert() {
        let db =
            TestDatabase::with_schema("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)")
                .await;

        let tool = WriteQueryTool;
        let result = tool
            .execute(WriteQueryInput {
                query: "INSERT INTO users (name) VALUES (?)".to_string(),
                params: vec![serde_json::json!("Alice")],
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["rows_affected"], 1);
        assert!(json["last_insert_rowid"].as_i64().is_some());
    }

    #[tokio::test]
    async fn test_write_query_update() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE users (id INTEGER, name TEXT);
             INSERT INTO users VALUES (1, 'Alice');
             INSERT INTO users VALUES (2, 'Bob');",
        )
        .await;

        let tool = WriteQueryTool;
        let result = tool
            .execute(WriteQueryInput {
                query: "UPDATE users SET name = 'Updated'".to_string(),
                params: vec![],
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["rows_affected"], 2);
    }

    #[tokio::test]
    async fn test_reject_select_query() {
        let db = TestDatabase::new().await;

        let result = WriteQueryTool
            .execute(WriteQueryInput {
                query: "SELECT * FROM users".to_string(),
                params: vec![],
                db_path: Some(db.key()),
            })
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_is_write_query() {
        assert!(WriteQueryTool::is_write_query(
            "INSERT INTO users VALUES (1)"
        ));
        assert!(WriteQueryTool::is_write_query(
            "UPDATE users SET name = 'x'"
        ));
        assert!(WriteQueryTool::is_write_query("DELETE FROM users"));
        assert!(WriteQueryTool::is_write_query(
            "REPLACE INTO users VALUES (1)"
        ));

        assert!(!WriteQueryTool::is_write_query("SELECT * FROM users"));
        assert!(!WriteQueryTool::is_write_query(
            "CREATE TABLE users (id INT)"
        ));
        assert!(!WriteQueryTool::is_write_query("DROP TABLE users"));
    }

    #[test]
    fn test_tool_metadata() {
        let tool = WriteQueryTool;
        assert_eq!(tool.name(), "sqlite_write_query");
        assert!(!tool.description().is_empty());
    }

    #[tokio::test]
    async fn test_write_query_delete() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE users (id INTEGER, name TEXT);
             INSERT INTO users VALUES (1, 'Alice');
             INSERT INTO users VALUES (2, 'Bob');
             INSERT INTO users VALUES (3, 'Charlie');",
        )
        .await;

        let result = WriteQueryTool
            .execute(WriteQueryInput {
                query: "DELETE FROM users WHERE id > 1".to_string(),
                params: vec![],
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["status"], "success");
        assert_eq!(json["rows_affected"], 2);
        assert!(json["last_insert_rowid"].is_null());
        assert_eq!(db.count("users"), 1);
    }

    #[tokio::test]
    async fn test_write_query_delete_all() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE users (id INTEGER, name TEXT);
             INSERT INTO users VALUES (1, 'Alice');
             INSERT INTO users VALUES (2, 'Bob');",
        )
        .await;

        let result = WriteQueryTool
            .execute(WriteQueryInput {
                query: "DELETE FROM users".to_string(),
                params: vec![],
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["rows_affected"], 2);
    }

    #[tokio::test]
    async fn test_write_query_replace() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
             INSERT INTO users VALUES (1, 'Alice');",
        )
        .await;

        let result = WriteQueryTool
            .execute(WriteQueryInput {
                query: "REPLACE INTO users VALUES (1, 'Updated Alice')".to_string(),
                params: vec![],
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["status"], "success");
        assert_eq!(json["rows_affected"], 1);

        // Verify replacement
        let rows = db.query("SELECT name FROM users WHERE id = 1");
        assert_eq!(rows[0][0], "Updated Alice");
        assert_eq!(db.count("users"), 1);
    }

    #[tokio::test]
    async fn test_write_query_replace_new_row() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
             INSERT INTO users VALUES (1, 'Alice');",
        )
        .await;

        let result = WriteQueryTool
            .execute(WriteQueryInput {
                query: "REPLACE INTO users VALUES (2, 'Bob')".to_string(),
                params: vec![],
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["rows_affected"], 1);
        assert_eq!(db.count("users"), 2);
    }

    #[tokio::test]
    async fn test_write_query_parameterized_insert() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE data (id INTEGER, name TEXT, score REAL, active INTEGER)",
        )
        .await;

        let result = WriteQueryTool
            .execute(WriteQueryInput {
                query: "INSERT INTO data VALUES (?, ?, ?, ?)".to_string(),
                params: vec![
                    serde_json::json!(1),
                    serde_json::json!("Alice"),
                    serde_json::json!(95.5),
                    serde_json::json!(true),
                ],
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["rows_affected"], 1);

        // Verify data
        let rows = db.query("SELECT name, score, active FROM data WHERE id = 1");
        assert_eq!(rows[0][0], "Alice");
        assert_eq!(rows[0][2], 1); // true -> 1
    }

    #[tokio::test]
    async fn test_write_query_parameterized_update() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE users (id INTEGER, name TEXT);
             INSERT INTO users VALUES (1, 'Alice');
             INSERT INTO users VALUES (2, 'Bob');",
        )
        .await;

        let result = WriteQueryTool
            .execute(WriteQueryInput {
                query: "UPDATE users SET name = ? WHERE id = ?".to_string(),
                params: vec![serde_json::json!("Updated"), serde_json::json!(1)],
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["rows_affected"], 1);
        assert!(json["last_insert_rowid"].is_null());

        let rows = db.query("SELECT name FROM users WHERE id = 1");
        assert_eq!(rows[0][0], "Updated");
    }

    #[tokio::test]
    async fn test_write_query_parameterized_delete() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE users (id INTEGER, name TEXT);
             INSERT INTO users VALUES (1, 'Alice');
             INSERT INTO users VALUES (2, 'Bob');
             INSERT INTO users VALUES (3, 'Charlie');",
        )
        .await;

        let result = WriteQueryTool
            .execute(WriteQueryInput {
                query: "DELETE FROM users WHERE name = ?".to_string(),
                params: vec![serde_json::json!("Bob")],
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["rows_affected"], 1);
        assert_eq!(db.count("users"), 2);
    }

    #[tokio::test]
    async fn test_write_query_null_parameter() {
        let db = TestDatabase::with_schema("CREATE TABLE users (id INTEGER, name TEXT)").await;

        let result = WriteQueryTool
            .execute(WriteQueryInput {
                query: "INSERT INTO users VALUES (?, ?)".to_string(),
                params: vec![serde_json::json!(1), serde_json::Value::Null],
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["rows_affected"], 1);

        let rows = db.query("SELECT name FROM users WHERE id = 1");
        assert!(rows[0][0].is_null());
    }

    #[tokio::test]
    async fn test_write_query_json_object_parameter() {
        let db = TestDatabase::with_schema("CREATE TABLE data (id INTEGER, metadata TEXT)").await;

        let result = WriteQueryTool
            .execute(WriteQueryInput {
                query: "INSERT INTO data VALUES (?, ?)".to_string(),
                params: vec![
                    serde_json::json!(1),
                    serde_json::json!({"key": "value", "count": 42}),
                ],
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["rows_affected"], 1);

        let rows = db.query("SELECT metadata FROM data WHERE id = 1");
        let parsed: serde_json::Value = serde_json::from_str(rows[0][0].as_str().unwrap()).unwrap();
        assert_eq!(parsed["key"], "value");
        assert_eq!(parsed["count"], 42);
    }

    #[tokio::test]
    async fn test_write_query_no_rows_affected() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE users (id INTEGER, name TEXT);
             INSERT INTO users VALUES (1, 'Alice');",
        )
        .await;

        let result = WriteQueryTool
            .execute(WriteQueryInput {
                query: "DELETE FROM users WHERE id = 999".to_string(),
                params: vec![],
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["status"], "success");
        assert_eq!(json["rows_affected"], 0);
    }

    #[tokio::test]
    async fn test_update_no_last_insert_rowid() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
             INSERT INTO users VALUES (1, 'Alice');",
        )
        .await;

        let result = WriteQueryTool
            .execute(WriteQueryInput {
                query: "UPDATE users SET name = 'Updated' WHERE id = 1".to_string(),
                params: vec![],
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert!(json["last_insert_rowid"].is_null());
    }
}
