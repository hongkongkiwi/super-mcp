//! Authentication module

pub mod jwt;
pub mod oauth;
pub mod provider;
pub mod static_token;

pub use jwt::JwtAuth;
pub use oauth::OAuthAuth;
pub use provider::{AuthProvider, Session, Tokens};
pub use static_token::StaticTokenAuth;
