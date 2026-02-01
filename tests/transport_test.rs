//! Transport layer tests

use super_mcp::core::protocol::{JsonRpcRequest, JsonRpcResponse, RequestId};
use serde_json::json;

#[test]
fn test_json_rpc_request_creation() {
    let request = JsonRpcRequest::new("initialize", Some(json!({"test": true})));
    
    assert_eq!(request.jsonrpc, "2.0");
    assert_eq!(request.method, "initialize");
    assert!(request.params.is_some());
}

#[test]
fn test_json_rpc_response_success() {
    let response = JsonRpcResponse::success(
        RequestId::Number(1),
        json!({"result": "ok"}),
    );
    
    assert_eq!(response.jsonrpc, "2.0");
    assert!(response.result.is_some());
    assert!(response.error.is_none());
}

#[test]
fn test_json_rpc_response_error() {
    let response = JsonRpcResponse::error(
        RequestId::Number(1),
        -32600,
        "Invalid request",
    );
    
    assert_eq!(response.jsonrpc, "2.0");
    assert!(response.result.is_none());
    assert!(response.error.is_some());
    
    let error = response.error.unwrap();
    assert_eq!(error.code, -32600);
    assert_eq!(error.message, "Invalid request");
}

#[test]
fn test_is_notification() {
    let notification = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: None,
        method: "notifications/initialized".to_string(),
        params: None,
    };
    assert!(notification.is_notification());

    let request = JsonRpcRequest::new("test", None);
    assert!(!request.is_notification());
}

#[test]
fn test_request_id_string() {
    let id = RequestId::String("abc123".to_string());
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, "\"abc123\"");
}

#[test]
fn test_request_id_number() {
    let id = RequestId::Number(42);
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, "42");
}

#[test]
fn test_request_id_deserialization() {
    let string_id: RequestId = serde_json::from_str("\"test-id\"").unwrap();
    assert!(matches!(string_id, RequestId::String(s) if s == "test-id"));
    
    let number_id: RequestId = serde_json::from_str("123").unwrap();
    assert!(matches!(number_id, RequestId::Number(n) if n == 123));
}

#[test]
fn test_json_rpc_serialization() {
    let request = JsonRpcRequest::new("tools/list", None);
    let json = serde_json::to_string(&request).unwrap();
    
    assert!(json.contains("\"jsonrpc\":\"2.0\""));
    assert!(json.contains("\"method\":\"tools/list\""));
    assert!(json.contains("\"id\""));
}

#[test]
fn test_json_rpc_deserialization() {
    let json = r#"{"jsonrpc":"2.0","id":1,"method":"ping","params":null}"#;
    let request: JsonRpcRequest = serde_json::from_str(json).unwrap();
    
    assert_eq!(request.jsonrpc, "2.0");
    assert_eq!(request.method, "ping");
    assert!(matches!(request.id.unwrap(), RequestId::Number(1)));
}
