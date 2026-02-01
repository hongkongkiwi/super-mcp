//! Metrics and telemetry integration
//!
//! Provides Prometheus-compatible metrics and OpenTelemetry support.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::debug;

/// Metrics collector
pub struct MetricsCollector {
    /// Total requests
    requests_total: AtomicU64,
    /// Requests by status code
    requests_by_status: dashmap::DashMap<u16, AtomicU64>,
    /// Active connections
    active_connections: AtomicU64,
    /// Request duration histogram (simplified)
    request_duration_ms: AtomicU64,
    /// Total request count for duration calculation
    request_count: AtomicU64,
    /// Server start time
    start_time: Instant,
    /// Cache hits
    cache_hits: AtomicU64,
    /// Cache misses
    cache_misses: AtomicU64,
}

impl MetricsCollector {
    /// Create a new metrics collector
    pub fn new() -> Self {
        Self {
            requests_total: AtomicU64::new(0),
            requests_by_status: dashmap::DashMap::new(),
            active_connections: AtomicU64::new(0),
            request_duration_ms: AtomicU64::new(0),
            request_count: AtomicU64::new(0),
            start_time: Instant::now(),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
        }
    }

    /// Record a request
    pub fn record_request(&self, status_code: u16, duration_ms: u64) {
        self.requests_total.fetch_add(1, Ordering::SeqCst);
        
        self.requests_by_status
            .entry(status_code)
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::SeqCst);

        self.request_duration_ms.fetch_add(duration_ms, Ordering::SeqCst);
        self.request_count.fetch_add(1, Ordering::SeqCst);

        debug!(
            "Request recorded: status={}, duration={}ms",
            status_code, duration_ms
        );
    }

    /// Increment active connections
    pub fn connection_opened(&self) {
        self.active_connections.fetch_add(1, Ordering::SeqCst);
    }

    /// Decrement active connections
    pub fn connection_closed(&self) {
        self.active_connections.fetch_sub(1, Ordering::SeqCst);
    }

    /// Record cache hit
    pub fn record_cache_hit(&self) {
        self.cache_hits.fetch_add(1, Ordering::SeqCst);
    }

    /// Record cache miss
    pub fn record_cache_miss(&self) {
        self.cache_misses.fetch_add(1, Ordering::SeqCst);
    }

    /// Get total requests
    pub fn total_requests(&self) -> u64 {
        self.requests_total.load(Ordering::SeqCst)
    }

    /// Get active connections
    pub fn active_connections(&self) -> u64 {
        self.active_connections.load(Ordering::SeqCst)
    }

    /// Get average request duration
    pub fn average_request_duration_ms(&self) -> f64 {
        let total_duration = self.request_duration_ms.load(Ordering::SeqCst);
        let count = self.request_count.load(Ordering::SeqCst);
        
        if count == 0 {
            0.0
        } else {
            total_duration as f64 / count as f64
        }
    }

    /// Get cache hit rate
    pub fn cache_hit_rate(&self) -> f64 {
        let hits = self.cache_hits.load(Ordering::SeqCst);
        let misses = self.cache_misses.load(Ordering::SeqCst);
        let total = hits + misses;
        
        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }

    /// Get uptime in seconds
    pub fn uptime_seconds(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    /// Export metrics in Prometheus format
    pub fn export_prometheus(&self) -> String {
        let mut output = String::new();

        // Help and type annotations
        output.push_str("# HELP mcp_requests_total Total number of requests\n");
        output.push_str("# TYPE mcp_requests_total counter\n");
        output.push_str(&format!("mcp_requests_total {}\n", self.total_requests()));

        output.push_str("# HELP mcp_active_connections Number of active connections\n");
        output.push_str("# TYPE mcp_active_connections gauge\n");
        output.push_str(&format!("mcp_active_connections {}\n", self.active_connections()));

        output.push_str("# HELP mcp_request_duration_ms Average request duration in milliseconds\n");
        output.push_str("# TYPE mcp_request_duration_ms gauge\n");
        output.push_str(&format!("mcp_request_duration_ms {:.2}\n", self.average_request_duration_ms()));

        output.push_str("# HELP mcp_cache_hit_rate Cache hit rate (0-1)\n");
        output.push_str("# TYPE mcp_cache_hit_rate gauge\n");
        output.push_str(&format!("mcp_cache_hit_rate {:.4}\n", self.cache_hit_rate()));

        output.push_str("# HELP mcp_uptime_seconds Server uptime in seconds\n");
        output.push_str("# TYPE mcp_uptime_seconds gauge\n");
        output.push_str(&format!("mcp_uptime_seconds {}\n", self.uptime_seconds()));

        // Requests by status code
        output.push_str("# HELP mcp_requests_by_status Total requests by HTTP status code\n");
        output.push_str("# TYPE mcp_requests_by_status counter\n");
        
        for entry in self.requests_by_status.iter() {
            output.push_str(&format!(
                "mcp_requests_by_status{{code=\"{}\"}} {}\n",
                entry.key(),
                entry.value().load(Ordering::SeqCst)
            ));
        }

        output
    }

    /// Export metrics in JSON format
    pub fn export_json(&self) -> serde_json::Value {
        let mut status_codes = serde_json::Map::new();
        for entry in self.requests_by_status.iter() {
            status_codes.insert(
                entry.key().to_string(),
                serde_json::json!(entry.value().load(Ordering::SeqCst)),
            );
        }

        serde_json::json!({
            "requests_total": self.total_requests(),
            "active_connections": self.active_connections(),
            "average_request_duration_ms": self.average_request_duration_ms(),
            "cache_hit_rate": self.cache_hit_rate(),
            "uptime_seconds": self.uptime_seconds(),
            "requests_by_status": status_codes,
        })
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared metrics handle
#[derive(Clone)]
pub struct SharedMetrics {
    inner: Arc<MetricsCollector>,
}

impl SharedMetrics {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(MetricsCollector::new()),
        }
    }

    pub fn record_request(&self, status_code: u16, duration_ms: u64) {
        self.inner.record_request(status_code, duration_ms);
    }

    pub fn connection_opened(&self) {
        self.inner.connection_opened();
    }

    pub fn connection_closed(&self) {
        self.inner.connection_closed();
    }

    pub fn record_cache_hit(&self) {
        self.inner.record_cache_hit();
    }

    pub fn record_cache_miss(&self) {
        self.inner.record_cache_miss();
    }

    pub fn export_prometheus(&self) -> String {
        self.inner.export_prometheus()
    }

    pub fn export_json(&self) -> serde_json::Value {
        self.inner.export_json()
    }
}

