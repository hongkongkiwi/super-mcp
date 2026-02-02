//! Structured audit logging for security events

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tracing::{error, info};

/// Type of audit event
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventType {
    /// Server started
    ServerStart,
    /// Server stopped
    ServerStop,
    /// Configuration reloaded
    ConfigReload,
    /// Configuration changed
    ConfigChange,
    /// Authentication attempt
    AuthAttempt,
    /// Authentication success
    AuthSuccess,
    /// Authentication failure
    AuthFailure,
    /// Authorization failure
    AuthorizationFailure,
    /// Request received
    Request,
    /// Response sent
    Response,
    /// Error occurred
    Error,
    /// Server spawned
    ServerSpawn,
    /// Server stopped/killed
    ServerStopRequest,
    /// Rate limit hit
    RateLimitHit,
    /// Suspicious activity detected
    SuspiciousActivity,
}

/// Audit event structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Timestamp of the event
    pub timestamp: DateTime<Utc>,
    /// Type of event
    pub event_type: AuditEventType,
    /// User ID (if authenticated)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// Client IP address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_ip: Option<String>,
    /// Request ID for correlation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Server name (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    /// Event details
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
    /// Success or failure
    pub success: bool,
    /// Error message (if failed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

impl AuditEvent {
    /// Create a new audit event
    pub fn new(event_type: AuditEventType) -> Self {
        Self {
            timestamp: Utc::now(),
            event_type,
            user_id: None,
            client_ip: None,
            request_id: None,
            server_name: None,
            details: None,
            success: true,
            error_message: None,
        }
    }

    /// Set user ID
    pub fn with_user_id(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    /// Set client IP
    pub fn with_client_ip(mut self, ip: impl Into<String>) -> Self {
        self.client_ip = Some(ip.into());
        self
    }

    /// Set request ID
    pub fn with_request_id(mut self, id: impl Into<String>) -> Self {
        self.request_id = Some(id.into());
        self
    }

    /// Set server name
    pub fn with_server_name(mut self, name: impl Into<String>) -> Self {
        self.server_name = Some(name.into());
        self
    }

    /// Set details
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    /// Mark as failed with error message
    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.success = false;
        self.error_message = Some(error.into());
        self
    }
}

/// Audit logger configuration
#[derive(Debug, Clone)]
pub struct AuditConfig {
    /// Log file path
    pub path: PathBuf,
    /// Log format
    pub format: LogFormat,
    /// Maximum file size in MB before rotation
    pub max_size_mb: u64,
    /// Maximum number of log files to keep
    pub max_files: u32,
    /// Log to stdout as well
    pub log_to_stdout: bool,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::from("/var/log/super-mcp/audit.log"),
            format: LogFormat::Json,
            max_size_mb: 100,
            max_files: 10,
            log_to_stdout: false,
        }
    }
}

/// Log format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Json,
    Pretty,
}

/// Async audit logger
pub struct AuditLogger {
    config: AuditConfig,
    file: Arc<Mutex<File>>,
    current_size: Arc<Mutex<u64>>,
}

impl AuditLogger {
    /// Create a new audit logger
    pub async fn new(config: AuditConfig) -> std::io::Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = config.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(false)
            .open(&config.path)
            .await?;

        let metadata = file.metadata().await?;
        let current_size = metadata.len();

        info!("Audit logger initialized: {}", config.path.display());

