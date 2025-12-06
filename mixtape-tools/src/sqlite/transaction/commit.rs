//! Commit transaction tool

use crate::prelude::*;
use crate::sqlite::manager::with_connection;

/// Input for committing a transaction
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CommitTransactionInput {
    /// Database file path. If not specified, uses the default database.
    #[serde(default)]
    pub db_path: Option<String>,
}

/// Tool for committing a database transaction
///
/// Commits all changes made during the current transaction.
/// The transaction must have been started with begin_transaction.
pub struct CommitTransactionTool;

impl Tool for CommitTransactionTool {
    type Input = CommitTransactionInput;

    fn name(&self) -> &str {
        "sqlite_commit_transaction"
    }

    fn description(&self) -> &str {
        "Commit the current transaction, making all changes permanent."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        with_connection(input.db_path, |conn| {
            conn.execute("COMMIT", [])?;
            Ok(())
        })
        .await?;

        let response = serde_json::json!({
            "status": "success",
            "message": "Transaction committed successfully"
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
    async fn test_commit_transaction() {
        let db = TestDatabase::with_schema("CREATE TABLE test (id INTEGER);").await;

        // Begin transaction
        let begin_tool = BeginTransactionTool;
        let begin_input = crate::sqlite::transaction::begin::BeginTransactionInput {
            db_path: Some(db.key()),
            transaction_type: crate::sqlite::transaction::begin::TransactionType::Deferred,
        };
        begin_tool.execute(begin_input).await.unwrap();

        // Insert data
        db.execute("INSERT INTO test VALUES (1)");

        // Commit
        let tool = CommitTransactionTool;
        let input = CommitTransactionInput {
            db_path: Some(db.key()),
        };

        let result = tool.execute(input).await;
        assert!(result.is_ok());

        // Verify data persisted
        assert_eq!(db.count("test"), 1);
    }

    #[test]
    fn test_tool_metadata() {
        let tool = CommitTransactionTool;
        assert_eq!(tool.name(), "sqlite_commit_transaction");
        assert!(!tool.description().is_empty());
    }
}
