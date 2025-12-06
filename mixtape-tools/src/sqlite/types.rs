//! Shared types for SQLite tools

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Column definition for table creation and schema introspection
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ColumnDefinition {
    /// Column name
    pub name: String,

    /// SQLite data type (TEXT, INTEGER, REAL, BLOB, NULL)
    #[serde(rename = "type")]
    pub data_type: String,

    /// Whether the column allows NULL values
    #[serde(default)]
    pub nullable: bool,

    /// Whether this column is (part of) the primary key
    #[serde(default)]
    pub primary_key: bool,

    /// Default value expression (if any)
    #[serde(default)]
    pub default: Option<String>,
}

/// Information about a table
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TableInfo {
    /// Table name
    pub name: String,

    /// Table type (table, view, etc.)
    #[serde(rename = "type")]
    pub table_type: String,

    /// Column definitions
    pub columns: Vec<ColumnDefinition>,

    /// Number of rows (approximate for large tables)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row_count: Option<i64>,
}

/// Result of a query execution
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryResult {
    /// Column names
    pub columns: Vec<String>,

    /// Row data as arrays of JSON values
    pub rows: Vec<Vec<serde_json::Value>>,

    /// Number of rows returned
    pub row_count: usize,

    /// Number of rows affected (for write operations)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows_affected: Option<usize>,
}

/// Database metadata and statistics
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DatabaseInfo {
    /// Database file path
    pub path: String,

    /// File size in bytes
    pub size_bytes: u64,

    /// Number of tables
    pub table_count: usize,

    /// Number of indexes
    pub index_count: usize,

    /// Number of views
    pub view_count: usize,

    /// Number of triggers
    pub trigger_count: usize,

    /// SQLite version
    pub sqlite_version: String,

    /// Page size in bytes
    pub page_size: i64,

    /// Page count
    pub page_count: i64,

    /// Whether the database is in WAL mode
    pub wal_mode: bool,
}

/// Export format for schema operations
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SchemaFormat {
    /// SQL statements (CREATE TABLE, etc.)
    #[default]
    Sql,
    /// JSON representation
    Json,
}

/// Verbosity level for describe operations
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Verbosity {
    /// Brief summary
    #[default]
    Summary,
    /// Full details
    Detailed,
}

/// Convert a JSON value to a rusqlite-compatible SQL parameter.
///
/// This handles the conversion from serde_json values to types that rusqlite
/// can bind to prepared statement parameters.
pub fn json_to_sql(value: &serde_json::Value) -> Box<dyn rusqlite::ToSql> {
    match value {
        serde_json::Value::Null => Box::new(Option::<i64>::None),
        serde_json::Value::Bool(b) => Box::new(*b as i64),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Box::new(i)
            } else if let Some(f) = n.as_f64() {
                Box::new(f)
            } else {
                Box::new(n.to_string())
            }
        }
        serde_json::Value::String(s) => Box::new(s.clone()),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => Box::new(value.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// Helper to test json_to_sql by inserting a value and reading it back
    fn roundtrip_json_value(json: serde_json::Value) -> rusqlite::types::Value {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("CREATE TABLE test (val)", []).unwrap();

        let param = json_to_sql(&json);
        conn.execute("INSERT INTO test VALUES (?1)", [param.as_ref()])
            .unwrap();

        conn.query_row("SELECT val FROM test", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn test_json_to_sql_null() {
        let result = roundtrip_json_value(serde_json::Value::Null);
        assert_eq!(result, rusqlite::types::Value::Null);
    }

    #[test]
    fn test_json_to_sql_bool_true() {
        let result = roundtrip_json_value(serde_json::json!(true));
        assert_eq!(result, rusqlite::types::Value::Integer(1));
    }

    #[test]
    fn test_json_to_sql_bool_false() {
        let result = roundtrip_json_value(serde_json::json!(false));
        assert_eq!(result, rusqlite::types::Value::Integer(0));
    }

    #[test]
    fn test_json_to_sql_integer() {
        let result = roundtrip_json_value(serde_json::json!(42));
        assert_eq!(result, rusqlite::types::Value::Integer(42));
    }

    #[test]
    fn test_json_to_sql_negative_integer() {
        let result = roundtrip_json_value(serde_json::json!(-100));
        assert_eq!(result, rusqlite::types::Value::Integer(-100));
    }

    #[test]
    fn test_json_to_sql_large_integer() {
        let large = i64::MAX;
        let result = roundtrip_json_value(serde_json::json!(large));
        assert_eq!(result, rusqlite::types::Value::Integer(large));
    }

    #[test]
    fn test_json_to_sql_float() {
        let result = roundtrip_json_value(serde_json::json!(1.234));
        match result {
            rusqlite::types::Value::Real(f) => assert!((f - 1.234).abs() < 0.001),
            _ => panic!("Expected Real, got {:?}", result),
        }
    }

    #[test]
    fn test_json_to_sql_float_negative() {
        let result = roundtrip_json_value(serde_json::json!(-2.5));
        match result {
            rusqlite::types::Value::Real(f) => assert!((f - (-2.5)).abs() < 0.001),
            _ => panic!("Expected Real, got {:?}", result),
        }
    }

    #[test]
    fn test_json_to_sql_string() {
        let result = roundtrip_json_value(serde_json::json!("hello world"));
        assert_eq!(
            result,
            rusqlite::types::Value::Text("hello world".to_string())
        );
    }

    #[test]
    fn test_json_to_sql_empty_string() {
        let result = roundtrip_json_value(serde_json::json!(""));
        assert_eq!(result, rusqlite::types::Value::Text("".to_string()));
    }

    #[test]
    fn test_json_to_sql_unicode_string() {
        let result = roundtrip_json_value(serde_json::json!("こんにちは 🎉"));
        assert_eq!(
            result,
            rusqlite::types::Value::Text("こんにちは 🎉".to_string())
        );
    }

    #[test]
    fn test_json_to_sql_array() {
        let result = roundtrip_json_value(serde_json::json!([1, 2, 3]));
        assert_eq!(result, rusqlite::types::Value::Text("[1,2,3]".to_string()));
    }

    #[test]
    fn test_json_to_sql_nested_array() {
        let result = roundtrip_json_value(serde_json::json!([[1, 2], [3, 4]]));
        assert_eq!(
            result,
            rusqlite::types::Value::Text("[[1,2],[3,4]]".to_string())
        );
    }

    #[test]
    fn test_json_to_sql_object() {
        let result = roundtrip_json_value(serde_json::json!({"key": "value"}));
        assert_eq!(
            result,
            rusqlite::types::Value::Text("{\"key\":\"value\"}".to_string())
        );
    }

    #[test]
    fn test_json_to_sql_complex_object() {
        let json = serde_json::json!({
            "name": "test",
            "values": [1, 2, 3],
            "nested": {"a": 1}
        });
        let result = roundtrip_json_value(json);
        match result {
            rusqlite::types::Value::Text(s) => {
                // Parse back to verify it's valid JSON
                let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
                assert_eq!(parsed["name"], "test");
                assert_eq!(parsed["values"][0], 1);
            }
            _ => panic!("Expected Text, got {:?}", result),
        }
    }

    #[test]
    fn test_json_to_sql_empty_array() {
        let result = roundtrip_json_value(serde_json::json!([]));
        assert_eq!(result, rusqlite::types::Value::Text("[]".to_string()));
    }

    #[test]
    fn test_json_to_sql_empty_object() {
        let result = roundtrip_json_value(serde_json::json!({}));
        assert_eq!(result, rusqlite::types::Value::Text("{}".to_string()));
    }
}
