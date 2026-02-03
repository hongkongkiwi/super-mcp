//! Tests for the tools command --skill flag

use clap::Parser;
use supermcp::cli::args::ToolsArgs;

#[test]
fn test_tools_args_accepts_skill_flag() {
    let args = ToolsArgs::parse_from([
        "supermcp",
        "tools",
        "--skill", "my-skill",
        "--all",
    ]);
    assert_eq!(args.skill, Some("my-skill".to_string()));
    assert!(args.all);
}

#[test]
fn test_tools_args_skill_is_optional() {
    let args = ToolsArgs::parse_from([
        "supermcp",
        "tools",
        "--all",
    ]);
    assert_eq!(args.skill, None);
    assert!(args.all);
}

#[test]
fn test_tools_args_without_all_flag() {
    let args = ToolsArgs::parse_from([
        "supermcp",
        "tools",
        "--skill", "test-skill",
    ]);
    assert_eq!(args.skill, Some("test-skill".to_string()));
    assert!(!args.all);
}
