//! Distributed state management for clustered deployment
//!
//! Provides consistent state storage across cluster nodes using
//! various backends (etcd, Redis, in-memory with Raft).

use crate::utils::errors::{McpError, McpResult};
use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::error;

/// State backend trait
#[async_trait]
pub trait StateBackend: Send + Sync {
    /// Get value by key
    async fn get(&self, key: &str) -> McpResult<Option<Vec<u8>>>;
    
    /// Set value for key
    async fn set(&self, key: &str, value: Vec<u8>) -> McpResult<()>;
    
    /// Delete key
    async fn delete(&self, key: &str) -> McpResult<()>;
    
    /// Watch for changes to a key
    async fn watch(&self, key: &str) -> McpResult<tokio::sync::mpsc::Receiver<StateEvent>>;
    
    /// Compare-and-swap operation
    async fn cas(&self, key: &str, expected: Option<Vec<u8>>, new: Vec<u8>) -> McpResult<bool>;
    
    /// List keys with prefix
    async fn list(&self, prefix: &str) -> McpResult<Vec<String>>;
}

/// State events
#[derive(Debug, Clone)]
pub enum StateEvent {
    /// Key was created
    Created { key: String, value: Vec<u8> },
    /// Key was updated
    Updated { key: String, value: Vec<u8> },
    /// Key was deleted
    Deleted { key: String },
}

