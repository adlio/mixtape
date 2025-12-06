//! Grant storage trait and implementations.

use super::grant::Grant;
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;

/// Errors that can occur in grant store operations.
#[derive(Debug, thiserror::Error)]
pub enum GrantStoreError {
    /// Failed to read grants from storage.
    #[error("Failed to read grants: {0}")]
    Read(String),

    /// Failed to write grants to storage.
    #[error("Failed to write grants: {0}")]
    Write(String),

    /// IO error during storage operations.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Trait for grant storage implementations.
///
/// Stores persist grants for later retrieval. Implementations handle
/// the mechanics of saving and loading grants.
#[async_trait]
pub trait GrantStore: Send + Sync {
    /// Save a grant to storage.
    async fn save(&self, grant: Grant) -> Result<(), GrantStoreError>;

    /// Load all grants for a specific tool.
    async fn load(&self, tool: &str) -> Result<Vec<Grant>, GrantStoreError>;

    /// Load all grants across all tools.
    async fn load_all(&self) -> Result<Vec<Grant>, GrantStoreError>;

    /// Remove a specific grant.
    ///
    /// Returns `true` if a grant was removed, `false` if not found.
    async fn delete(&self, tool: &str, params_hash: Option<&str>) -> Result<bool, GrantStoreError>;

    /// Clear all grants.
    async fn clear(&self) -> Result<(), GrantStoreError>;
}

/// In-memory grant store.
///
/// Grants are cleared when the process exits. This is the default store
/// used by the agent.
pub struct MemoryGrantStore {
    grants: RwLock<HashMap<String, Vec<Grant>>>,
}

impl MemoryGrantStore {
    /// Create a new empty memory store.
    pub fn new() -> Self {
        Self {
            grants: RwLock::new(HashMap::new()),
        }
    }

    /// Grant permission to use a tool (any parameters).
    ///
    /// Convenience method for `save(Grant::tool(name))`.
    pub async fn grant_tool(&self, tool: &str) -> Result<(), GrantStoreError> {
        self.save(Grant::tool(tool)).await
    }
}

impl Default for MemoryGrantStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl GrantStore for MemoryGrantStore {
    async fn save(&self, grant: Grant) -> Result<(), GrantStoreError> {
        let mut grants = self.grants.write().expect("RwLock poisoned");
        grants.entry(grant.tool.clone()).or_default().push(grant);
        Ok(())
    }

    async fn load(&self, tool: &str) -> Result<Vec<Grant>, GrantStoreError> {
        Ok(self
            .grants
            .read()
            .expect("RwLock poisoned")
            .get(tool)
            .cloned()
            .unwrap_or_default())
    }

    async fn load_all(&self) -> Result<Vec<Grant>, GrantStoreError> {
        Ok(self
            .grants
            .read()
            .expect("RwLock poisoned")
            .values()
            .flatten()
            .cloned()
            .collect())
    }

    async fn delete(&self, tool: &str, params_hash: Option<&str>) -> Result<bool, GrantStoreError> {
        let mut grants = self.grants.write().expect("RwLock poisoned");

        if let Some(tool_grants) = grants.get_mut(tool) {
            let original_len = tool_grants.len();
            tool_grants.retain(|g| g.params_hash.as_deref() != params_hash);
            Ok(tool_grants.len() < original_len)
        } else {
            Ok(false)
        }
    }

    async fn clear(&self) -> Result<(), GrantStoreError> {
        let mut grants = self.grants.write().expect("RwLock poisoned");
        grants.clear();
        Ok(())
    }
}

/// File-based grant store.
///
/// Grants are persisted to a JSON file. The file is created automatically
/// when the first grant is stored.
pub struct FileGrantStore {
    path: PathBuf,
    cache: RwLock<Option<HashMap<String, Vec<Grant>>>>,
}

impl FileGrantStore {
    /// Create a new file-based store at the given path.
    ///
    /// The file does not need to exist - it will be created when
    /// the first grant is saved.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            cache: RwLock::new(None),
        }
    }

    /// Load grants from file into cache if not already loaded.
    fn ensure_loaded(&self) -> Result<(), GrantStoreError> {
        let mut cache = self.cache.write().expect("RwLock poisoned");
        if cache.is_some() {
            return Ok(());
        }

        let grants = if self.path.exists() {
            let contents = std::fs::read_to_string(&self.path)?;
            if contents.trim().is_empty() {
                HashMap::new()
            } else {
                serde_json::from_str(&contents)?
            }
        } else {
            HashMap::new()
        };

        *cache = Some(grants);
        Ok(())
    }

    /// Write cache to file.
    fn flush(&self) -> Result<(), GrantStoreError> {
        let cache = self.cache.read().expect("RwLock poisoned");
        if let Some(ref grants) = *cache {
            // Create parent directories if needed
            if let Some(parent) = self.path.parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent)?;
                }
            }
            let json = serde_json::to_string_pretty(grants)?;
            std::fs::write(&self.path, json)?;
        }
        Ok(())
    }
}

#[async_trait]
impl GrantStore for FileGrantStore {
    async fn save(&self, grant: Grant) -> Result<(), GrantStoreError> {
        self.ensure_loaded()?;
        {
            let mut cache = self.cache.write().expect("RwLock poisoned");
            if let Some(ref mut grants) = *cache {
                grants.entry(grant.tool.clone()).or_default().push(grant);
            }
        }
        self.flush()
    }

