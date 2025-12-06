//! Rollback transaction tool

use crate::prelude::*;
use crate::sqlite::manager::with_connection;

/// Input for rolling back a transaction
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RollbackTransactionInput {
    /// Database file path. If not specified, uses the default database.
    #[serde(default)]
    pub db_path: Option<String>,
}

/// Tool for rolling back a database transaction
///
/// Reverts all changes made during the current transaction.
/// The transaction must have been started with begin_transaction.
pub struct RollbackTransactionTool;

impl Tool for RollbackTransactionTool {
    type Input = RollbackTransactionInput;

    fn name(&self) -> &str {
        "sqlite_rollback_transaction"
    }

    fn description(&self) -> &str {
        "Rollback the current transaction, reverting all changes made since the transaction began."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        with_connection(input.db_path, |conn| {
            conn.execute("ROLLBACK", [])?;
            Ok(())
        })
        .await?;

        let response = serde_json::json!({
            "status": "success",
            "message": "Transaction rolled back successfully"
        });
        Ok(ToolResult::Json(response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::test_utils::TestDatabase;
    use crate::sqlite::transaction::BeginTransactionTool;

    #[tokio::test]
    async fn test_rollback_transaction() {
        let db = TestDatabase::with_schema("CREATE TABLE test (id INTEGER);").await;

        // Begin transaction
        let begin_tool = BeginTransactionTool;
        let begin_input = crate::sqlite::transaction::begin::BeginTransactionInput {
            db_path: Some(db.key()),
            transaction_type: crate::sqlite::transaction::begin::TransactionType::Deferred,
        };
        begin_tool.execute(begin_input).await.unwrap();

        // Insert data (will be rolled back)
        db.execute("INSERT INTO test VALUES (1)");
        db.execute("INSERT INTO test VALUES (2)");

        // Rollback
        let tool = RollbackTransactionTool;
        let input = RollbackTransactionInput {
            db_path: Some(db.key()),
        };

        let result = tool.execute(input).await;
        assert!(result.is_ok());

        // Verify data was rolled back
        assert_eq!(db.count("test"), 0);
    }

    #[test]
    fn test_tool_metadata() {
        let tool = RollbackTransactionTool;
        assert_eq!(tool.name(), "sqlite_rollback_transaction");
        assert!(!tool.description().is_empty());
    }
}
