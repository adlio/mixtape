//! Bulk insert tool

use crate::prelude::*;
use crate::sqlite::error::SqliteToolError;
use crate::sqlite::manager::with_connection;
use crate::sqlite::types::json_to_sql;

/// Input for bulk insert operation
#[derive(Debug, Deserialize, JsonSchema)]
pub struct BulkInsertInput {
    /// Table name to insert into
    pub table: String,

    /// Array of records to insert. Each record is an object with column names as keys.
    pub data: Vec<serde_json::Map<String, serde_json::Value>>,

    /// Number of records to insert per batch (default: 1000)
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,

    /// Database file path. If not specified, uses the default database.
    #[serde(default)]
    pub db_path: Option<String>,
}

fn default_batch_size() -> usize {
    1000
}

/// Bulk insert result
#[derive(Debug, Serialize, JsonSchema)]
struct BulkInsertResult {
    status: String,
    total_inserted: usize,
    batches: usize,
}

/// Tool for efficiently inserting multiple records (DESTRUCTIVE)
///
/// Inserts records in batches using transactions for efficiency.
/// Each record is an object with column names as keys.
pub struct BulkInsertTool;

impl Tool for BulkInsertTool {
    type Input = BulkInsertInput;

    fn name(&self) -> &str {
        "sqlite_bulk_insert"
    }

    fn description(&self) -> &str {
        "Efficiently insert multiple records into a table using batched transactions. Each record is an object with column names as keys."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        if input.data.is_empty() {
            return Ok(ToolResult::Json(serde_json::json!({
                "status": "success",
                "total_inserted": 0,
                "batches": 0,
                "message": "No data to insert"
            })));
        }

        let table = input.table;
        let data = input.data;
        let batch_size = input.batch_size.max(1);

        let result = with_connection(input.db_path, move |conn| {
            // Get column names from first record
            let columns: Vec<&String> = data[0].keys().collect();
            if columns.is_empty() {
                return Err(SqliteToolError::InvalidQuery(
                    "Records must have at least one column".to_string(),
                ));
            }

            // Build INSERT statement
            let column_names = columns
                .iter()
                .map(|c| format!("\"{}\"", c))
                .collect::<Vec<_>>()
                .join(", ");
            let placeholders = columns.iter().map(|_| "?").collect::<Vec<_>>().join(", ");

            let sql = format!(
                "INSERT INTO \"{}\" ({}) VALUES ({})",
                table, column_names, placeholders
            );

            let mut total_inserted = 0;
            let mut batches = 0;

            // Process in batches
            for chunk in data.chunks(batch_size) {
                conn.execute("BEGIN TRANSACTION", [])?;

                for record in chunk {
                    // Collect values in column order
                    let values: Vec<Box<dyn rusqlite::ToSql>> = columns
                        .iter()
                        .map(|col| {
                            let value = record.get(*col).unwrap_or(&serde_json::Value::Null);
                            json_to_sql(value)
                        })
                        .collect();

                    let params: Vec<&dyn rusqlite::ToSql> =
                        values.iter().map(|b| b.as_ref()).collect();

                    conn.execute(&sql, params.as_slice())?;
                    total_inserted += 1;
                }

                conn.execute("COMMIT", [])?;
                batches += 1;
            }

            Ok(BulkInsertResult {
                status: "success".to_string(),
                total_inserted,
                batches,
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

    #[tokio::test]
    async fn test_bulk_insert() {
        let db =
            TestDatabase::with_schema("CREATE TABLE users (id INTEGER, name TEXT, age INTEGER);")
                .await;

        // Bulk insert
        let tool = BulkInsertTool;
        let mut data = Vec::new();
        for i in 0..100 {
            let mut record = serde_json::Map::new();
            record.insert("id".to_string(), serde_json::json!(i));
            record.insert("name".to_string(), serde_json::json!(format!("User {}", i)));
            record.insert("age".to_string(), serde_json::json!(20 + (i % 50)));
            data.push(record);
        }

        let input = BulkInsertInput {
            table: "users".to_string(),
            data,
            batch_size: 25,
            db_path: Some(db.key()),
        };

        let result = tool.execute(input).await.unwrap();
        let json = unwrap_json(result);

        assert_eq!(json["total_inserted"].as_i64().unwrap(), 100);
        assert_eq!(json["batches"].as_i64().unwrap(), 4);

        // Verify data
        assert_eq!(db.count("users"), 100);
    }

    #[tokio::test]
    async fn test_bulk_insert_empty() {
        let db = TestDatabase::new().await;

        let tool = BulkInsertTool;
        let input = BulkInsertInput {
            table: "users".to_string(),
            data: vec![],
            batch_size: 1000,
            db_path: Some(db.key()),
        };

        let result = tool.execute(input).await.unwrap();
        let json = unwrap_json(result);

        assert_eq!(json["total_inserted"].as_i64().unwrap(), 0);
    }

    #[test]
    fn test_tool_metadata() {
        let tool = BulkInsertTool;
        assert_eq!(tool.name(), "sqlite_bulk_insert");
        assert!(!tool.description().is_empty());
    }
}
