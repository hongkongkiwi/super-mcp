//! Authentication provider tests

use supermcp::auth::{AuthProvider, JwtAuth, StaticTokenAuth};

#[tokio::test]
async fn test_static_token_auth_valid() {
    let auth = StaticTokenAuth::new("test-token-123");
    
    let session = auth.validate_token("test-token-123").await.unwrap();
    assert_eq!(session.user_id, "admin");
    assert_eq!(session.scopes, vec!["*"]);
}

#[tokio::test]
async fn test_static_token_auth_invalid() {
    let auth = StaticTokenAuth::new("test-token-123");
    
    let result = auth.validate_token("wrong-token").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_static_token_auth_with_custom_user() {
    let auth = StaticTokenAuth::new("test-token")
        .with_user("custom-user")
        .with_scopes(vec!["read".to_string(), "write".to_string()]);
    
    let session = auth.validate_token("test-token").await.unwrap();
    assert_eq!(session.user_id, "custom-user");
    assert_eq!(session.scopes, vec!["read", "write"]);
}

#[tokio::test]
async fn test_jwt_auth_valid() {
    let auth = JwtAuth::new("test-secret-key");
    
    // Generate a token
    let tokens = auth.generate_token("user123", vec!["read".to_string()]).await.unwrap();
    
    // Validate the token
    let session = auth.validate_token(&tokens.access_token).await.unwrap();
    assert_eq!(session.user_id, "user123");
    assert_eq!(session.scopes, vec!["read"]);
}

#[tokio::test]
async fn test_jwt_auth_invalid_secret() {
    let auth1 = JwtAuth::new("secret1");
    let auth2 = JwtAuth::new("secret2");
    
    let tokens = auth1.generate_token("user123", vec![]).await.unwrap();
    
    // Should fail with different secret
    let result = auth2.validate_token(&tokens.access_token).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_jwt_auth_refresh() {
    let auth = JwtAuth::new("test-secret-key");
    
    let tokens = auth.generate_token("user123", vec!["read".to_string()]).await.unwrap();
    let refreshed = auth.refresh_token(&tokens.access_token).await.unwrap();
    
    // Should get a new valid token
    let session = auth.validate_token(&refreshed.access_token).await.unwrap();
    assert_eq!(session.user_id, "user123");
}

#[tokio::test]
async fn test_jwt_auth_is_configured() {
    let auth1 = JwtAuth::new("test-secret");
    assert!(auth1.is_configured());
    
    let auth2 = JwtAuth::new("");
    assert!(!auth2.is_configured());
}

#[tokio::test]
async fn test_static_token_is_configured() {
    let auth1 = StaticTokenAuth::new("test-token");
    assert!(auth1.is_configured());
    
    let auth2 = StaticTokenAuth::new("");
    assert!(!auth2.is_configured());
}