impl Default for SharedMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Metrics middleware for axum
pub async fn metrics_middleware(
    metrics: SharedMetrics,
    request: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let start = Instant::now();
    
    metrics.connection_opened();
    
    let response = next.run(request).await;
    
    metrics.connection_closed();
    
    let duration = start.elapsed();
    let duration_ms = duration.as_millis() as u64;
    let status = response.status().as_u16();
    
    metrics.record_request(status, duration_ms);
    
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_collector_creation() {
        let metrics = MetricsCollector::new();
        assert_eq!(metrics.total_requests(), 0);
        assert_eq!(metrics.active_connections(), 0);
    }

    #[test]
    fn test_record_request() {
        let metrics = MetricsCollector::new();
        metrics.record_request(200, 50);
        metrics.record_request(200, 100);
        metrics.record_request(404, 20);

        assert_eq!(metrics.total_requests(), 3);
        let avg = metrics.average_request_duration_ms();
        assert!(avg > 56.0 && avg < 57.0, "Average should be around 56.67, got {}", avg);
    }

    #[test]
    fn test_connections() {
        let metrics = MetricsCollector::new();
        
        metrics.connection_opened();
        metrics.connection_opened();
        assert_eq!(metrics.active_connections(), 2);
        
        metrics.connection_closed();
        assert_eq!(metrics.active_connections(), 1);
    }

    #[test]
    fn test_cache_metrics() {
        let metrics = MetricsCollector::new();
        
        metrics.record_cache_hit();
        metrics.record_cache_hit();
        metrics.record_cache_miss();
        
        assert_eq!(metrics.cache_hit_rate(), 2.0 / 3.0);
    }

    #[test]
    fn test_prometheus_export() {
        let metrics = MetricsCollector::new();
        metrics.record_request(200, 100);
        
        let output = metrics.export_prometheus();
        assert!(output.contains("mcp_requests_total"));
        assert!(output.contains("mcp_active_connections"));
    }

    #[test]
    fn test_json_export() {
        let metrics = MetricsCollector::new();
        metrics.record_request(200, 100);
        
        let json = metrics.export_json();
        assert!(json.get("requests_total").is_some());
        assert!(json.get("active_connections").is_some());
    }
}
