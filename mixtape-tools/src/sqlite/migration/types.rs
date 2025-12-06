//! Types for migration management

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A database schema migration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Migration {
    /// Unique version identifier (timestamp-based)
    pub version: String,

    /// Human-readable description of the migration
    pub name: String,

    /// The SQL DDL to execute
    pub sql: String,

    /// When the migration was applied (None = pending)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applied_at: Option<String>,

    /// SHA256 checksum of the SQL for integrity verification
    pub checksum: String,
}

/// Filter for listing migrations by their application status
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MigrationStatusFilter {
    /// Show all migrations (no filtering)
    #[default]
    All,
    /// Only pending (not yet applied) migrations
    Pending,
    /// Only applied migrations
    Applied,
}
