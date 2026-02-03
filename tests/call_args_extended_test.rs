//! Additional tests for argument parsing edge cases

use supermcp::cli::call::{parse_call_args, parse_function_style};

#[test]
fn test_parse_call_args_empty() {
    let args: Vec<String> = vec![];
    let result = parse_call_args(&args).unwrap();
    assert!(result.as_object().unwrap().is_empty());
}

#[test]
fn test_parse_call_args_only_flags() {
    let args = vec!["--verbose".to_string(), "--debug".to_string()];
    let result = parse_call_args(&args).unwrap();
    assert_eq!(result["--verbose"], true);
    assert_eq!(result["--debug"], true);
}

#[test]
fn test_parse_call_args_colons_only() {
    let args = vec!["key:value:with:colons".to_string()];
    let result = parse_call_args(&args).unwrap();
    assert_eq!(result["key"], "value:with:colons");
}

#[test]
fn test_parse_call_args_equals_only() {
    let args = vec!["key=value=equals".to_string()];
    let result = parse_call_args(&args).unwrap();
    assert_eq!(result["key"], "value=equals");
}

#[test]
fn test_parse_call_args_number_values() {
    let args = vec![
        "int:42".to_string(),
        "float:3.14".to_string(),
        "negative:-10".to_string(),
    ];
    let result = parse_call_args(&args).unwrap();
    assert_eq!(result["int"], 42);
    assert!(result["float"].is_number());
    assert_eq!(result["negative"], -10);
}

#[test]
fn test_parse_call_args_boolean_values() {
    let args = vec!["t:true".to_string(), "f:false".to_string()];
    let result = parse_call_args(&args).unwrap();
    assert_eq!(result["t"], true);
    assert_eq!(result["f"], false);
}

#[test]
fn test_parse_function_style_nested_brackets() {
    let input = r#"tool(data: {"nested": {"key": [1, 2, 3]}})"#;
    let (name, params) = parse_function_style(input).unwrap();
    assert_eq!(name, "tool");
    assert!(params["data"]["nested"]["key"].is_array());
}

#[test]
fn test_parse_function_style_quoted_strings() {
    let input = r#"search(query: "hello \"world\"", filter: 'single quotes')"#;
    let (name, params) = parse_function_style(input).unwrap();
    assert_eq!(name, "search");
    assert_eq!(params["query"], "hello \"world\"");
    assert_eq!(params["filter"], "single quotes");
}

#[test]
fn test_parse_function_style_trailing_comma() {
    let input = r#"tool(a: 1, b: 2,)"#;
    let (name, params) = parse_function_style(input).unwrap();
    assert_eq!(name, "tool");
    assert_eq!(params["a"], 1);
    assert_eq!(params["b"], 2);
}

#[test]
fn test_parse_function_style_no_args() {
    let input = "tool";
    let result = parse_function_style(input);
    assert!(result.is_err());
}

#[test]
fn test_parse_function_style_unclosed_paren() {
    let input = r#"tool(arg: value"#;
    let result = parse_function_style(input);
    assert!(result.is_err());
}

#[test]
fn test_parse_call_args_json_with_special_chars() {
    let args = vec!["data:{\"key\":\"value with spaces\"}".to_string()];
    let result = parse_call_args(&args).unwrap();
    assert_eq!(result["data"]["key"], "value with spaces");
}

#[test]
fn test_parse_call_args_mixed_formats() {
    let args = vec![
        "name=John".to_string(),
        "age:30".to_string(),
        "active".to_string(),
        "meta:{\"debug\":true}".to_string(),
    ];
    let result = parse_call_args(&args).unwrap();
    assert_eq!(result["name"], "John");
    assert_eq!(result["age"], 30);
    assert_eq!(result["active"], true);
    assert_eq!(result["meta"]["debug"], true);
}