    async fn load(&self, tool: &str) -> Result<Vec<Grant>, GrantStoreError> {
        self.ensure_loaded()?;
        let cache = self.cache.read().expect("RwLock poisoned");
        Ok(cache
            .as_ref()
            .and_then(|g| g.get(tool).cloned())
            .unwrap_or_default())
    }

    async fn load_all(&self) -> Result<Vec<Grant>, GrantStoreError> {
        self.ensure_loaded()?;
        let cache = self.cache.read().expect("RwLock poisoned");
        Ok(cache
            .as_ref()
            .map(|g| g.values().flatten().cloned().collect())
            .unwrap_or_default())
    }

    async fn delete(&self, tool: &str, params_hash: Option<&str>) -> Result<bool, GrantStoreError> {
        self.ensure_loaded()?;
        let removed = {
            let mut cache = self.cache.write().expect("RwLock poisoned");
            if let Some(ref mut grants) = *cache {
                if let Some(tool_grants) = grants.get_mut(tool) {
                    let original_len = tool_grants.len();
                    tool_grants.retain(|g| g.params_hash.as_deref() != params_hash);
                    tool_grants.len() < original_len
                } else {
                    false
                }
            } else {
                false
            }
        };
        if removed {
            self.flush()?;
        }
        Ok(removed)
    }

    async fn clear(&self) -> Result<(), GrantStoreError> {
        self.ensure_loaded()?;
        {
            let mut cache = self.cache.write().expect("RwLock poisoned");
            if let Some(ref mut grants) = *cache {
                grants.clear();
            }
        }
        self.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_store_basic() {
        let store = MemoryGrantStore::new();

        // Initially empty
        assert!(store.load("test").await.unwrap().is_empty());
        assert!(store.load_all().await.unwrap().is_empty());

        // Save a grant
        store.save(Grant::tool("test")).await.unwrap();

        // Should be retrievable
        let grants = store.load("test").await.unwrap();
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].tool, "test");

        // load_all should include it
        assert_eq!(store.load_all().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_memory_store_multiple_grants() {
        let store = MemoryGrantStore::new();

        store.save(Grant::tool("test")).await.unwrap();
        store.save(Grant::exact("test", "hash1")).await.unwrap();
        store.save(Grant::exact("test", "hash2")).await.unwrap();

        let grants = store.load("test").await.unwrap();
        assert_eq!(grants.len(), 3);
    }

    #[tokio::test]
    async fn test_memory_store_multiple_tools() {
        let store = MemoryGrantStore::new();

        store.save(Grant::tool("tool_a")).await.unwrap();
        store.save(Grant::tool("tool_b")).await.unwrap();

        assert_eq!(store.load("tool_a").await.unwrap().len(), 1);
        assert_eq!(store.load("tool_b").await.unwrap().len(), 1);
        assert_eq!(store.load_all().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_memory_store_delete() {
        let store = MemoryGrantStore::new();

        store.save(Grant::tool("test")).await.unwrap();
        store.save(Grant::exact("test", "hash1")).await.unwrap();

        assert_eq!(store.load("test").await.unwrap().len(), 2);

        // Delete the exact grant
        let removed = store.delete("test", Some("hash1")).await.unwrap();
        assert!(removed);

        let grants = store.load("test").await.unwrap();
        assert_eq!(grants.len(), 1);
        assert!(grants[0].is_tool_wide());

        // Delete the tool-wide grant
        let removed = store.delete("test", None).await.unwrap();
        assert!(removed);
        assert!(store.load("test").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_memory_store_delete_nonexistent() {
        let store = MemoryGrantStore::new();

        let removed = store.delete("test", None).await.unwrap();
        assert!(!removed);
    }

    #[tokio::test]
    async fn test_memory_store_clear() {
        let store = MemoryGrantStore::new();

        store.save(Grant::tool("a")).await.unwrap();
        store.save(Grant::tool("b")).await.unwrap();

        assert_eq!(store.load_all().await.unwrap().len(), 2);

        store.clear().await.unwrap();
        assert!(store.load_all().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_file_store_basic() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("grants.json");

        let store = FileGrantStore::new(&path);

        // Initially empty (file doesn't exist)
        assert!(store.load("test").await.unwrap().is_empty());

        // Save a grant
        store.save(Grant::tool("test")).await.unwrap();

        // File should exist now
        assert!(path.exists());

        // Should be retrievable
        let grants = store.load("test").await.unwrap();
        assert_eq!(grants.len(), 1);

        // Create new store instance to verify persistence
        let store2 = FileGrantStore::new(&path);
        let grants = store2.load("test").await.unwrap();
        assert_eq!(grants.len(), 1);
    }

    #[tokio::test]
    async fn test_file_store_creates_parent_dirs() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("nested/dir/grants.json");

        let store = FileGrantStore::new(&path);
        store.save(Grant::tool("test")).await.unwrap();

        assert!(path.exists());
    }

    #[tokio::test]
    async fn test_file_store_handles_empty_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("grants.json");

        // Create empty file
        std::fs::write(&path, "").unwrap();

        let store = FileGrantStore::new(&path);
        assert!(store.load("test").await.unwrap().is_empty());

        // Can still save
        store.save(Grant::tool("test")).await.unwrap();
        assert_eq!(store.load("test").await.unwrap().len(), 1);
    }
}
