//! List tables tool

use crate::prelude::*;
use crate::sqlite::manager::with_connection;

/// Input for listing tables
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListTablesInput {
    /// Database file path. If not specified, uses the default database.
    #[serde(default)]
    pub db_path: Option<String>,
}

/// Table entry information
#[derive(Debug, Serialize, JsonSchema)]
struct TableEntry {
    name: String,
    #[serde(rename = "type")]
    table_type: String,
}

/// Tool for listing all tables and views in a database
///
/// Returns a list of all tables and views, excluding:
/// - SQLite internal tables (`sqlite_*`)
/// - System tables managed by tools (`_*`)
pub struct ListTablesTool;

impl Tool for ListTablesTool {
    type Input = ListTablesInput;

    fn name(&self) -> &str {
        "sqlite_list_tables"
    }

    fn description(&self) -> &str {
        "List all tables and views in a SQLite database. Excludes SQLite internal tables (sqlite_*) \
         and system tables managed by tools (_*). Returns the name and type of each table/view."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let tables = with_connection(input.db_path, |conn| {
            let mut stmt = conn.prepare(
                "SELECT name, type FROM sqlite_master
                 WHERE type IN ('table', 'view')
                 AND name NOT LIKE 'sqlite_%'
                 AND name NOT LIKE '\\_%' ESCAPE '\\'
                 ORDER BY type, name",
            )?;

            let tables: Vec<TableEntry> = stmt
                .query_map([], |row| {
                    Ok(TableEntry {
                        name: row.get(0)?,
                        table_type: row.get(1)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(tables)
        })
        .await?;

        let count = tables.len();
        Ok(ToolResult::Json(serde_json::json!({
            "tables": tables,
            "count": count
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::test_utils::{unwrap_json, TestDatabase};

    #[tokio::test]
    async fn test_list_tables() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
             CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER);
             CREATE VIEW user_posts AS SELECT * FROM users JOIN posts ON users.id = posts.user_id;",
        )
        .await;

        let result = ListTablesTool
            .execute(ListTablesInput {
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["count"].as_i64().unwrap(), 3);
    }

    #[test]
    fn test_tool_metadata() {
        let tool = ListTablesTool;
        assert_eq!(tool.name(), "sqlite_list_tables");
        assert!(!tool.description().is_empty());
    }

    #[tokio::test]
    async fn test_excludes_system_tables() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE users (id INTEGER PRIMARY KEY);
             CREATE TABLE _system_table (id INTEGER PRIMARY KEY);",
        )
        .await;

        let result = ListTablesTool
            .execute(ListTablesInput {
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        // Should only show 'users', not '_system_table'
        assert_eq!(json["count"].as_i64().unwrap(), 1);
        assert_eq!(json["tables"][0]["name"], "users");
    }
}
