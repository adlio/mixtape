//! Read query tool

use crate::prelude::*;
use crate::sqlite::error::SqliteToolError;
use crate::sqlite::manager::with_connection;
use crate::sqlite::types::{json_to_sql, QueryResult};
use rusqlite::types::ValueRef;

/// Input for read query execution
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadQueryInput {
    /// SQL query to execute (SELECT, PRAGMA, or EXPLAIN only)
    pub query: String,

    /// Query parameters for prepared statements
    #[serde(default)]
    pub params: Vec<serde_json::Value>,

    /// Database file path. If not specified, uses the default database.
    #[serde(default)]
    pub db_path: Option<String>,

    /// Maximum number of rows to return (default: 1000)
    #[serde(default = "default_limit")]
    pub limit: usize,

    /// Number of rows to skip (default: 0)
    #[serde(default)]
    pub offset: usize,
}

impl ReadQueryInput {
    /// Creates a new ReadQueryInput with the given query and default values.
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            params: vec![],
            db_path: None,
            limit: 1000,
            offset: 0,
        }
    }

    /// Sets the database path.
    pub fn db_path(mut self, path: impl Into<String>) -> Self {
        self.db_path = Some(path.into());
        self
    }

    /// Sets the query parameters.
    pub fn params(mut self, params: Vec<serde_json::Value>) -> Self {
        self.params = params;
        self
    }
}

fn default_limit() -> usize {
    1000
}

/// Tool for executing read-only queries (SAFE)
///
/// Executes SELECT, PRAGMA, and EXPLAIN queries.
/// Other query types will be rejected for safety.
pub struct ReadQueryTool;

impl ReadQueryTool {
    /// Validates that a query is read-only
    fn is_read_only(sql: &str) -> bool {
        let normalized = sql.trim().to_uppercase();

        // Check for allowed prefixes
        let allowed_prefixes = ["SELECT", "PRAGMA", "EXPLAIN", "WITH"];

        // WITH queries should eventually lead to SELECT
        if normalized.starts_with("WITH") {
            // Basic check - could be more sophisticated
            return normalized.contains("SELECT");
        }

        allowed_prefixes
            .iter()
            .any(|prefix| normalized.starts_with(prefix))
    }
}

impl Tool for ReadQueryTool {
    type Input = ReadQueryInput;

    fn name(&self) -> &str {
        "sqlite_read_query"
    }

    fn description(&self) -> &str {
        "Execute a read-only SQL query (SELECT, PRAGMA, EXPLAIN). Returns the query results with column names and row data."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        // Validate query is read-only
        if !Self::is_read_only(&input.query) {
            return Err(SqliteToolError::InvalidQuery(
                "Only SELECT, PRAGMA, EXPLAIN, and WITH...SELECT queries are allowed. Use sqlite_write_query for modifications.".to_string()
            ).into());
        }

        let query = input.query;
        let params = input.params;
        let limit = input.limit;
        let offset = input.offset;

        let result = with_connection(input.db_path, move |conn| {
            let mut stmt = conn.prepare(&query)?;

            // Get column names
            let columns: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();

            // Convert params to rusqlite values
            let params_ref: Vec<Box<dyn rusqlite::ToSql>> =
                params.iter().map(|v| json_to_sql(v)).collect();

            let params_slice: Vec<&dyn rusqlite::ToSql> =
                params_ref.iter().map(|b| b.as_ref()).collect();

            // Execute query and collect rows
            let mut rows_result = stmt.query(params_slice.as_slice())?;
            let mut rows: Vec<Vec<serde_json::Value>> = Vec::new();
            let mut skipped = 0;

            while let Some(row) = rows_result.next()? {
                // Handle offset
                if skipped < offset {
                    skipped += 1;
                    continue;
                }

                // Handle limit
                if rows.len() >= limit {
                    break;
                }

                let mut row_data: Vec<serde_json::Value> = Vec::new();
                for i in 0..columns.len() {
                    let value = row.get_ref(i)?;
                    row_data.push(sql_to_json(value));
                }
                rows.push(row_data);
            }

            Ok(QueryResult {
                row_count: rows.len(),
                columns,
                rows,
                rows_affected: None,
            })
        })
        .await?;

        Ok(ToolResult::Json(serde_json::to_value(result)?))
    }
}

