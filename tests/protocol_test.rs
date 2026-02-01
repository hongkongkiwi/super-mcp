use mcp_one::core::protocol::*;
use serde_json::json;

#[test]
fn test_json_rpc_request_serialization() {
    let request = JsonRpcRequest::new(
        "initialize",
        Some(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "1.0" }
        })),
    );

    let json_str = serde_json::to_string(&request).unwrap();
    assert!(json_str.contains("\"jsonrpc\":\"2.0\""));
    assert!(json_str.contains("\"method\":\"initialize\""));
}

#[test]
fn test_json_rpc_response_serialization() {
    let response = JsonRpcResponse::success(
        RequestId::Number(1),
        json!({ "protocolVersion": "2024-11-05" }),
    );

    let json_str = serde_json::to_string(&response).unwrap();
    assert!(json_str.contains("\"jsonrpc\":\"2.0\""));
    assert!(json_str.contains("\"result\""));
}

#[test]
fn test_request_id_types() {
    let string_id: RequestId = serde_json::from_str("\"abc123\"").unwrap();
    assert!(matches!(string_id, RequestId::String(s) if s == "abc123"));

    let number_id: RequestId = serde_json::from_str("42").unwrap();
    assert!(matches!(number_id, RequestId::Number(n) if n == 42));
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
