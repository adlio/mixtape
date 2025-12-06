//! Permission grant types.
//!
//! A grant represents stored permission to execute a tool, either for
//! any invocation or for a specific set of parameters.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Determines how long a permission grant persists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Scope {
    /// Grant lives in memory only, cleared when process exits.
    #[default]
    Session,

    /// Grant persists to storage (location determined by store).
    Persistent,
}

impl std::fmt::Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Scope::Session => write!(f, "Session"),
            Scope::Persistent => write!(f, "Persistent"),
        }
    }
}

/// A stored permission grant.
///
/// Grants allow tool execution either unconditionally (entire tool) or
/// for specific parameter combinations (exact match).
///
/// # Example
///
/// ```rust
/// use mixtape_core::permission::{Grant, Scope};
///
/// // Trust entire tool
/// let grant = Grant::tool("echo");
///
/// // Trust specific parameters (hash computed from JSON)
/// let grant = Grant::exact("database", "abc123def456");
///
/// // With persistence
/// let grant = Grant::tool("safe_tool").with_scope(Scope::Persistent);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Grant {
    /// Tool name this grant applies to.
    pub tool: String,

    /// SHA256 hash of canonical JSON parameters, or None for entire tool.
    ///
    /// When Some, only invocations with exactly matching parameters are allowed.
    /// When None, all invocations of this tool are allowed.
    pub params_hash: Option<String>,

    /// Where this grant should be stored.
    #[serde(default)]
    pub scope: Scope,

    /// When the grant was created.
    pub created_at: DateTime<Utc>,
}

impl Grant {
    /// Create a grant that trusts the entire tool (any parameters).
    pub fn tool(name: impl Into<String>) -> Self {
        Self {
            tool: name.into(),
            params_hash: None,
            scope: Scope::default(),
            created_at: Utc::now(),
        }
    }

    /// Create a grant that trusts a specific parameter combination.
    ///
    /// The hash should be computed from the canonical JSON of the parameters.
    /// Use [`hash_params`] to compute this.
    pub fn exact(name: impl Into<String>, params_hash: impl Into<String>) -> Self {
        Self {
            tool: name.into(),
            params_hash: Some(params_hash.into()),
            scope: Scope::default(),
            created_at: Utc::now(),
        }
    }

    /// Set the scope for this grant.
    pub fn with_scope(mut self, scope: Scope) -> Self {
        self.scope = scope;
        self
    }

    /// Check if this grant covers the entire tool.
    pub fn is_tool_wide(&self) -> bool {
        self.params_hash.is_none()
    }

    /// Check if this grant matches a specific params hash.
    pub fn matches(&self, params_hash: &str) -> bool {
        match &self.params_hash {
            None => true, // Tool-wide grant matches everything
            Some(h) => h == params_hash,
        }
    }
}

impl PartialEq for Grant {
    fn eq(&self, other: &Self) -> bool {
        self.tool == other.tool
            && self.params_hash == other.params_hash
            && self.scope == other.scope
    }
}

impl Eq for Grant {}

/// Compute a hash of parameters for exact-match grants.
///
/// This creates a deterministic hash from JSON parameters using canonical
/// JSON (sorted keys) to ensure consistent hashing regardless of key order.
pub fn hash_params(params: &serde_json::Value) -> String {
    use sha2::{Digest, Sha256};

    let canonical = canonicalize_json(params);
    let json = serde_json::to_string(&canonical).unwrap_or_default();
    let hash = Sha256::digest(json.as_bytes());
    format!("{:x}", hash)
}

/// Convert a JSON value to canonical form with sorted keys.
fn canonicalize_json(value: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    use std::collections::BTreeMap;

    match value {
        Value::Object(map) => {
            let sorted: BTreeMap<_, _> = map
                .iter()
                .map(|(k, v)| (k.clone(), canonicalize_json(v)))
                .collect();
            Value::Object(sorted.into_iter().collect())
        }
        Value::Array(arr) => Value::Array(arr.iter().map(canonicalize_json).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grant_tool() {
        let grant = Grant::tool("echo");
        assert_eq!(grant.tool, "echo");
        assert!(grant.params_hash.is_none());
        assert!(grant.is_tool_wide());
        assert_eq!(grant.scope, Scope::Session);
    }

    #[test]
    fn test_grant_exact() {
        let grant = Grant::exact("database", "abc123");
        assert_eq!(grant.tool, "database");
        assert_eq!(grant.params_hash, Some("abc123".to_string()));
        assert!(!grant.is_tool_wide());
    }

    #[test]
    fn test_grant_with_scope() {
        let grant = Grant::tool("test").with_scope(Scope::Persistent);
        assert_eq!(grant.scope, Scope::Persistent);
    }

    #[test]
    fn test_grant_matches() {
        let tool_grant = Grant::tool("test");
        assert!(tool_grant.matches("any_hash"));
        assert!(tool_grant.matches("other_hash"));

        let exact_grant = Grant::exact("test", "specific_hash");
        assert!(exact_grant.matches("specific_hash"));
        assert!(!exact_grant.matches("other_hash"));
    }

    #[test]
    fn test_hash_params() {
        let params = serde_json::json!({"key": "value"});
        let hash = hash_params(&params);
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64); // SHA256 hex = 64 chars

        // Same params = same hash
        let params2 = serde_json::json!({"key": "value"});
        assert_eq!(hash_params(&params2), hash);

        // Different params = different hash
        let params3 = serde_json::json!({"key": "other"});
        assert_ne!(hash_params(&params3), hash);
    }

    #[test]
    fn test_hash_params_canonical_order() {
        // Different key order should produce same hash
        let params1 = serde_json::json!({"a": 1, "b": 2, "c": 3});
        let params2 = serde_json::json!({"c": 3, "b": 2, "a": 1});
        assert_eq!(hash_params(&params1), hash_params(&params2));

        // Nested objects too
        let nested1 = serde_json::json!({"outer": {"z": 1, "a": 2}});
        let nested2 = serde_json::json!({"outer": {"a": 2, "z": 1}});
        assert_eq!(hash_params(&nested1), hash_params(&nested2));
    }

    #[test]
    fn test_scope_display() {
        assert_eq!(Scope::Session.to_string(), "Session");
        assert_eq!(Scope::Persistent.to_string(), "Persistent");
    }

    #[test]
    fn test_grant_equality() {
        let g1 = Grant::tool("test");
        let g2 = Grant::tool("test");
        assert_eq!(g1, g2); // Same despite different created_at

        let g3 = Grant::exact("test", "hash");
        assert_ne!(g1, g3); // Different params_hash
    }

    #[test]
    fn test_grant_serialization() {
        let grant = Grant::exact("tool", "hash123").with_scope(Scope::Persistent);
        let json = serde_json::to_string(&grant).unwrap();
        let parsed: Grant = serde_json::from_str(&json).unwrap();

        assert_eq!(grant.tool, parsed.tool);
        assert_eq!(grant.params_hash, parsed.params_hash);
        assert_eq!(grant.scope, parsed.scope);
    }
}
