//! Begin transaction tool

use crate::prelude::*;
use crate::sqlite::manager::with_connection;

/// Input for beginning a transaction
#[derive(Debug, Deserialize, JsonSchema)]
pub struct BeginTransactionInput {
    /// Database file path. If not specified, uses the default database.
    #[serde(default)]
    pub db_path: Option<String>,

    /// Transaction type (default: DEFERRED)
    #[serde(default)]
    pub transaction_type: TransactionType,
}

/// SQLite transaction types
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum TransactionType {
    /// Deferred transaction (default) - locks acquired on first access
    #[default]
    Deferred,
    /// Immediate transaction - acquires reserved lock immediately
    Immediate,
    /// Exclusive transaction - acquires exclusive lock immediately
    Exclusive,
}

impl std::fmt::Display for TransactionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransactionType::Deferred => write!(f, "DEFERRED"),
            TransactionType::Immediate => write!(f, "IMMEDIATE"),
            TransactionType::Exclusive => write!(f, "EXCLUSIVE"),
        }
    }
}

/// Tool for beginning a database transaction
///
/// Starts a new transaction. All subsequent operations will be part of
/// this transaction until committed or rolled back.
pub struct BeginTransactionTool;

impl Tool for BeginTransactionTool {
    type Input = BeginTransactionInput;

    fn name(&self) -> &str {
        "sqlite_begin_transaction"
    }

    fn description(&self) -> &str {
        "Begin a new database transaction. All subsequent operations will be part of this transaction until committed or rolled back."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let tx_type = input.transaction_type;

        with_connection(input.db_path, move |conn| {
            let sql = format!("BEGIN {} TRANSACTION", tx_type);
            conn.execute(&sql, [])?;
            Ok(())
        })
        .await?;

        let response = serde_json::json!({
            "status": "success",
            "message": format!("Transaction started ({})", tx_type)
        });
        Ok(ToolResult::Json(response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::test_utils::TestDatabase;

    #[tokio::test]
    async fn test_begin_transaction() {
        let db = TestDatabase::new().await;

        let tool = BeginTransactionTool;
        let input = BeginTransactionInput {
            db_path: Some(db.key()),
            transaction_type: TransactionType::Deferred,
        };

        let result = tool.execute(input).await;
        assert!(result.is_ok());

        // Rollback to clean up
        db.execute("ROLLBACK");
    }

    #[test]
    fn test_tool_metadata() {
        let tool = BeginTransactionTool;
        assert_eq!(tool.name(), "sqlite_begin_transaction");
        assert!(!tool.description().is_empty());
    }
}
