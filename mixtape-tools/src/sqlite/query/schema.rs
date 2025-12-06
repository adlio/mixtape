//! Schema query tool

use crate::prelude::*;
use crate::sqlite::error::SqliteToolError;
use crate::sqlite::manager::with_connection;
use crate::sqlite::types::json_to_sql;

/// Input for schema query execution
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SchemaQueryInput {
    /// SQL DDL query to execute (CREATE, ALTER, DROP)
    pub query: String,

    /// Query parameters for prepared statements
    #[serde(default)]
    pub params: Vec<serde_json::Value>,

    /// Database file path. If not specified, uses the default database.
    #[serde(default)]
    pub db_path: Option<String>,
}

/// Tool for executing DDL (schema) queries (DESTRUCTIVE)
///
/// Executes CREATE, ALTER, and DROP statements.
/// Use this for schema modifications rather than data modifications.
pub struct SchemaQueryTool;

impl SchemaQueryTool {
    /// Validates that a query is a DDL operation
    fn is_schema_query(sql: &str) -> bool {
        let normalized = sql.trim().to_uppercase();
        let ddl_prefixes = ["CREATE", "ALTER", "DROP"];
        ddl_prefixes
            .iter()
            .any(|prefix| normalized.starts_with(prefix))
    }
}

impl Tool for SchemaQueryTool {
    type Input = SchemaQueryInput;

    fn name(&self) -> &str {
        "sqlite_schema_query"
    }

    fn description(&self) -> &str {
        "Execute a DDL (Data Definition Language) SQL query (CREATE, ALTER, DROP). Use for schema modifications."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        // Validate query is a DDL operation
        if !Self::is_schema_query(&input.query) {
            return Err(SqliteToolError::InvalidQuery(
                "Only CREATE, ALTER, and DROP queries are allowed. Use sqlite_write_query for INSERT/UPDATE/DELETE.".to_string()
            ).into());
        }

        let query = input.query.clone();
        let params = input.params;

        with_connection(input.db_path, move |conn| {
            // Convert params to rusqlite values
            let params_ref: Vec<Box<dyn rusqlite::ToSql>> =
                params.iter().map(|v| json_to_sql(v)).collect();

            let params_slice: Vec<&dyn rusqlite::ToSql> =
                params_ref.iter().map(|b| b.as_ref()).collect();

            conn.execute(&query, params_slice.as_slice())?;

            Ok(())
        })
        .await?;

        let response = serde_json::json!({
            "status": "success",
            "message": format!("Schema query executed successfully")
        });
        Ok(ToolResult::Json(response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::test_utils::TestDatabase;

    #[tokio::test]
    async fn test_schema_query_create() {
        let db = TestDatabase::new().await;

        let tool = SchemaQueryTool;
        let input = SchemaQueryInput {
            query: "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)".to_string(),
            params: vec![],
            db_path: Some(db.key()),
        };

        let result = tool.execute(input).await;
        assert!(result.is_ok());

        // Verify table was created
        let rows =
            db.query("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='users'");
        assert_eq!(rows[0][0], 1);
    }

    #[tokio::test]
    async fn test_schema_query_alter() {
        let db = TestDatabase::with_schema("CREATE TABLE users (id INTEGER);").await;

        let tool = SchemaQueryTool;
        let input = SchemaQueryInput {
            query: "ALTER TABLE users ADD COLUMN name TEXT".to_string(),
            params: vec![],
            db_path: Some(db.key()),
        };

        let result = tool.execute(input).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_reject_select() {
        let db = TestDatabase::new().await;

        let tool = SchemaQueryTool;
        let input = SchemaQueryInput {
            query: "SELECT * FROM users".to_string(),
            params: vec![],
            db_path: Some(db.key()),
        };

        let result = tool.execute(input).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_is_schema_query() {
        assert!(SchemaQueryTool::is_schema_query(
            "CREATE TABLE users (id INT)"
        ));
        assert!(SchemaQueryTool::is_schema_query(
            "ALTER TABLE users ADD COLUMN name TEXT"
        ));
        assert!(SchemaQueryTool::is_schema_query("DROP TABLE users"));
        assert!(SchemaQueryTool::is_schema_query(
            "CREATE INDEX idx ON users(id)"
        ));

        assert!(!SchemaQueryTool::is_schema_query("SELECT * FROM users"));
        assert!(!SchemaQueryTool::is_schema_query(
            "INSERT INTO users VALUES (1)"
        ));
        assert!(!SchemaQueryTool::is_schema_query(
            "UPDATE users SET name = 'x'"
        ));
    }

    #[test]
    fn test_tool_metadata() {
        let tool = SchemaQueryTool;
        assert_eq!(tool.name(), "sqlite_schema_query");
        assert!(!tool.description().is_empty());
    }
}