/// Convert a rusqlite value to JSON
fn sql_to_json(value: ValueRef) -> serde_json::Value {
    match value {
        ValueRef::Null => serde_json::Value::Null,
        ValueRef::Integer(i) => serde_json::Value::Number(i.into()),
        ValueRef::Real(f) => serde_json::Number::from_f64(f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        ValueRef::Text(s) => serde_json::Value::String(String::from_utf8_lossy(s).to_string()),
        ValueRef::Blob(b) => {
            // Return as base64-encoded string
            use base64::Engine;
            serde_json::Value::String(base64::engine::general_purpose::STANDARD.encode(b))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::test_utils::{unwrap_json, TestDatabase};

    #[tokio::test]
    async fn test_read_query() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE users (id INTEGER, name TEXT);
             INSERT INTO users VALUES (1, 'Alice');
             INSERT INTO users VALUES (2, 'Bob');",
        )
        .await;

        let result = ReadQueryTool
            .execute(ReadQueryInput::new("SELECT * FROM users ORDER BY id").db_path(db.key()))
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["row_count"].as_i64().unwrap(), 2);
        assert_eq!(json["columns"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_reject_write_query() {
        let db = TestDatabase::new().await;

        let result = ReadQueryTool
            .execute(ReadQueryInput::new("INSERT INTO users VALUES (1, 'test')").db_path(db.key()))
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_is_read_only() {
        assert!(ReadQueryTool::is_read_only("SELECT * FROM users"));
        assert!(ReadQueryTool::is_read_only("  SELECT * FROM users"));
        assert!(ReadQueryTool::is_read_only("PRAGMA table_info(users)"));
        assert!(ReadQueryTool::is_read_only("EXPLAIN SELECT * FROM users"));
        assert!(ReadQueryTool::is_read_only(
            "WITH cte AS (SELECT 1) SELECT * FROM cte"
        ));

        assert!(!ReadQueryTool::is_read_only("INSERT INTO users VALUES (1)"));
        assert!(!ReadQueryTool::is_read_only("UPDATE users SET name = 'x'"));
        assert!(!ReadQueryTool::is_read_only("DELETE FROM users"));
        assert!(!ReadQueryTool::is_read_only("DROP TABLE users"));
    }

    #[test]
    fn test_tool_metadata() {
        let tool = ReadQueryTool;
        assert_eq!(tool.name(), "sqlite_read_query");
        assert!(!tool.description().is_empty());
    }

    #[tokio::test]
    async fn test_parameterized_query_with_types() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE data (id INTEGER, name TEXT, score REAL, active INTEGER);
             INSERT INTO data VALUES (1, 'Alice', 95.5, 1);
             INSERT INTO data VALUES (2, 'Bob', 87.0, 0);
             INSERT INTO data VALUES (3, NULL, 72.5, 1);",
        )
        .await;

        // Test with integer parameter
        let result = ReadQueryTool
            .execute(ReadQueryInput {
                query: "SELECT * FROM data WHERE id = ?".to_string(),
                params: vec![serde_json::json!(2)],
                db_path: Some(db.key()),
                limit: 1000,
                offset: 0,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["row_count"], 1);
        assert_eq!(json["rows"][0][1], "Bob");

        // Test with string parameter
        let json = unwrap_json(
            ReadQueryTool
                .execute(ReadQueryInput {
                    query: "SELECT * FROM data WHERE name = ?".to_string(),
                    params: vec![serde_json::json!("Alice")],
                    db_path: Some(db.key()),
                    limit: 1000,
                    offset: 0,
                })
                .await
                .unwrap(),
        );
        assert_eq!(json["row_count"], 1);

        // Test with float parameter
        let json = unwrap_json(
            ReadQueryTool
                .execute(ReadQueryInput {
                    query: "SELECT * FROM data WHERE score > ?".to_string(),
                    params: vec![serde_json::json!(90.0)],
                    db_path: Some(db.key()),
                    limit: 1000,
                    offset: 0,
                })
                .await
                .unwrap(),
        );
        assert_eq!(json["row_count"], 1);

        // Test with boolean parameter (converts to 1/0)
        let json = unwrap_json(
            ReadQueryTool
                .execute(ReadQueryInput {
                    query: "SELECT * FROM data WHERE active = ?".to_string(),
                    params: vec![serde_json::json!(true)],
                    db_path: Some(db.key()),
                    limit: 1000,
                    offset: 0,
                })
                .await
                .unwrap(),
        );
        assert_eq!(json["row_count"], 2);

        // Test with multiple parameters
        let json = unwrap_json(
            ReadQueryTool
                .execute(ReadQueryInput {
                    query: "SELECT * FROM data WHERE id > ? AND score < ?".to_string(),
                    params: vec![serde_json::json!(1), serde_json::json!(80.0)],
                    db_path: Some(db.key()),
                    limit: 1000,
                    offset: 0,
                })
                .await
                .unwrap(),
        );
        assert_eq!(json["row_count"], 1);
        assert_eq!(json["rows"][0][0], 3);
    }

    #[tokio::test]
    async fn test_null_parameter() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE data (id INTEGER, name TEXT);
             INSERT INTO data VALUES (1, 'Alice');
             INSERT INTO data VALUES (2, NULL);",
        )
        .await;

        let result = ReadQueryTool
            .execute(ReadQueryInput {
                query: "SELECT * FROM data WHERE name IS ?".to_string(),
                params: vec![serde_json::Value::Null],
                db_path: Some(db.key()),
                limit: 1000,
                offset: 0,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["row_count"], 1);
        assert_eq!(json["rows"][0][0], 2);
    }

    #[tokio::test]
    async fn test_blob_data_base64() {
        let db = TestDatabase::with_schema("CREATE TABLE files (id INTEGER, data BLOB);").await;
        // Insert raw bytes
        db.execute("INSERT INTO files VALUES (1, X'48656C6C6F')"); // "Hello" in hex
        db.execute("INSERT INTO files VALUES (2, X'0001020304')");

        let result = ReadQueryTool
            .execute(ReadQueryInput {
                query: "SELECT * FROM files ORDER BY id".to_string(),
                params: vec![],
                db_path: Some(db.key()),
                limit: 1000,
                offset: 0,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);

        // Blobs should be returned as base64-encoded strings
        use base64::Engine;
        let expected_hello = base64::engine::general_purpose::STANDARD.encode(b"Hello");
        let expected_bytes = base64::engine::general_purpose::STANDARD.encode([0, 1, 2, 3, 4]);

        assert_eq!(json["rows"][0][1], expected_hello);
        assert_eq!(json["rows"][1][1], expected_bytes);
    }

    #[tokio::test]
    async fn test_limit_parameter() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE numbers (n INTEGER);
             INSERT INTO numbers VALUES (1), (2), (3), (4), (5);",
        )
        .await;

        let result = ReadQueryTool
            .execute(ReadQueryInput {
                query: "SELECT * FROM numbers ORDER BY n".to_string(),
                params: vec![],
                db_path: Some(db.key()),
                limit: 2,
                offset: 0,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["row_count"], 2);
        assert_eq!(json["rows"][0][0], 1);
        assert_eq!(json["rows"][1][0], 2);
    }

    #[tokio::test]
    async fn test_offset_parameter() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE numbers (n INTEGER);
             INSERT INTO numbers VALUES (1), (2), (3), (4), (5);",
        )
        .await;

        let result = ReadQueryTool
            .execute(ReadQueryInput {
                query: "SELECT * FROM numbers ORDER BY n".to_string(),
                params: vec![],
                db_path: Some(db.key()),
                limit: 1000,
                offset: 2,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["row_count"], 3);
        assert_eq!(json["rows"][0][0], 3);
        assert_eq!(json["rows"][1][0], 4);
        assert_eq!(json["rows"][2][0], 5);
    }

    #[tokio::test]
    async fn test_limit_and_offset_combined() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE numbers (n INTEGER);
             INSERT INTO numbers VALUES (1), (2), (3), (4), (5);",
        )
        .await;

        let result = ReadQueryTool
            .execute(ReadQueryInput {
                query: "SELECT * FROM numbers ORDER BY n".to_string(),
                params: vec![],
                db_path: Some(db.key()),
                limit: 2,
                offset: 1,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["row_count"], 2);
        assert_eq!(json["rows"][0][0], 2);
        assert_eq!(json["rows"][1][0], 3);
    }

    #[tokio::test]
    async fn test_with_select_query() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE employees (id INTEGER, manager_id INTEGER, name TEXT);
             INSERT INTO employees VALUES (1, NULL, 'CEO');
             INSERT INTO employees VALUES (2, 1, 'VP');
             INSERT INTO employees VALUES (3, 2, 'Manager');",
        )
        .await;

        let result = ReadQueryTool
            .execute(ReadQueryInput {
                query: "WITH managers AS (SELECT * FROM employees WHERE manager_id IS NOT NULL) SELECT * FROM managers".to_string(),
                params: vec![],
                db_path: Some(db.key()),
                limit: 1000,
                offset: 0,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["row_count"], 2);
    }

    #[tokio::test]
    async fn test_pragma_query() {
        let db =
            TestDatabase::with_schema("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);")
                .await;

        let result = ReadQueryTool
            .execute(ReadQueryInput {
                query: "PRAGMA table_info(users)".to_string(),
                params: vec![],
                db_path: Some(db.key()),
                limit: 1000,
                offset: 0,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["row_count"], 2);
    }

    #[tokio::test]
    async fn test_null_in_results() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE data (id INTEGER, value TEXT);
             INSERT INTO data VALUES (1, NULL);",
        )
        .await;

        let result = ReadQueryTool
            .execute(ReadQueryInput {
                query: "SELECT * FROM data".to_string(),
                params: vec![],
                db_path: Some(db.key()),
                limit: 1000,
                offset: 0,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert!(json["rows"][0][1].is_null());
    }
}
