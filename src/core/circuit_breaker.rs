//! Circuit breaker pattern for resilient server communication
//!
//! Prevents cascade failures by temporarily disabling requests to failing servers.

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{info, warn};

/// Circuit breaker states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Normal operation - requests pass through
    Closed,
    /// Failure threshold reached - requests are rejected
    Open,
    /// Testing if service has recovered
    HalfOpen,
}

impl std::fmt::Display for CircuitState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CircuitState::Closed => write!(f, "closed"),
            CircuitState::Open => write!(f, "open"),
            CircuitState::HalfOpen => write!(f, "half-open"),
        }
    }
}

/// Circuit breaker configuration
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening circuit
    pub failure_threshold: u32,
    /// Duration to wait before trying again (half-open)
    pub reset_timeout: Duration,
    /// Success threshold in half-open state to close circuit
    pub success_threshold: u32,
    /// Request timeout
    pub request_timeout: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            reset_timeout: Duration::from_secs(30),
            success_threshold: 2,
            request_timeout: Duration::from_secs(30),
        }
    }
}

/// Circuit breaker for a single server
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    state: Arc<RwLock<CircuitState>>,
    failure_count: AtomicU64,
    success_count: AtomicU64,
    last_failure_time: Arc<RwLock<Option<Instant>>>,
    name: String,
}

impl CircuitBreaker {
    /// Create a new circuit breaker
    pub fn new(name: impl Into<String>, config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(CircuitState::Closed)),
            failure_count: AtomicU64::new(0),
            success_count: AtomicU64::new(0),
            last_failure_time: Arc::new(RwLock::new(None)),
            name: name.into(),
        }
    }

    /// Get current state
    pub async fn state(&self) -> CircuitState {
        *self.state.read().await
    }

    /// Check if request should be allowed
    pub async fn allow_request(&self) -> bool {
        let state = *self.state.read().await;

        match state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if reset timeout has elapsed
                let last_failure = *self.last_failure_time.read().await;
                if let Some(time) = last_failure {
                    if time.elapsed() >= self.config.reset_timeout {
                        // Transition to half-open
                        let mut state_write = self.state.write().await;
                        if *state_write == CircuitState::Open {
                            *state_write = CircuitState::HalfOpen;
                            self.success_count.store(0, Ordering::SeqCst);
                            info!(
                                "Circuit breaker '{}' transitioned to half-open",
                                self.name
                            );
                        }
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true,
        }
    }

    /// Record a successful request
    pub async fn record_success(&self) {
        let state = *self.state.read().await;

        match state {
            CircuitState::Closed => {
                self.failure_count.store(0, Ordering::SeqCst);
            }
            CircuitState::HalfOpen => {
                let successes = self.success_count.fetch_add(1, Ordering::SeqCst) + 1;
                if successes >= self.config.success_threshold as u64 {
                    let mut state_write = self.state.write().await;
                    *state_write = CircuitState::Closed;
                    self.failure_count.store(0, Ordering::SeqCst);
                    self.success_count.store(0, Ordering::SeqCst);
                    info!("Circuit breaker '{}' closed after recovery", self.name);
                }
            }
            CircuitState::Open => {
                // Shouldn't happen, but reset just in case
                self.failure_count.store(0, Ordering::SeqCst);
            }
        }
    }

    /// Record a failed request
    pub async fn record_failure(&self) {
        let state = *self.state.read().await;

        match state {
            CircuitState::Closed => {
                let failures = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;
                *self.last_failure_time.write().await = Some(Instant::now());

                if failures >= self.config.failure_threshold as u64 {
                    let mut state_write = self.state.write().await;
                    *state_write = CircuitState::Open;
                    warn!(
                        "Circuit breaker '{}' opened after {} failures",
                        self.name, failures
                    );
                }
            }
            CircuitState::HalfOpen => {
                // Failure in half-open state goes back to open
                let mut state_write = self.state.write().await;
                *state_write = CircuitState::Open;
                *self.last_failure_time.write().await = Some(Instant::now());
                warn!(
                    "Circuit breaker '{}' re-opened after failure in half-open state",
                    self.name
                );
            }
            CircuitState::Open => {
                *self.last_failure_time.write().await = Some(Instant::now());
            }
        }
    }

    /// Execute a function with circuit breaker protection
    pub async fn call<F, Fut, T, E>(&self, f: F) -> Result<T, CircuitBreakerError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, E>>,
        E: std::fmt::Display,
    {
        if !self.allow_request().await {
            return Err(CircuitBreakerError::Open);
        }

        let timeout = sleep(self.config.request_timeout);
        tokio::pin!(timeout);

        let result = tokio::select! {
            res = f() => res.map_err(|e| CircuitBreakerError::Inner(e.to_string())),
            _ = &mut timeout => Err(CircuitBreakerError::Timeout),
        };

        match &result {
            Ok(_) => self.record_success().await,
            Err(_) => self.record_failure().await,
        }

        result
    }

    /// Get statistics
    pub fn stats(&self) -> CircuitBreakerStats {
        CircuitBreakerStats {
            state: self.state.blocking_read().clone(),
            failure_count: self.failure_count.load(Ordering::SeqCst),
            success_count: self.success_count.load(Ordering::SeqCst),
        }
    }

    /// Reset the circuit breaker (for manual intervention)
    pub async fn reset(&self) {
        let mut state = self.state.write().await;
        *state = CircuitState::Closed;
        self.failure_count.store(0, Ordering::SeqCst);
        self.success_count.store(0, Ordering::SeqCst);
        *self.last_failure_time.write().await = None;
        info!("Circuit breaker '{}' manually reset", self.name);
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new("default", CircuitBreakerConfig::default())
    }
}

