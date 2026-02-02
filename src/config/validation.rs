//! Configuration validation using JSON Schema

use crate::config::Config;
#[allow(unused_imports)]
use crate::utils::errors::McpResult;
use schemars::schema_for;
use serde_json::Value;
use std::path::Path;
use validator::Validate;

/// Validation error
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub path: String,
    pub message: String,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

impl std::error::Error for ValidationError {}

/// Configuration validator
pub struct ConfigValidator {
    schema: Value,
}

impl ConfigValidator {
    /// Create a new validator with the generated schema
    pub fn new() -> Self {
        let schema = schema_for!(Config);
        Self {
            schema: serde_json::to_value(&schema).unwrap_or_default(),
        }
    }

    /// Get the JSON Schema for the configuration
    pub fn get_schema(&self) -> &Value {
        &self.schema
    }

    /// Export the schema to a JSON string
    pub fn export_schema(&self) -> String {
        serde_json::to_string_pretty(&self.schema).unwrap_or_default()
    }

    /// Validate a configuration file
    pub async fn validate_file(&self, path: &str) -> Result<(), Vec<ValidationError>> {
        let expanded = shellexpand::tilde(path).to_string();
        let path = Path::new(&expanded);

        if !path.exists() {
            return Err(vec![ValidationError {
                path: path.to_string_lossy().to_string(),
                message: "Configuration file does not exist".to_string(),
            }]);
        }

        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| vec![ValidationError {
                path: path.to_string_lossy().to_string(),
                message: format!("Failed to read file: {}", e),
            }])?;

