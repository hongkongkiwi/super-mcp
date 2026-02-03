//! Tests for ToolResult formatting and text extraction

use supermcp::core::provider::ToolResult;

#[test]
fn test_tool_result_success() {
    let result = ToolResult::success("test data").unwrap();
    assert!(result.success);
    assert!(result.error.is_none());
    assert!(result.data.is_some());
}

#[test]
fn test_tool_result_error() {
    let result = ToolResult::error("something went wrong");
    assert!(!result.success);
    assert!(result.error.is_some());
    assert!(result.error.unwrap().contains("went wrong"));
}

#[test]
fn test_tool_result_with_content() {
    let content = vec![
        serde_json::json!({"text": "Hello"}),
        serde_json::json!({"text": "World"}),
    ];
    let result = ToolResult::success("data").unwrap().with_content(content);
    assert!(result.content.is_some());
    assert_eq!(result.content.unwrap().len(), 2);
}

#[test]
fn test_tool_result_text_extraction() {
    let content = vec![serde_json::json!({"text": "extracted text"})];
    let result = ToolResult::success("data").unwrap().with_content(content);
    assert_eq!(result.text(), Some("extracted text".to_string()));
}

#[test]
fn test_tool_result_text_no_content() {
    let result = ToolResult::success("data").unwrap();
    assert_eq!(result.text(), None);
}
