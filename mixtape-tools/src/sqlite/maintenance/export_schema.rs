//! Export schema tool

use crate::prelude::*;
use crate::sqlite::manager::with_connection;
use crate::sqlite::types::{ColumnDefinition, SchemaFormat, TableInfo};

/// Input for exporting schema
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExportSchemaInput {
    /// Database file path. If not specified, uses the default database.
    #[serde(default)]
    pub db_path: Option<String>,

    /// Export format (sql or json)
    #[serde(default)]
    pub format: SchemaFormat,

    /// Specific tables to export. If empty, exports all tables.
    #[serde(default)]
    pub tables: Vec<String>,
}

/// Tool for exporting database schema (SAFE)
///
/// Exports the database schema in SQL or JSON format.
/// Can export all tables or specific tables.
pub struct ExportSchemaTool;

impl Tool for ExportSchemaTool {
    type Input = ExportSchemaInput;

    fn name(&self) -> &str {
        "sqlite_export_schema"
    }

    fn description(&self) -> &str {
        "Export the database schema in SQL or JSON format. Can export all tables or specific tables."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let format = input.format;
        let filter_tables = input.tables;

        let result = with_connection(input.db_path, move |conn| {
            // Get all tables/views
            let mut stmt = conn.prepare(
                "SELECT name, type, sql FROM sqlite_master
                 WHERE type IN ('table', 'view')
                 AND name NOT LIKE 'sqlite_%'
                 ORDER BY type, name",
            )?;

            let objects: Vec<(String, String, Option<String>)> = stmt
                .query_map([], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })?
                .filter_map(|r| r.ok())
                .filter(|(name, _, _)| {
                    filter_tables.is_empty() || filter_tables.contains(name)
                })
                .collect();

            match format {
                SchemaFormat::Sql => {
                    // Export as SQL statements
                    let mut sql = String::new();
                    for (name, obj_type, create_sql) in &objects {
                        if let Some(s) = create_sql {
                            sql.push_str(&format!("-- {} '{}'\n", obj_type, name));
                            sql.push_str(s);
                            sql.push_str(";\n\n");
                        }
                    }

                    // Also export indexes
                    let mut idx_stmt = conn.prepare(
                        "SELECT name, sql FROM sqlite_master WHERE type = 'index' AND sql IS NOT NULL"
                    )?;
                    let indexes: Vec<(String, String)> = idx_stmt
                        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                        .filter_map(|r| r.ok())
                        .collect();

                    if !indexes.is_empty() {
                        sql.push_str("-- Indexes\n");
                        for (name, create_sql) in indexes {
                            sql.push_str(&format!("-- index '{}'\n", name));
                            sql.push_str(&create_sql);
                            sql.push_str(";\n\n");
                        }
                    }

                    Ok(serde_json::json!({
                        "format": "sql",
                        "schema": sql,
                        "table_count": objects.len()
                    }))
                }
                SchemaFormat::Json => {
                    // Export as structured JSON
                    let mut tables = Vec::new();

                    for (name, table_type, _) in &objects {
                        // Get column info
                        let mut col_stmt =
                            conn.prepare(&format!("PRAGMA table_info('{}')", name))?;

                        let columns: Vec<ColumnDefinition> = col_stmt
                            .query_map([], |row| {
                                let pk: i32 = row.get(5)?;
                                let notnull: i32 = row.get(3)?;
                                Ok(ColumnDefinition {
                                    name: row.get(1)?,
                                    data_type: row.get(2)?,
                                    nullable: notnull == 0,
                                    primary_key: pk > 0,
                                    default: row.get(4)?,
                                })
                            })?
                            .filter_map(|r| r.ok())
                            .collect();

                        tables.push(TableInfo {
                            name: name.clone(),
                            table_type: table_type.clone(),
                            columns,
                            row_count: None,
                        });
                    }

                    Ok(serde_json::json!({
                        "format": "json",
                        "tables": tables,
                        "table_count": tables.len()
                    }))
                }
            }
        })
        .await?;

        Ok(ToolResult::Json(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::test_utils::{unwrap_json, TestDatabase};

    #[tokio::test]
    async fn test_export_schema_sql() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
             CREATE TABLE posts (id INTEGER, user_id INTEGER);",
        )
        .await;

        let tool = ExportSchemaTool;
        let input = ExportSchemaInput {
            db_path: Some(db.key()),
            format: SchemaFormat::Sql,
            tables: vec![],
        };

        let result = tool.execute(input).await.unwrap();
        let json = unwrap_json(result);

        assert_eq!(json["format"].as_str().unwrap(), "sql");
        assert_eq!(json["table_count"].as_i64().unwrap(), 2);
        let schema = json["schema"].as_str().unwrap();
        assert!(schema.contains("CREATE TABLE"));
    }

    #[tokio::test]
    async fn test_export_schema_json() {
        let db =
            TestDatabase::with_schema("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);")
                .await;

        let tool = ExportSchemaTool;
        let input = ExportSchemaInput {
            db_path: Some(db.key()),
            format: SchemaFormat::Json,
            tables: vec![],
        };

        let result = tool.execute(input).await.unwrap();
        let json = unwrap_json(result);

        assert_eq!(json["format"].as_str().unwrap(), "json");
        let tables = json["tables"].as_array().unwrap();
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0]["name"].as_str().unwrap(), "users");
    }

    #[test]
    fn test_tool_metadata() {
        let tool = ExportSchemaTool;
        assert_eq!(tool.name(), "sqlite_export_schema");
        assert!(!tool.description().is_empty());
    }
}
