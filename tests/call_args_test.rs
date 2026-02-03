//! Tests for argument parsing functions

use supermcp::cli::call::{parse_call_args, parse_function_style};

#[test]
fn test_parse_call_args_with_bare_flags() {
    let args = vec!["verbose".to_string(), "debug".to_string()];
    let result = parse_call_args(&args).unwrap();
    assert_eq!(result["verbose"], true);
    assert_eq!(result["debug"], true);
}

#[test]
fn test_parse_call_args_mixed_syntax() {
    let args = vec![
        "key1:value1".to_string(),
        "key2=value2".to_string(),
        "flag".to_string(),
        "num:42".to_string(),
    ];
    let result = parse_call_args(&args).unwrap();
    assert_eq!(result["key1"], "value1");
    assert_eq!(result["key2"], "value2");
    assert_eq!(result["flag"], true);
    assert_eq!(result["num"], 42);
}

#[test]
fn test_parse_call_args_json_values() {
    let args = vec!["data:{\"key\":\"value\"}".to_string()];
    let result = parse_call_args(&args).unwrap();
    assert_eq!(result["data"]["key"], "value");
}

#[test]
fn test_parse_function_style_with_booleans() {
    let input = r#"tool(flag: true, debug: false)"#;
    let (name, params) = parse_function_style(input).unwrap();
    assert_eq!(name, "tool");
    assert_eq!(params["flag"], true);
    assert_eq!(params["debug"], false);
}

#[test]
fn test_parse_function_style_with_null() {
    let input = r#"tool(value: null)"#;
    let (name, params) = parse_function_style(input).unwrap();
    assert_eq!(name, "tool");
    assert_eq!(params["value"], serde_json::json!(null));
}

#[test]
fn test_parse_function_style_with_array() {
    let input = r#"tool(items: [1, 2, 3])"#;
    let (name, params) = parse_function_style(input).unwrap();
    assert_eq!(name, "tool");
    assert!(params["items"].is_array());
}

#[test]
fn test_parse_function_style_empty_args() {
    let input = "tool()";
    let (name, params) = parse_function_style(input).unwrap();
    assert_eq!(name, "tool");
    assert!(params.as_object().unwrap().is_empty());
}

#[test]
fn test_parse_function_style_complex_values() {
    let input = r#"search(query: "hello world", limit: 10, options: {"a": 1})"#;
    let (name, params) = parse_function_style(input).unwrap();
    assert_eq!(name, "search");
    assert_eq!(params["query"], "hello world");
    assert_eq!(params["limit"], 10);
    assert_eq!(params["options"]["a"], 1);
}
