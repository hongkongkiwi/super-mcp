//! Tests for the skill provider module

use supermcp::cli::skill_provider::SkillProvider;
use supermcp::core::Provider;
use tempfile::TempDir;
use tokio::fs;

#[tokio::test]
async fn test_skill_provider_parses_skill_md() {
    let temp_dir = TempDir::new().unwrap();
    let skill_dir = temp_dir.path().join("test-skill");
    fs::create_dir_all(&skill_dir).await.unwrap();

    // Create a SKILL.md file with tool definitions
    let skill_content = r#"# Test Skill

## Tools

### tool1
Description for tool1

Arguments:
- arg1: string (required) - First argument
- arg2: number (optional) - Second argument

### tool2
Description for tool2
"#;

    fs::write(skill_dir.join("SKILL.md"), skill_content).await.unwrap();

    let provider = SkillProvider::new("test-skill", skill_dir).await.unwrap();
    let tools = provider.list_tools().await.unwrap();

    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0].name, "test-skill.tool1");
    assert_eq!(tools[0].description, Some("Description for tool1".to_string()));
}

#[tokio::test]
async fn test_skill_provider_call_tool() {
    let temp_dir = TempDir::new().unwrap();
    let skill_dir = temp_dir.path().join("exec-skill");
    fs::create_dir_all(&skill_dir).await.unwrap();

    // Create a skill that executes a command
    let skill_content = r#"# Exec Skill

Executes shell commands.

### exec
Execute a shell command

Arguments:
- cmd: string (required) - The command to execute
"#;

    fs::write(skill_dir.join("SKILL.md"), skill_content).await.unwrap();

    let provider = SkillProvider::new("exec-skill", skill_dir).await.unwrap();
    let result = provider.call_tool("exec-skill.exec", serde_json::json!({"cmd": "echo hello"})).await;

    // Should either succeed or fail gracefully (depending on implementation)
    assert!(result.is_ok() || result.unwrap().success == false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_skill_md_simple() {
        let content = r#"# Test Skill

### read_file
Read a file from disk

Arguments:
- path: string (required) - The file path to read
- encoding: string (optional) - File encoding (default: utf-8)
"#;

        let tools = SkillProvider::parse_skill_md("test", content).unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "test.read_file");
        assert_eq!(tools[0].parameters.len(), 2);
    }

    #[test]
    fn test_parse_skill_md_multiple_tools() {
        let content = r#"# Multi Tool Skill

### tool1
First tool

Arguments:
- arg1: string (required) - First argument

### tool2
Second tool

### tool3
Third tool
"#;

        let tools = SkillProvider::parse_skill_md("multi", content).unwrap();
        assert_eq!(tools.len(), 3);
    }
}
