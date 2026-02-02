//! Runtime manager for coordinating multiple runtime instances
//!
//! This module provides the RuntimeManager which handles registration,
//! validation, and execution of different runtime types.

use crate::runtime::types::{
    ExecutionResult, RuntimeConfig, RuntimeError, RuntimeType, Runtime as RuntimeTrait,
};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info};

/// Runtime instance wrapper
#[derive(Clone)]
pub struct RuntimeInstance {
    /// Runtime name
    name: String,
    /// Runtime configuration
    config: RuntimeConfig,
    /// Runtime implementation (boxed trait object)
    runtime: Arc<dyn RuntimeTrait>,
    /// Whether the runtime is currently validated
    validated: Arc<RwLock<bool>>,
}

impl RuntimeInstance {
    /// Create a new runtime instance
    fn new(name: String, config: RuntimeConfig, runtime: Arc<dyn RuntimeTrait>) -> Self {
        Self {
            name,
            config,
            runtime,
            validated: Arc::new(RwLock::new(false)),
        }
    }

    /// Get the runtime name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the runtime type
    pub fn runtime_type(&self) -> RuntimeType {
        self.config.type_.clone()
    }

    /// Check if the runtime is validated
    pub fn is_validated(&self) -> bool {
        *self.validated.read()
    }

    /// Mark the runtime as validated
    pub fn set_validated(&self, validated: bool) {
        *self.validated.write() = validated;
    }

    /// Get access to the inner runtime
    pub fn runtime(&self) -> Arc<dyn RuntimeTrait> {
        self.runtime.clone()
    }

    /// Get the config
    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }
}

/// Runtime manager for coordinating multiple runtime instances
#[derive(Clone, Default)]
pub struct RuntimeManager {
    /// Map of registered runtimes
    runtimes: DashMap<String, RuntimeInstance>,
    /// Default runtime name
    default_runtime: Arc<RwLock<Option<String>>>,
}

impl RuntimeManager {
    /// Create a new runtime manager
    pub fn new() -> Self {
        Self {
            runtimes: DashMap::new(),
            default_runtime: Arc::new(RwLock::new(None)),
        }
    }

    /// Register a runtime
    pub fn register(&self, config: RuntimeConfig, runtime: Arc<dyn RuntimeTrait>) {
        let name = config.name.clone();
        let instance = RuntimeInstance::new(name.clone(), config, runtime);

        self.runtimes.insert(name.clone(), instance);

        // Set as default if first runtime
        let is_first = self.runtimes.len() == 1;
        if is_first {
            *self.default_runtime.write() = Some(name.clone());
        }

        info!("Registered runtime: {}", name);
    }

    /// Register a runtime with automatic runtime detection
    pub fn register_auto(&self, config: RuntimeConfig) -> Result<(), RuntimeError> {
        let runtime: Arc<dyn RuntimeTrait> = match config.type_ {
            RuntimeType::PythonWasm => {
                Arc::new(crate::runtime::python_wasm::PythonWasmRuntime::new(
                    config.name.clone(),
                    config.clone(),
                ))
            }
            RuntimeType::NodePnpm => Arc::new(crate::runtime::node::NodeRuntimeImpl::new(
                config.name.clone(),
                config.clone(),
            )),
            RuntimeType::NodeNpm => Arc::new(crate::runtime::node::NodeRuntimeImpl::new(
                config.name.clone(),
                config.clone(),
            )),
            RuntimeType::NodeBun => Arc::new(crate::runtime::node::NodeRuntimeImpl::new(
                config.name.clone(),
                config.clone(),
            )),
        };

        self.register(config, runtime);
        Ok(())
    }

    /// Remove a runtime
    pub fn remove(&self, name: &str) -> bool {
        self.runtimes.remove(name).is_some()
    }

    /// Get a runtime by name
    pub fn get(&self, name: &str) -> Option<RuntimeInstance> {
        self.runtimes.get(name).map(|r| r.clone())
    }

    /// Get the default runtime
    pub fn default(&self) -> Option<RuntimeInstance> {
        let default = self.default_runtime.read().clone()?;
        self.get(&default)
    }

    /// Set the default runtime
    pub fn set_default(&self, name: &str) -> bool {
        if self.runtimes.contains_key(name) {
            *self.default_runtime.write() = Some(name.to_string());
            true
        } else {
            false
        }
    }

    /// List all registered runtime names
    pub fn list(&self) -> Vec<String> {
        self.runtimes.iter().map(|r| r.key().clone()).collect()
    }

    /// Get all runtimes
    pub fn all(&self) -> Vec<RuntimeInstance> {
        self.runtimes.iter().map(|r| r.clone()).collect()
    }

