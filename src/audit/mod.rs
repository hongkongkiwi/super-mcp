//! Audit logging module for security events

pub mod logger;

pub use logger::{AuditEvent, AuditEventType, AuditLogger};
