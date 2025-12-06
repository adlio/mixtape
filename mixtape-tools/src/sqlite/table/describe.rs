//! Describe table tool

use crate::prelude::*;
use crate::sqlite::error::SqliteToolError;
use crate::sqlite::manager::with_connection;
use crate::sqlite::types::{ColumnDefinition, TableInfo, Verbosity};

/// Input for describing a table
#[derive(Debug, Deserialize, JsonSchema)]
pub struct DescribeTableInput {
    /// Table name to describe
    pub table: String,

    /// Database file path. If not specified, uses the default database.
    #[serde(default)]
    pub db_path: Option<String>,

    /// Level of detail to include (default: summary)
    #[serde(default)]
    pub verbosity: Verbosity,
}

/// Tool for getting detailed schema information about a table
///
/// Returns column definitions including names, types, constraints,
/// and optionally row count and index information.
pub struct DescribeTableTool;

impl Tool for DescribeTableTool {
    type Input = DescribeTableInput;

    fn name(&self) -> &str {
        "sqlite_describe_table"
    }

    fn description(&self) -> &str {
        "Get detailed schema information for a table including column definitions, types, and constraints."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let table_name = input.table.clone();
        let verbosity = input.verbosity;

        let info = with_connection(input.db_path, move |conn| {
            // Check if table exists and get its type
            let table_type: String = conn
                .query_row(
                    "SELECT type FROM sqlite_master WHERE name = ? AND type IN ('table', 'view')",
                    [&table_name],
                    |row| row.get(0),
                )
                .map_err(|_| SqliteToolError::TableNotFound(table_name.clone()))?;

            // Get column info using PRAGMA
            let mut stmt = conn.prepare(&format!("PRAGMA table_info('{}')", table_name))?;

            let columns: Vec<ColumnDefinition> = stmt
                .query_map([], |row| {
                    let pk: i32 = row.get(5)?;
                    let notnull: i32 = row.get(3)?;
                    let default: Option<String> = row.get(4)?;

                    Ok(ColumnDefinition {
                        name: row.get(1)?,
                        data_type: row.get(2)?,
                        nullable: notnull == 0,
                        primary_key: pk > 0,
                        default,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            // Get row count if detailed
            let row_count = if verbosity == Verbosity::Detailed && table_type == "table" {
                conn.query_row(
                    &format!("SELECT COUNT(*) FROM \"{}\"", table_name),
                    [],
                    |row| row.get(0),
                )
                .ok()
            } else {
                None
            };

            Ok(TableInfo {
                name: table_name,
                table_type,
                columns,
                row_count,
            })
        })
        .await?;

        Ok(ToolResult::Json(serde_json::to_value(info)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::test_utils::{unwrap_json, TestDatabase};

    #[tokio::test]
    async fn test_describe_table() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE users (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                email TEXT,
                age INTEGER DEFAULT 0
            );",
        )
        .await;

        let result = DescribeTableTool
            .execute(DescribeTableInput {
                table: "users".to_string(),
                db_path: Some(db.key()),
                verbosity: Verbosity::Detailed,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["name"].as_str().unwrap(), "users");
        assert_eq!(json["columns"].as_array().unwrap().len(), 4);

        // Check id column
        let id_col = &json["columns"][0];
        assert_eq!(id_col["name"].as_str().unwrap(), "id");
        assert!(id_col["primary_key"].as_bool().unwrap());

        // Check name column (NOT NULL)
        let name_col = &json["columns"][1];
        assert_eq!(name_col["name"].as_str().unwrap(), "name");
        assert!(!name_col["nullable"].as_bool().unwrap());
    }

    #[test]
    fn test_tool_metadata() {
        let tool = DescribeTableTool;
        assert_eq!(tool.name(), "sqlite_describe_table");
        assert!(!tool.description().is_empty());
    }

    #[tokio::test]
    async fn test_describe_view() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, email TEXT);
             CREATE VIEW active_users AS SELECT id, name FROM users WHERE id > 0;",
        )
        .await;

        let result = DescribeTableTool
            .execute(DescribeTableInput {
                table: "active_users".to_string(),
                db_path: Some(db.key()),
                verbosity: Verbosity::Summary,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["name"], "active_users");
        assert_eq!(json["type"], "view");
        assert_eq!(json["columns"].as_array().unwrap().len(), 2);
        assert_eq!(json["columns"][0]["name"], "id");
        assert_eq!(json["columns"][1]["name"], "name");
    }

    #[tokio::test]
    async fn test_describe_table_no_primary_key() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE logs (timestamp TEXT, message TEXT, level INTEGER);",
        )
        .await;

        let result = DescribeTableTool
            .execute(DescribeTableInput {
                table: "logs".to_string(),
                db_path: Some(db.key()),
                verbosity: Verbosity::Summary,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["columns"].as_array().unwrap().len(), 3);
        // No column should be a primary key
        for col in json["columns"].as_array().unwrap() {
            assert!(!col["primary_key"].as_bool().unwrap());
        }
    }

    #[tokio::test]
    async fn test_describe_table_composite_primary_key() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE order_items (
                order_id INTEGER,
                product_id INTEGER,
                quantity INTEGER,
                PRIMARY KEY (order_id, product_id)
            );",
        )
        .await;

        let result = DescribeTableTool
            .execute(DescribeTableInput {
                table: "order_items".to_string(),
                db_path: Some(db.key()),
                verbosity: Verbosity::Summary,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        // Both order_id and product_id should be marked as primary keys
        let cols = json["columns"].as_array().unwrap();
        let order_id = cols.iter().find(|c| c["name"] == "order_id").unwrap();
        let product_id = cols.iter().find(|c| c["name"] == "product_id").unwrap();
        let quantity = cols.iter().find(|c| c["name"] == "quantity").unwrap();

        assert!(order_id["primary_key"].as_bool().unwrap());
        assert!(product_id["primary_key"].as_bool().unwrap());
        assert!(!quantity["primary_key"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_describe_table_verbosity_summary() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
             INSERT INTO users VALUES (1, 'Alice');
             INSERT INTO users VALUES (2, 'Bob');
             INSERT INTO users VALUES (3, 'Charlie');",
        )
        .await;

        let result = DescribeTableTool
            .execute(DescribeTableInput {
                table: "users".to_string(),
                db_path: Some(db.key()),
                verbosity: Verbosity::Summary,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        // Summary should NOT include row_count
        assert!(json.get("row_count").is_none() || json["row_count"].is_null());
    }

    #[tokio::test]
    async fn test_describe_table_verbosity_detailed() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
             INSERT INTO users VALUES (1, 'Alice');
             INSERT INTO users VALUES (2, 'Bob');
             INSERT INTO users VALUES (3, 'Charlie');",
        )
        .await;

        let result = DescribeTableTool
            .execute(DescribeTableInput {
                table: "users".to_string(),
                db_path: Some(db.key()),
                verbosity: Verbosity::Detailed,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        // Detailed should include row_count
        assert_eq!(json["row_count"], 3);
    }

    #[tokio::test]
    async fn test_describe_view_no_row_count_even_detailed() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
             INSERT INTO users VALUES (1, 'Alice');
             CREATE VIEW all_users AS SELECT * FROM users;",
        )
        .await;

        let result = DescribeTableTool
            .execute(DescribeTableInput {
                table: "all_users".to_string(),
                db_path: Some(db.key()),
                verbosity: Verbosity::Detailed,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        // Views should NOT have row_count even in Detailed mode
        assert!(json.get("row_count").is_none() || json["row_count"].is_null());
    }

    #[tokio::test]
    async fn test_describe_table_not_found() {
        let db = TestDatabase::new().await;

        let result = DescribeTableTool
            .execute(DescribeTableInput {
                table: "nonexistent".to_string(),
                db_path: Some(db.key()),
                verbosity: Verbosity::Summary,
            })
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not found") || err.to_string().contains("nonexistent"));
    }

    #[tokio::test]
    async fn test_describe_table_with_default_values() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE config (
                key TEXT PRIMARY KEY,
                value TEXT DEFAULT 'empty',
                count INTEGER DEFAULT 0,
                active INTEGER DEFAULT 1
            );",
        )
        .await;

        let result = DescribeTableTool
            .execute(DescribeTableInput {
                table: "config".to_string(),
                db_path: Some(db.key()),
                verbosity: Verbosity::Summary,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        let cols = json["columns"].as_array().unwrap();
        let value_col = cols.iter().find(|c| c["name"] == "value").unwrap();
        let count_col = cols.iter().find(|c| c["name"] == "count").unwrap();

        assert_eq!(value_col["default"], "'empty'");
        assert_eq!(count_col["default"], "0");
    }

    #[tokio::test]
    async fn test_describe_table_nullable_columns() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE users (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                email TEXT,
                phone TEXT NOT NULL
            );",
        )
        .await;

        let result = DescribeTableTool
            .execute(DescribeTableInput {
                table: "users".to_string(),
                db_path: Some(db.key()),
                verbosity: Verbosity::Summary,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        let cols = json["columns"].as_array().unwrap();
        let name_col = cols.iter().find(|c| c["name"] == "name").unwrap();
        let email_col = cols.iter().find(|c| c["name"] == "email").unwrap();
        let phone_col = cols.iter().find(|c| c["name"] == "phone").unwrap();

        assert!(!name_col["nullable"].as_bool().unwrap());
        assert!(email_col["nullable"].as_bool().unwrap());
        assert!(!phone_col["nullable"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_describe_table_data_types() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE mixed_types (
                id INTEGER,
                name TEXT,
                price REAL,
                data BLOB,
                anything
            );",
        )
        .await;

        let result = DescribeTableTool
            .execute(DescribeTableInput {
                table: "mixed_types".to_string(),
                db_path: Some(db.key()),
                verbosity: Verbosity::Summary,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        let cols = json["columns"].as_array().unwrap();
        assert_eq!(
            cols.iter().find(|c| c["name"] == "id").unwrap()["type"],
            "INTEGER"
        );
        assert_eq!(
            cols.iter().find(|c| c["name"] == "name").unwrap()["type"],
            "TEXT"
        );
        assert_eq!(
            cols.iter().find(|c| c["name"] == "price").unwrap()["type"],
            "REAL"
        );
        assert_eq!(
            cols.iter().find(|c| c["name"] == "data").unwrap()["type"],
            "BLOB"
        );
        // Column with no type should have empty string
        assert_eq!(
            cols.iter().find(|c| c["name"] == "anything").unwrap()["type"],
            ""
        );
    }
}
