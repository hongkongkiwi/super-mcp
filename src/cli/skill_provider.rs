//! Skill provider implementation
//!
//! Parses SKILL.md files to extract tool definitions and execute skills.

use crate::core::provider::{ParameterSchema, Provider, ProviderType, Tool, ToolResult};
use crate::utils::errors::{McpError, McpResult};
use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;

/// Provider for SKILL.md-based skills
pub struct SkillProvider {
    name: String,
    skill_path: PathBuf,
    tools: Vec<Tool>,
}

impl SkillProvider {
    /// Create a new skill provider by parsing SKILL.md
    pub async fn new(name: &str, path: PathBuf) -> McpResult<Self> {
        let skill_file = path.join("SKILL.md");

        if !skill_file.exists() {
            return Err(McpError::ConfigError(format!(
                "SKILL.md not found at {}",
                skill_file.display()
            )));
        }

        let content = tokio::fs::read_to_string(&skill_file).await
            .map_err(|e| McpError::ConfigError(format!("Failed to read SKILL.md: {}", e)))?;

        let tools = Self::parse_skill_md(name, &content)?;

        Ok(Self {
            name: name.to_string(),
            skill_path: path,
            tools,
        })
    }

    /// Parse SKILL.md content to extract tool definitions
    pub fn parse_skill_md(skill_name: &str, content: &str) -> McpResult<Vec<Tool>> {
        let mut tools = Vec::new();

        // Parse markdown headers for tool definitions
        // Format: ### tool_name
        // Description...
        // Arguments:
        // - arg_name: type (required/optional) - description

        let lines: Vec<&str> = content.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i].trim();

            // Look for ### tool_name pattern
            if let Some(tool_name) = line.strip_prefix("### ") {
                let tool_name = tool_name.trim();

                // Skip if it's a section header (all caps or contains spaces as first word)
                if tool_name.is_empty() {
                    i += 1;
                    continue;
                }

                // Check if first word is all uppercase (section header like "## Tools")
                let first_word = tool_name.split_whitespace().next().unwrap_or("");
                if first_word.chars().all(|c| c.is_uppercase()) {
                    i += 1;
                    continue;
                }

                // Collect description (lines until "Arguments:" or next ###)
                let mut description = String::new();
                let mut end_of_tool = i + 1;
                while end_of_tool < lines.len() && !lines[end_of_tool].trim().starts_with("### ") {
                    if lines[end_of_tool].trim() == "Arguments:" {
                        break;
                    }
                    if !lines[end_of_tool].trim().is_empty() {
                        if !description.is_empty() {
                            description.push(' ');
                        }
                        description.push_str(lines[end_of_tool].trim());
                    }
                    end_of_tool += 1;
                }

                // Parse arguments if present
                let mut parameters = Vec::new();
                if end_of_tool < lines.len() && lines[end_of_tool].trim() == "Arguments:" {
                    let mut k = end_of_tool + 1;
                    while k < lines.len() && lines[k].trim().starts_with("- ") {
                        let arg_line = lines[k].trim().trim_start_matches("- ");
                        if let Some((arg_def, arg_desc)) = arg_line.split_once(':') {
                            let parts: Vec<&str> = arg_def.splitn(2, '(').collect();
                            let arg_name = parts[0].trim().to_string();

                            let required = if parts.len() > 1 {
                                let type_part = parts[1];
                                type_part.contains("required")
                            } else {
                                false
                            };

                            // Extract type
                            let arg_type = if parts.len() > 1 {
                                let type_part = parts[1];
                                if type_part.contains("string") {
                                    "string".to_string()
                                } else if type_part.contains("number") {
                                    "number".to_string()
                                } else if type_part.contains("boolean") {
                                    "boolean".to_string()
                                } else if type_part.contains("array") {
                                    "array".to_string()
                                } else if type_part.contains("object") {
                                    "object".to_string()
                                } else {
                                    "any".to_string()
                                }
                            } else {
                                "any".to_string()
                            };

                            parameters.push(ParameterSchema {
                                name: arg_name.clone(),
                                description: Some(arg_desc.trim().to_string()),
                                required,
                                param_type: arg_type,
                                default: None,
                            });
                        }
                        k += 1;
                    }
                    end_of_tool = k;
                }

                tools.push(Tool {
                    name: format!("{}.{}", skill_name, tool_name),
                    description: Some(description),
                    provider: skill_name.to_string(),
                    provider_type: ProviderType::Skill,
                    parameters,
                    metadata: std::collections::HashMap::new(),
                });

                i = end_of_tool;
            } else {
                i += 1;
            }
        }

        Ok(tools)
    }
}

#[async_trait]
impl Provider for SkillProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn provider_type(&self) -> ProviderType {
        ProviderType::Skill
    }

    async fn is_available(&self) -> bool {
        self.skill_path.exists() && self.skill_path.join("SKILL.md").exists()
    }

    async fn list_tools(&self) -> McpResult<Vec<Tool>> {
        Ok(self.tools.clone())
    }

    async fn call_tool(&self, name: &str, arguments: Value) -> McpResult<ToolResult> {
        // Strip provider prefix to get tool name
        let tool_name = name
            .strip_prefix(&format!("{}.", self.name))
            .unwrap_or(name);

        // Find the tool definition
        let tool = self.tools.iter().find(|t| {
            t.name == name || t.name == format!("{}.{}", self.name, tool_name)
        });

        if tool.is_none() {
            return Ok(ToolResult::error(format!("Tool '{}' not found in skill", tool_name)));
        }

        // For now, return a placeholder result
        // Full implementation would execute the skill logic
        Ok(ToolResult {
            success: true,
            data: Some(serde_json::json!({
                "status": "skill_called",
                "skill": self.name,
                "tool": tool_name,
                "arguments": arguments,
                "note": "Skill execution not yet fully implemented"
            })),
            error: None,
            content: None,
        })
    }
}