        Ok(Self {
            config,
            file: Arc::new(Mutex::new(file)),
            current_size: Arc::new(Mutex::new(current_size)),
        })
    }

    /// Log an audit event
    pub async fn log(&self, event: AuditEvent) {
        let log_line = match self.config.format {
            LogFormat::Json => match serde_json::to_string(&event) {
                Ok(json) => format!("{}\n", json),
                Err(e) => {
                    error!("Failed to serialize audit event: {}", e);
                    return;
                }
            },
            LogFormat::Pretty => self.format_pretty(&event),
        };

        let bytes = log_line.as_bytes();
        let len = bytes.len() as u64;

        // Check if we need to rotate
        let should_rotate = {
            let current = *self.current_size.lock().await;
            current + len > self.config.max_size_mb * 1024 * 1024
        };

        if should_rotate {
            if let Err(e) = self.rotate().await {
                error!("Failed to rotate audit log: {}", e);
            }
        }

        // Write to file
        {
            let mut file = self.file.lock().await;
            if let Err(e) = file.write_all(bytes).await {
                error!("Failed to write audit log: {}", e);
                return;
            }
            if let Err(e) = file.flush().await {
                error!("Failed to flush audit log: {}", e);
            }
        }

        // Update size
        *self.current_size.lock().await += len;

        // Also log to stdout if configured
        if self.config.log_to_stdout {
            println!("[AUDIT] {}", log_line.trim());
        }
    }

    /// Format event in pretty format
    fn format_pretty(&self, event: &AuditEvent) -> String {
        let user_str = event.user_id.as_deref().unwrap_or("anonymous");
        let ip_str = event.client_ip.as_deref().unwrap_or("unknown");
        let server_str = event.server_name.as_deref().unwrap_or("-");
        let status = if event.success { "OK" } else { "FAIL" };

        format!(
            "[{}] {} | user={} | ip={} | server={} | status={} | type={:?}\n",
            event.timestamp.to_rfc3339(),
            event.request_id.as_deref().unwrap_or("-"),
            user_str,
            ip_str,
            server_str,
            status,
            event.event_type
        )
    }

    /// Rotate log file
    async fn rotate(&self) -> std::io::Result<()> {
        let path = &self.config.path;
        let max_files = self.config.max_files;

        // Remove oldest log file if at limit
        let oldest = format!("{}.{}.{}", path.display(), max_files, "log");
        let _ = tokio::fs::remove_file(&oldest).await;

        // Shift existing log files
        for i in (1..max_files).rev() {
            let from = format!("{}.{}.{}", path.display(), i - 1, "log");
            let to = format!("{}.{}.{}", path.display(), i, "log");
            let _ = tokio::fs::rename(&from, &to).await;
        }

        // Rename current log
        let rotated = format!("{}.{}.{}", path.display(), 0, "log");
        let _ = tokio::fs::rename(path, &rotated).await;

        // Create new log file
        let new_file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(false)
            .open(path)
            .await?;

        *self.file.lock().await = new_file;
        *self.current_size.lock().await = 0;

        info!("Audit log rotated");
        Ok(())
    }

    /// Log server start event
    pub async fn log_server_start(&self, host: &str, port: u16) {
        let event = AuditEvent::new(AuditEventType::ServerStart)
            .with_details(serde_json::json!({
                "host": host,
                "port": port,
            }));
        self.log(event).await;
    }

    /// Log authentication attempt
    pub async fn log_auth_attempt(&self, user_id: Option<&str>, client_ip: &str, success: bool) {
        let event_type = if success {
            AuditEventType::AuthSuccess
        } else {
            AuditEventType::AuthFailure
        };

        let mut event = AuditEvent::new(event_type)
            .with_client_ip(client_ip);

        if let Some(uid) = user_id {
            event = event.with_user_id(uid);
        }

        self.log(event).await;
    }

    /// Log MCP request
    pub async fn log_request(
        &self,
        user_id: Option<&str>,
        client_ip: &str,
        server_name: &str,
        method: &str,
    ) {
        let mut event = AuditEvent::new(AuditEventType::Request)
            .with_client_ip(client_ip)
            .with_server_name(server_name)
            .with_details(serde_json::json!({
                "method": method,
            }));

        if let Some(uid) = user_id {
            event = event.with_user_id(uid);
        }

        self.log(event).await;
    }

    /// Log configuration change
    pub async fn log_config_change(&self, user_id: Option<&str>, change_type: &str, details: &str) {
        let mut event = AuditEvent::new(AuditEventType::ConfigChange)
            .with_details(serde_json::json!({
                "change_type": change_type,
                "details": details,
            }));

        if let Some(uid) = user_id {
            event = event.with_user_id(uid);
        }

        self.log(event).await;
    }

    /// Log rate limit hit
    pub async fn log_rate_limit(&self, client_ip: &str, user_id: Option<&str>) {
        let mut event = AuditEvent::new(AuditEventType::RateLimitHit)
            .with_client_ip(client_ip);

        if let Some(uid) = user_id {
            event = event.with_user_id(uid);
        }

        self.log(event).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_audit_logger_creation() {
        let temp_dir = TempDir::new().unwrap();
        let config = AuditConfig {
            path: temp_dir.path().join("audit.log"),
            ..Default::default()
        };

        let logger = AuditLogger::new(config).await;
        assert!(logger.is_ok());
    }

    #[tokio::test]
    async fn test_audit_event_logging() {
        let temp_dir = TempDir::new().unwrap();
        let config = AuditConfig {
            path: temp_dir.path().join("audit.log"),
            format: LogFormat::Json,
            ..Default::default()
        };

        let logger = AuditLogger::new(config).await.unwrap();

        let event = AuditEvent::new(AuditEventType::ServerStart)
            .with_user_id("test-user")
            .with_client_ip("127.0.0.1")
            .with_details(serde_json::json!({"port": 3000}));

        logger.log(event).await;

        // Verify log was written
        let content = tokio::fs::read_to_string(temp_dir.path().join("audit.log"))
            .await
            .unwrap();
        assert!(content.contains("server_start"));
        assert!(content.contains("test-user"));
        assert!(content.contains("127.0.0.1"));
    }

    #[test]
    fn test_audit_event_builder() {
        let event = AuditEvent::new(AuditEventType::AuthAttempt)
            .with_user_id("user123")
            .with_client_ip("192.168.1.1")
            .with_request_id("req-456")
            .with_server_name("test-server")
            .with_details(serde_json::json!({"foo": "bar"}))
            .with_error("Invalid credentials");

        assert_eq!(event.user_id, Some("user123".to_string()));
        assert_eq!(event.client_ip, Some("192.168.1.1".to_string()));
        assert_eq!(event.request_id, Some("req-456".to_string()));
        assert_eq!(event.server_name, Some("test-server".to_string()));
        assert!(!event.success);
        assert_eq!(event.error_message, Some("Invalid credentials".to_string()));
    }
}