/// Distributed state manager
pub struct DistributedState {
    backend: Arc<dyn StateBackend>,
    local_cache: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl DistributedState {
    /// Create a new distributed state manager
    pub fn new(backend: Arc<dyn StateBackend>) -> Self {
        Self {
            backend,
            local_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get typed value
    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> McpResult<Option<T>> {
        // Check local cache first
        {
            let cache = self.local_cache.read().await;
            if let Some(data) = cache.get(key) {
                return serde_json::from_slice(data)
                    .map(Some)
                    .map_err(|e| McpError::Serialization(e));
            }
        }

        // Fetch from backend
        match self.backend.get(key).await? {
            Some(data) => {
                // Update cache
                let mut cache = self.local_cache.write().await;
                cache.insert(key.to_string(), data.clone());
                
                serde_json::from_slice(&data)
                    .map(Some)
                    .map_err(|e| McpError::Serialization(e))
            }
            None => Ok(None),
        }
    }

    /// Set typed value
    pub async fn set<T: Serialize>(&self, key: &str, value: &T) -> McpResult<()> {
        let data = serde_json::to_vec(value)?;
        
        // Update backend
        self.backend.set(key, data.clone()).await?;
        
        // Update cache
        let mut cache = self.local_cache.write().await;
        cache.insert(key.to_string(), data);
        
        Ok(())
    }

    /// Delete value
    pub async fn delete(&self, key: &str) -> McpResult<()> {
        self.backend.delete(key).await?;
        
        let mut cache = self.local_cache.write().await;
        cache.remove(key);
        
        Ok(())
    }

    /// Compare-and-swap for optimistic concurrency
    pub async fn cas<T: Serialize + DeserializeOwned>(
        &self,
        key: &str,
        expected: Option<&T>,
        new: &T,
    ) -> McpResult<bool> {
        let expected_bytes = expected.map(|v| serde_json::to_vec(v).unwrap());
        let new_bytes = serde_json::to_vec(new)?;
        
        let success = self.backend.cas(key, expected_bytes, new_bytes.clone()).await?;
        
        if success {
            let mut cache = self.local_cache.write().await;
            cache.insert(key.to_string(), new_bytes);
        }
        
        Ok(success)
    }

    /// List keys with prefix
    pub async fn list(&self, prefix: &str) -> McpResult<Vec<String>> {
        self.backend.list(prefix).await
    }

    /// Invalidate cache entry
    pub async fn invalidate(&self, key: &str) {
        let mut cache = self.local_cache.write().await;
        cache.remove(key);
    }

    /// Clear local cache
    pub async fn clear_cache(&self) {
        let mut cache = self.local_cache.write().await;
        cache.clear();
    }

    /// Start cache synchronization
    pub async fn start_sync(&self, prefix: &str) {
        let backend = self.backend.clone();
        let cache = self.local_cache.clone();
        let prefix = prefix.to_string();

        tokio::spawn(async move {
            let mut rx = match backend.watch(&prefix).await {
                Ok(rx) => rx,
                Err(e) => {
                    error!("Failed to start state sync: {}", e);
                    return;
                }
            };

            while let Some(event) = rx.recv().await {
                match event {
                    StateEvent::Created { key, value } | StateEvent::Updated { key, value } => {
                        let mut c = cache.write().await;
                        c.insert(key, value);
                    }
                    StateEvent::Deleted { key } => {
                        let mut c = cache.write().await;
                        c.remove(&key);
                    }
                }
            }
        });
    }
}

/// In-memory state backend (for single-node deployments)
pub struct InMemoryBackend {
    data: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    watchers: Arc<RwLock<HashMap<String, Vec<tokio::sync::mpsc::Sender<StateEvent>>>>>,
}

impl InMemoryBackend {
    /// Create a new in-memory backend
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
            watchers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Notify watchers of state change
    async fn notify_watchers(&self, key: &str, event: StateEvent) {
        let watchers = self.watchers.read().await;
        
        for (prefix, senders) in watchers.iter() {
            if key.starts_with(prefix) {
                for sender in senders {
                    let _ = sender.send(event.clone()).await;
                }
            }
        }
    }
}

impl Default for InMemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StateBackend for InMemoryBackend {
    async fn get(&self, key: &str) -> McpResult<Option<Vec<u8>>> {
        let data = self.data.read().await;
        Ok(data.get(key).cloned())
    }

    async fn set(&self, key: &str, value: Vec<u8>) -> McpResult<()> {
        let existed = {
            let data = self.data.read().await;
            data.contains_key(key)
        };

        {
            let mut data = self.data.write().await;
            data.insert(key.to_string(), value.clone());
        }

        let event = if existed {
            StateEvent::Updated {
                key: key.to_string(),
                value,
            }
        } else {
            StateEvent::Created {
                key: key.to_string(),
                value,
            }
        };

        self.notify_watchers(key, event).await;
        Ok(())
    }

    async fn delete(&self, key: &str) -> McpResult<()> {
        {
            let mut data = self.data.write().await;
            data.remove(key);
        }

        self.notify_watchers(
            key,
            StateEvent::Deleted {
                key: key.to_string(),
            },
        )
        .await;

        Ok(())
    }

    async fn watch(&self, key: &str) -> McpResult<tokio::sync::mpsc::Receiver<StateEvent>> {
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        
        let mut watchers = self.watchers.write().await;
        watchers
            .entry(key.to_string())
            .or_insert_with(Vec::new)
            .push(tx);

        Ok(rx)
    }

    async fn cas(&self, key: &str, expected: Option<Vec<u8>>, new: Vec<u8>) -> McpResult<bool> {
        let mut data = self.data.write().await;
        
        let current = data.get(key).cloned();
        
        if current == expected {
            data.insert(key.to_string(), new);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn list(&self, prefix: &str) -> McpResult<Vec<String>> {
        let data = self.data.read().await;
        Ok(data
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::time::Duration;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestData {
        name: String,
        value: i32,
    }

    #[tokio::test]
    async fn test_in_memory_backend_basic() {
        let backend = InMemoryBackend::new();

        // Test set and get
        backend.set("key1", b"value1".to_vec()).await.unwrap();
        let value = backend.get("key1").await.unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));

        // Test delete
        backend.delete("key1").await.unwrap();
        let value = backend.get("key1").await.unwrap();
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn test_distributed_state_typed() {
        let backend = Arc::new(InMemoryBackend::new());
        let state = DistributedState::new(backend);

        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        // Set typed value
        state.set("test-key", &data).await.unwrap();

        // Get typed value
        let retrieved: Option<TestData> = state.get("test-key").await.unwrap();
        assert_eq!(retrieved, Some(data));
    }

    #[tokio::test]
    async fn test_cas_operation() {
        let backend = Arc::new(InMemoryBackend::new());
        let state = DistributedState::new(backend);

        let data1 = TestData {
            name: "first".to_string(),
            value: 1,
        };
        let data2 = TestData {
            name: "second".to_string(),
            value: 2,
        };

        // Initial set
        state.set("cas-key", &data1).await.unwrap();

        // CAS with correct expected value
        let success = state.cas("cas-key", Some(&data1), &data2).await.unwrap();
        assert!(success);

        // Verify update
        let retrieved: Option<TestData> = state.get("cas-key").await.unwrap();
        assert_eq!(retrieved, Some(data2));

        // CAS with incorrect expected value should fail
        let success = state.cas("cas-key", Some(&data1), &data1).await.unwrap();
        assert!(!success);
    }

    #[tokio::test]
    async fn test_list_keys() {
        let backend = InMemoryBackend::new();

        backend.set("prefix/key1", b"1".to_vec()).await.unwrap();
        backend.set("prefix/key2", b"2".to_vec()).await.unwrap();
        backend.set("other/key3", b"3".to_vec()).await.unwrap();

        let keys = backend.list("prefix/").await.unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"prefix/key1".to_string()));
        assert!(keys.contains(&"prefix/key2".to_string()));
    }

    #[tokio::test]
    async fn test_watch() {
        let backend = InMemoryBackend::new();
        let mut rx = backend.watch("watch/").await.unwrap();

        // Create a value
        backend.set("watch/key1", b"value1".to_vec()).await.unwrap();

        // Should receive event
        let event = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await;
        assert!(event.is_ok());
        assert!(event.unwrap().is_some());
    }
}
