//! Runtime module for executing scripts in sandboxed environments
//!
//! This module provides support for executing scripts using different runtimes
//! with maximum sandboxing:
//! - Python via WASM (Pyodide-like)
//! - Node.js via pnpm, npm, or bun

pub mod manager;
pub mod node;
pub mod python_wasm;
pub mod types;

pub use manager::RuntimeManager;
pub use types::{RuntimeConfig, ResourceLimits, RuntimeType};
