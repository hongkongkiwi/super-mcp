use crate::config::Config;
use crate::core::ServerManager;
use crate::http_server::routes;
use axum::{
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::info;

pub struct HttpServer {
    config: Config,
    server_manager: Arc<ServerManager>,
}

impl HttpServer {
    pub fn new(config: Config, server_manager: Arc<ServerManager>) -> Self {
        Self {
            config,
            server_manager,
        }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        let app = self.create_router();

        let addr = SocketAddr::from((
            self.config.server.host.parse::<std::net::IpAddr>()?,
            self.config.server.port,
        ));

        info!("Starting HTTP server on {}", addr);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }

    fn create_router(&self) -> Router {
        let server_manager = self.server_manager.clone();

        Router::new()
            .route("/health", get(routes::health))
            .route("/mcp", post(routes::mcp_handler))
            .route("/mcp/:server", post(routes::server_handler))
            .with_state(server_manager)
    }
}