    /// Validate all runtimes
    pub async fn validate_all(&self) -> Vec<(String, Result<(), RuntimeError>)> {
        let mut results = Vec::new();

        for entry in self.runtimes.iter() {
            let name = entry.key().clone();
            let runtime = entry.runtime.clone();

            debug!("Validating runtime: {}", name);

            let result = runtime.validate().await;
            entry.set_validated(result.is_ok());

            results.push((name, result));
        }

        results
    }

    /// Execute a script using a named runtime
    pub async fn execute(
        &self,
        runtime_name: &str,
        script: &str,
        input: Option<Value>,
    ) -> Result<ExecutionResult, RuntimeError> {
        let instance = self
            .runtimes
            .get(runtime_name)
            .ok_or_else(|| RuntimeError::RuntimeNotFound(runtime_name.to_string()))?;

        // Validate if not already validated
        if !instance.is_validated() {
            instance.runtime.validate().await?;
            instance.set_validated(true);
        }

        instance.runtime.execute(script, input).await
    }

    /// Execute a script using the default runtime
    pub async fn execute_default(
        &self,
        script: &str,
        input: Option<Value>,
    ) -> Result<ExecutionResult, RuntimeError> {
        let default = self
            .default_runtime
            .read()
            .clone()
            .ok_or_else(|| RuntimeError::RuntimeNotFound("No default runtime set".to_string()))?;

        self.execute(&default, script, input).await
    }

    /// Execute a script file using a named runtime
    pub async fn execute_file(
        &self,
        runtime_name: &str,
        path: &PathBuf,
        input: Option<Value>,
    ) -> Result<ExecutionResult, RuntimeError> {
        let instance = self
            .runtimes
            .get(runtime_name)
            .ok_or_else(|| RuntimeError::RuntimeNotFound(runtime_name.to_string()))?;

        instance.runtime.execute_file(path, input).await
    }

    /// Get runtime information
    pub fn info(&self, name: &str) -> Option<RuntimeInfo> {
        self.runtimes.get(name).map(|entry| RuntimeInfo {
            name: entry.name().to_string(),
            runtime_type: entry.config.type_.clone(),
            packages: entry.config.packages.clone(),
            enabled: entry.config.enabled,
            resource_limits: entry.config.resource_limits.clone(),
        })
    }

    /// Check if a runtime exists
    pub fn contains(&self, name: &str) -> bool {
        self.runtimes.contains_key(name)
    }

    /// Get the count of registered runtimes
    pub fn len(&self) -> usize {
        self.runtimes.len()
    }

    /// Check if no runtimes are registered
    pub fn is_empty(&self) -> bool {
        self.runtimes.is_empty()
    }
}

/// Runtime information for display
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RuntimeInfo {
    pub name: String,
    pub runtime_type: RuntimeType,
    pub packages: Vec<String>,
    pub enabled: bool,
    pub resource_limits: crate::runtime::types::ResourceLimits,
}

impl RuntimeInfo {
    /// Get a human-readable type name
    pub fn type_name(&self) -> String {
        match self.runtime_type {
            RuntimeType::PythonWasm => "Python (WASM)".to_string(),
            RuntimeType::NodePnpm => "Node.js (pnpm)".to_string(),
            RuntimeType::NodeNpm => "Node.js (npm)".to_string(),
            RuntimeType::NodeBun => "Node.js (bun)".to_string(),
        }
    }
}

/// Create a runtime manager with default runtimes
pub fn create_default_runtime_manager() -> RuntimeManager {
    let manager = RuntimeManager::new();

    // Register default Python WASM runtime
    let python_config = RuntimeConfig {
        name: "python".to_string(),
        type_: RuntimeType::PythonWasm,
        packages: vec![],
        working_dir: None,
        env: HashMap::new(),
        resource_limits: crate::runtime::types::ResourceLimits::default(),
        enabled: true,
    };

    let _ = manager.register_auto(python_config);

    // Register default Node.js runtimes
    for (name, runtime_type) in [
        ("pnpm".to_string(), RuntimeType::NodePnpm),
        ("npm".to_string(), RuntimeType::NodeNpm),
        ("bun".to_string(), RuntimeType::NodeBun),
    ] {
        let config = RuntimeConfig {
            name: name.clone(),
            type_: runtime_type,
            packages: vec![],
            working_dir: None,
            env: HashMap::new(),
            resource_limits: crate::runtime::types::ResourceLimits::default(),
            enabled: true,
        };

        let _ = manager.register_auto(config);
    }

    manager
}

use std::collections::HashMap;
