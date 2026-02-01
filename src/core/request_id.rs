//! Unique request ID generation
//!
//! Provides thread-safe generation of unique request IDs for JSON-RPC requests.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use uuid::Uuid;

/// Request ID generator
pub struct RequestIdGenerator {
    /// Atomic counter for sequential IDs
    counter: AtomicU64,
    /// Prefix for IDs (e.g., node ID in distributed setup)
    prefix: Option<String>,
    /// Whether to use UUIDs instead of sequential IDs
    use_uuid: bool,
}

impl RequestIdGenerator {
    /// Create a new request ID generator with sequential IDs
    pub fn new() -> Self {
        Self {
            counter: AtomicU64::new(1),
            prefix: None,
            use_uuid: false,
        }
    }

    /// Create a new request ID generator with UUIDs
    pub fn with_uuid() -> Self {
        Self {
            counter: AtomicU64::new(1),
            prefix: None,
            use_uuid: true,
        }
    }

    /// Create a new generator with a prefix
    pub fn with_prefix(prefix: impl Into<String>) -> Self {
        Self {
            counter: AtomicU64::new(1),
            prefix: Some(prefix.into()),
            use_uuid: false,
        }
    }

    /// Generate the next request ID
    pub fn next_id(&self) -> crate::core::protocol::RequestId {
        if self.use_uuid {
            self.next_uuid_id()
        } else {
            self.next_numeric_id()
        }
    }

    /// Generate a numeric ID
    fn next_numeric_id(&self) -> crate::core::protocol::RequestId {
        let num = self.counter.fetch_add(1, Ordering::SeqCst);
        
        match &self.prefix {
            Some(prefix) => {
                crate::core::protocol::RequestId::String(format!("{}-{}", prefix, num))
            }
            None => crate::core::protocol::RequestId::Number(num as i64),
        }
    }

    /// Generate a UUID-based ID
    fn next_uuid_id(&self) -> crate::core::protocol::RequestId {
        let uuid = Uuid::new_v4().to_string();
        
        match &self.prefix {
            Some(prefix) => {
                crate::core::protocol::RequestId::String(format!("{}-{}", prefix, uuid))
            }
            None => crate::core::protocol::RequestId::String(uuid),
        }
    }

    /// Get current counter value (for debugging)
    pub fn current_value(&self) -> u64 {
        self.counter.load(Ordering::SeqCst)
    }

    /// Reset the counter (use with caution)
    pub fn reset(&self) {
        self.counter.store(1, Ordering::SeqCst);
    }
}

impl Default for RequestIdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread-safe shared request ID generator
#[derive(Clone)]
pub struct SharedRequestIdGenerator {
    inner: Arc<RequestIdGenerator>,
}

impl SharedRequestIdGenerator {
    /// Create a new shared generator
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RequestIdGenerator::new()),
        }
    }

    /// Create a new shared generator with UUIDs
    pub fn with_uuid() -> Self {
        Self {
            inner: Arc::new(RequestIdGenerator::with_uuid()),
        }
    }

    /// Create a new shared generator with prefix
    pub fn with_prefix(prefix: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(RequestIdGenerator::with_prefix(prefix)),
        }
    }

    /// Generate the next request ID
    pub fn next_id(&self) -> crate::core::protocol::RequestId {
        self.inner.next_id()
    }

    /// Get current counter value
    pub fn current_value(&self) -> u64 {
        self.inner.current_value()
    }
}

impl Default for SharedRequestIdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sequential_id_generation() {
        let generator = RequestIdGenerator::new();
        
        let id1 = generator.next_id();
        let id2 = generator.next_id();
        let id3 = generator.next_id();

        // Check they are sequential numbers
        if let crate::core::protocol::RequestId::Number(n1) = id1 {
            assert_eq!(n1, 1);
        } else {
            panic!("Expected numeric ID");
        }

        if let crate::core::protocol::RequestId::Number(n2) = id2 {
            assert_eq!(n2, 2);
        } else {
            panic!("Expected numeric ID");
        }

        if let crate::core::protocol::RequestId::Number(n3) = id3 {
            assert_eq!(n3, 3);
        } else {
            panic!("Expected numeric ID");
        }
    }

    #[test]
    fn test_uuid_id_generation() {
        let generator = RequestIdGenerator::with_uuid();
        
        let id1 = generator.next_id();
        let id2 = generator.next_id();

        // Check they are strings (UUIDs)
        assert!(matches!(id1, crate::core::protocol::RequestId::String(_)));
        assert!(matches!(id2, crate::core::protocol::RequestId::String(_)));

        // Check they are different
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_prefixed_id_generation() {
        let generator = RequestIdGenerator::with_prefix("node1");
        
        let id1 = generator.next_id();
        let id2 = generator.next_id();

        if let crate::core::protocol::RequestId::String(s1) = &id1 {
            assert!(s1.starts_with("node1-1"));
        } else {
            panic!("Expected string ID");
        }

        if let crate::core::protocol::RequestId::String(s2) = &id2 {
            assert!(s2.starts_with("node1-2"));
        } else {
            panic!("Expected string ID");
        }
    }

    #[test]
    fn test_shared_generator() {
        let generator1 = SharedRequestIdGenerator::new();
        let generator2 = generator1.clone();

        let id1 = generator1.next_id();
        let id2 = generator2.next_id();
        let id3 = generator1.next_id();

        // All should be sequential
        if let crate::core::protocol::RequestId::Number(n1) = id1 {
            assert_eq!(n1, 1);
        } else {
            panic!("Expected numeric ID");
        }

        if let crate::core::protocol::RequestId::Number(n2) = id2 {
            assert_eq!(n2, 2);
        } else {
            panic!("Expected numeric ID");
        }

        if let crate::core::protocol::RequestId::Number(n3) = id3 {
            assert_eq!(n3, 3);
        } else {
            panic!("Expected numeric ID");
        }
    }

    #[test]
    fn test_reset() {
        let generator = RequestIdGenerator::new();
        
        let _ = generator.next_id();
        let _ = generator.next_id();
        
        assert_eq!(generator.current_value(), 3);
        
        generator.reset();
        
        assert_eq!(generator.current_value(), 1);
        
        let id = generator.next_id();
        if let crate::core::protocol::RequestId::Number(n) = id {
            assert_eq!(n, 1);
        } else {
            panic!("Expected numeric ID");
        }
    }
}