/// Circuit breaker statistics
#[derive(Debug, Clone)]
pub struct CircuitBreakerStats {
    pub state: CircuitState,
    pub failure_count: u64,
    pub success_count: u64,
}

/// Circuit breaker errors
#[derive(Debug, Clone)]
pub enum CircuitBreakerError {
    /// Circuit is open - requests are being rejected
    Open,
    /// Request timed out
    Timeout,
    /// Inner function error
    Inner(String),
}

impl std::fmt::Display for CircuitBreakerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CircuitBreakerError::Open => write!(f, "Circuit breaker is open"),
            CircuitBreakerError::Timeout => write!(f, "Request timed out"),
            CircuitBreakerError::Inner(msg) => write!(f, "Request failed: {}", msg),
        }
    }
}

impl std::error::Error for CircuitBreakerError {}

/// Manager for multiple circuit breakers (one per server)
pub struct CircuitBreakerManager {
    breakers: Arc<RwLock<std::collections::HashMap<String, Arc<CircuitBreaker>>>>,
    config: CircuitBreakerConfig,
}

impl CircuitBreakerManager {
    /// Create a new circuit breaker manager
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            breakers: Arc::new(RwLock::new(std::collections::HashMap::new())),
            config,
        }
    }

    /// Get or create a circuit breaker for a server
    pub async fn get_breaker(&self, server_name: &str) -> Arc<CircuitBreaker> {
        let read = self.breakers.read().await;
        if let Some(breaker) = read.get(server_name) {
            return breaker.clone();
        }
        drop(read);

        let mut write = self.breakers.write().await;
        write
            .entry(server_name.to_string())
            .or_insert_with(|| Arc::new(CircuitBreaker::new(server_name, self.config.clone())))
            .clone()
    }

    /// Get all breaker stats
    pub async fn get_all_stats(&self) -> std::collections::HashMap<String, CircuitBreakerStats> {
        let read = self.breakers.read().await;
        let mut stats = std::collections::HashMap::new();
        for (name, breaker) in read.iter() {
            stats.insert(name.clone(), breaker.stats());
        }
        stats
    }

    /// Reset a specific breaker
    pub async fn reset_breaker(&self, server_name: &str) {
        let read = self.breakers.read().await;
        if let Some(breaker) = read.get(server_name) {
            breaker.reset().await;
        }
    }

    /// Reset all breakers
    pub async fn reset_all(&self) {
        let read = self.breakers.read().await;
        for (_, breaker) in read.iter() {
            breaker.reset().await;
        }
    }
}

impl Default for CircuitBreakerManager {
    fn default() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_circuit_breaker_starts_closed() {
        let cb = CircuitBreaker::new("test", CircuitBreakerConfig::default());
        assert_eq!(cb.state().await, CircuitState::Closed);
        assert!(cb.allow_request().await);
    }

    #[tokio::test]
    async fn test_circuit_opens_after_failures() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            reset_timeout: Duration::from_secs(60),
            ..Default::default()
        };
        let cb = CircuitBreaker::new("test", config);

        // Record failures
        for _ in 0..3 {
            cb.record_failure().await;
        }

        assert_eq!(cb.state().await, CircuitState::Open);
        assert!(!cb.allow_request().await);
    }

    #[tokio::test]
    async fn test_circuit_half_open_after_timeout() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            reset_timeout: Duration::from_millis(10),
            ..Default::default()
        };
        let cb = CircuitBreaker::new("test", config);

        cb.record_failure().await;
        assert_eq!(cb.state().await, CircuitState::Open);

        // Wait for timeout
        sleep(Duration::from_millis(20)).await;

        // Should transition to half-open on next check
        assert!(cb.allow_request().await);
        assert_eq!(cb.state().await, CircuitState::HalfOpen);
    }

    #[tokio::test]
    async fn test_circuit_closes_after_successes() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            reset_timeout: Duration::from_millis(10),
            success_threshold: 2,
            ..Default::default()
        };
        let cb = CircuitBreaker::new("test", config);

        // Open the circuit
        cb.record_failure().await;
        assert_eq!(cb.state().await, CircuitState::Open);

        // Wait and transition to half-open
        sleep(Duration::from_millis(20)).await;
        assert!(cb.allow_request().await);

        // Record successes to close
        cb.record_success().await;
        cb.record_success().await;

        assert_eq!(cb.state().await, CircuitState::Closed);
    }

    #[test]
    fn test_circuit_breaker_config_default() {
        let config = CircuitBreakerConfig::default();
        assert_eq!(config.failure_threshold, 5);
        assert_eq!(config.reset_timeout, Duration::from_secs(30));
        assert_eq!(config.success_threshold, 2);
    }

    #[test]
    fn test_circuit_state_display() {
        assert_eq!(format!("{}", CircuitState::Closed), "closed");
        assert_eq!(format!("{}", CircuitState::Open), "open");
        assert_eq!(format!("{}", CircuitState::HalfOpen), "half-open");
    }
}
