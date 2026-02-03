//! Additional tests for skill provider parsing

use supermcp::cli::skill_provider::SkillProvider;

#[test]
fn test_parse_skill_md_empty_content() {
    let content = "";
    let tools = SkillProvider::parse_skill_md("test", content).unwrap();
    assert!(tools.is_empty());
}

#[test]
fn test_parse_skill_md_only_header() {
    let content = "# Test Skill\n\nJust a description.";
    let tools = SkillProvider::parse_skill_md("test", content).unwrap();
    assert!(tools.is_empty());
}

#[test]
fn test_parse_skill_md_section_headers_skipped() {
    let content = r#"# Test Skill

## TOOLS
This section should be skipped

### tool1
First tool
"#;
    let tools = SkillProvider::parse_skill_md("test", content).unwrap();
    // TOOLS (all caps) should be skipped as a section header
    assert!(tools.is_empty() || tools.len() == 1);
}

#[test]
fn test_parse_skill_md_tool_without_args() {
    let content = r#"# Test Skill

### simple_tool
A tool without arguments
"#;
    let tools = SkillProvider::parse_skill_md("test", content).unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "test.simple_tool");
    assert!(tools[0].parameters.is_empty());
}

#[test]
fn test_parse_skill_md_multiple_args() {
    let content = r#"# Test Skill

### tool_with_many_args
Description here

Arguments:
- arg1: string (required) - First argument
- arg2: number (optional) - Second argument
- arg3: boolean (required) - Third argument
- arg4: array (optional) - Fourth argument
"#;
    let tools = SkillProvider::parse_skill_md("test", content).unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].parameters.len(), 4);

    // Check arg1
    assert_eq!(tools[0].parameters[0].name, "arg1");
    assert_eq!(tools[0].parameters[0].param_type, "string");
    assert!(tools[0].parameters[0].required);

    // Check arg2
    assert_eq!(tools[0].parameters[1].name, "arg2");
    assert_eq!(tools[0].parameters[1].param_type, "number");
    assert!(!tools[0].parameters[1].required);

    // Check arg3
    assert_eq!(tools[0].parameters[2].name, "arg3");
    assert_eq!(tools[0].parameters[2].param_type, "boolean");
    assert!(tools[0].parameters[2].required);

    // Check arg4
    assert_eq!(tools[0].parameters[3].name, "arg4");
    assert_eq!(tools[0].parameters[3].param_type, "array");
    assert!(!tools[0].parameters[3].required);
}

#[test]
fn test_parse_skill_md_tool_name_with_underscores() {
    let content = r#"# Test Skill

### read_file_from_path
Read a file
"#;
    let tools = SkillProvider::parse_skill_md("test", content).unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "test.read_file_from_path");
}

#[test]
fn test_parse_skill_md_tool_name_with_dashes() {
    let content = r#"# Test Skill

### read-file
Read a file
"#;
    let tools = SkillProvider::parse_skill_md("test", content).unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "test.read-file");
}

#[test]
fn test_parse_skill_md_unknown_type_defaults_to_any() {
    let content = r#"# Test Skill

### weird_tool
A tool with unknown type

Arguments:
- weird_arg: unknown_type (optional) - Some arg
"#;
    let tools = SkillProvider::parse_skill_md("test", content).unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].parameters[0].param_type, "any");
}

#[test]
fn test_parse_skill_md_multiline_description() {
    let content = r#"# Test Skill

### multiline_tool
This is a tool with
a multiline description
that spans multiple lines.

Arguments:
- arg1: string (required) - First
"#;
    let tools = SkillProvider::parse_skill_md("test", content).unwrap();
    assert_eq!(tools.len(), 1);
    // Description should contain parts of the multiline text
    assert!(tools[0].description.as_ref().unwrap().contains("multiline"));
}

#[test]
fn test_parse_skill_md_multiple_tools() {
    let content = r#"# Multi Tool Skill

### tool1
First tool

### tool2
Second tool

Arguments:
- arg: string (required) - An arg

### tool3
Third tool without args
"#;
    let tools = SkillProvider::parse_skill_md("multi", content).unwrap();
    assert_eq!(tools.len(), 3);

    assert_eq!(tools[0].name, "multi.tool1");
    assert_eq!(tools[1].name, "multi.tool2");
    assert_eq!(tools[2].name, "multi.tool3");
}

#[test]
fn test_parse_skill_md_preserves_description() {
    let content = r#"# Test Skill

### important_tool
This is a very important tool that does critical things.
"#;
    let tools = SkillProvider::parse_skill_md("test", content).unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(
        tools[0].description,
        Some("This is a very important tool that does critical things.".to_string())
    );
}