        self.validate_toml(&content)
    }

    /// Validate TOML content
    pub fn validate_toml(&self, content: &str) -> Result<(), Vec<ValidationError>> {
        let mut errors = Vec::new();

        // Parse TOML
        let config: Config = match toml::from_str(content) {
            Ok(c) => c,
            Err(e) => {
                errors.push(ValidationError {
                    path: "root".to_string(),
                    message: format!("TOML parse error: {}", e),
                });
                return Err(errors);
            }
        };

        // Validate using validator crate
        if let Err(validation_errors) = config.validate() {
            for error in validation_errors.field_errors() {
                errors.push(ValidationError {
                    path: error.0.to_string(),
                    message: format!("{:?}", error.1),
                });
            }
        }

        // Additional custom validations
        self.validate_server_configs(&config, &mut errors);
        self.validate_preset_configs(&config, &mut errors);
        self.validate_auth_config(&config, &mut errors);

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn validate_server_configs(&self, config: &Config, errors: &mut Vec<ValidationError>) {
        let mut names = std::collections::HashSet::new();

        for (idx, server) in config.servers.iter().enumerate() {
            // Check for duplicate names
            if !names.insert(&server.name) {
                errors.push(ValidationError {
                    path: format!("servers[{}].name", idx),
                    message: format!("Duplicate server name: {}", server.name),
                });
            }

            // Validate server name
            if server.name.is_empty() {
                errors.push(ValidationError {
                    path: format!("servers[{}].name", idx),
                    message: "Server name cannot be empty".to_string(),
                });
            }

            // Validate command
            if server.command.is_empty() {
                errors.push(ValidationError {
                    path: format!("servers[{}].command", idx),
                    message: "Server command cannot be empty".to_string(),
                });
            }

            // Validate sandbox memory limits
            if server.sandbox.max_memory_mb == 0 {
                errors.push(ValidationError {
                    path: format!("servers[{}].sandbox.max_memory_mb", idx),
                    message: "Memory limit must be greater than 0".to_string(),
                });
            }

            // Validate sandbox CPU limits
            if server.sandbox.max_cpu_percent == 0 || server.sandbox.max_cpu_percent > 100 {
                errors.push(ValidationError {
                    path: format!("servers[{}].sandbox.max_cpu_percent", idx),
                    message: "CPU percentage must be between 1 and 100".to_string(),
                });
            }
        }
    }

    fn validate_preset_configs(&self, config: &Config, errors: &mut Vec<ValidationError>) {
        let mut names = std::collections::HashSet::new();

        for (idx, preset) in config.presets.iter().enumerate() {
            // Check for duplicate names
            if !names.insert(&preset.name) {
                errors.push(ValidationError {
                    path: format!("presets[{}].name", idx),
                    message: format!("Duplicate preset name: {}", preset.name),
                });
            }

            // Validate preset name
            if preset.name.is_empty() {
                errors.push(ValidationError {
                    path: format!("presets[{}].name", idx),
                    message: "Preset name cannot be empty".to_string(),
                });
            }

            // Validate that tags are not empty
            if preset.tags.is_empty() {
                errors.push(ValidationError {
                    path: format!("presets[{}].tags", idx),
                    message: "Preset must have at least one tag".to_string(),
                });
            }
        }
    }

    fn validate_auth_config(&self, config: &Config, errors: &mut Vec<ValidationError>) {
        use crate::config::AuthType;

        match config.auth.auth_type {
            AuthType::Static => {
                if config.auth.token.is_none() {
                    errors.push(ValidationError {
                        path: "auth.token".to_string(),
                        message: "Static auth requires a token".to_string(),
                    });
                }
            }
            AuthType::Jwt => {
                if config.auth.issuer.is_none() {
                    errors.push(ValidationError {
                        path: "auth.issuer".to_string(),
                        message: "JWT auth requires an issuer".to_string(),
                    });
                }
                if config.auth.jwt_secret.is_none() {
                    errors.push(ValidationError {
                        path: "auth.jwt_secret".to_string(),
                        message: "JWT auth requires a jwt_secret".to_string(),
                    });
                }
            }
            AuthType::OAuth => {
                if config.auth.client_id.is_none() {
                    errors.push(ValidationError {
                        path: "auth.client_id".to_string(),
                        message: "OAuth auth requires a client_id".to_string(),
                    });
                }
                if config.auth.client_secret.is_none() {
                    errors.push(ValidationError {
                        path: "auth.client_secret".to_string(),
                        message: "OAuth auth requires a client_secret".to_string(),
                    });
                }
                if config.auth.issuer.is_none()
                    && (config.auth.auth_url.is_none() || config.auth.token_url.is_none())
                {
                    errors.push(ValidationError {
                        path: "auth.issuer".to_string(),
                        message: "OAuth auth requires either issuer or auth_url + token_url".to_string(),
                    });
                }
                if config.auth.introspection_url.is_none()
                    && config.auth.userinfo_url.is_none()
                    && config.auth.jwks_url.is_none()
                    && config.auth.issuer.is_none()
                    && !config.auth.allow_unverified_jwt
                {
                    errors.push(ValidationError {
                        path: "auth".to_string(),
                        message: "OAuth auth requires jwks_url, introspection_url, or userinfo_url (or allow_unverified_jwt=true)".to_string(),
                    });
                }
            }
            AuthType::None => {}
        }
    }
}

impl Default for ConfigValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_empty_config() {
        let validator = ConfigValidator::new();
        let config = Config::default();
        
        // Convert to TOML and back to validate
        let toml = toml::to_string(&config).unwrap();
        let result = validator.validate_toml(&toml);
        
        // Empty config should be valid (with defaults)
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_duplicate_server_names() {
        let validator = ConfigValidator::new();
        let toml = r#"
[[servers]]
name = "test"
command = "echo"

[[servers]]
name = "test"
command = "echo"
"#;
        
        let result = validator.validate_toml(toml);
        assert!(result.is_err());
        
        let errors = result.unwrap_err();
        assert!(errors.iter().any(|e| e.message.contains("Duplicate")));
    }

    #[test]
    fn test_validate_empty_server_name() {
        let validator = ConfigValidator::new();
        let toml = r#"
[[servers]]
name = ""
command = "echo"
"#;
        
        let result = validator.validate_toml(toml);
        assert!(result.is_err());
        
        let errors = result.unwrap_err();
        assert!(errors.iter().any(|e| e.path.contains("name")));
    }

    #[test]
    fn test_schema_generation() {
        let validator = ConfigValidator::new();
        let schema = validator.export_schema();
        
        assert!(!schema.is_empty());
        assert!(schema.contains("$schema"));
    }
}
