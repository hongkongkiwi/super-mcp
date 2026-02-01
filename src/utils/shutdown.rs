//! Graceful shutdown handling
//!
//! Handles SIGTERM, SIGINT signals for graceful server shutdown.

use std::future::Future;
use tokio::signal;
use tracing::{info, warn};

/// Graceful shutdown coordinator
pub struct ShutdownCoordinator {
    /// Shutdown signal receivers
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
}

impl ShutdownCoordinator {
    /// Create a new shutdown coordinator
    pub fn new() -> Self {
        let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);
        Self { shutdown_tx }
    }

    /// Subscribe to shutdown signal
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<()> {
        self.shutdown_tx.subscribe()
    }

    /// Trigger shutdown
    pub fn shutdown(&self) {
        info!("Shutdown signal sent");
        let _ = self.shutdown_tx.send(());
    }

    /// Wait for shutdown signal (SIGTERM, SIGINT, or manual trigger)
    pub async fn wait_for_shutdown_signal(&self) {
        let ctrl_c = async {
            signal::ctrl_c()
                .await
                .expect("Failed to install Ctrl+C handler");
        };

        #[cfg(unix)]
        let terminate = async {
            signal::unix::signal(signal::unix::SignalKind::terminate())
                .expect("Failed to install signal handler")
                .recv()
                .await;
        };

        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();

        tokio::select! {
            _ = ctrl_c => {
                info!("Received Ctrl+C, starting graceful shutdown");
            }
            _ = terminate => {
                info!("Received SIGTERM, starting graceful shutdown");
            }
        }

        self.shutdown();
    }

    /// Run a future with graceful shutdown support
    pub async fn run_with_shutdown<F>(&self, fut: F, timeout_secs: u64)
    where
        F: Future<Output = ()>,
    {
        let mut shutdown_rx = self.subscribe();

        tokio::select! {
            _ = fut => {
                info!("Server stopped normally");
            }
            _ = shutdown_rx.recv() => {
                info!("Received shutdown signal, waiting for graceful shutdown...");
                
                // Give components time to shutdown
                let timeout = tokio::time::Duration::from_secs(timeout_secs);
                tokio::time::sleep(timeout).await;
                
                warn!("Shutdown timeout reached, forcing exit");
            }
        }
    }
}

impl Default for ShutdownCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

/// Guard that triggers shutdown when dropped (for cleanup)
pub struct ShutdownGuard {
    coordinator: Option<ShutdownCoordinator>,
}

impl ShutdownGuard {
    pub fn new(coordinator: ShutdownCoordinator) -> Self {
        Self {
            coordinator: Some(coordinator),
        }
    }
}

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        if let Some(coordinator) = self.coordinator.take() {
            coordinator.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shutdown_coordinator_creation() {
        let coordinator = ShutdownCoordinator::new();
        let rx = coordinator.subscribe();
        assert_eq!(rx.len(), 0); // No messages yet
    }

    #[tokio::test]
    async fn test_shutdown_signal() {
        let coordinator = ShutdownCoordinator::new();
        let mut rx = coordinator.subscribe();

        coordinator.shutdown();

        assert!(rx.recv().await.is_ok());
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let coordinator = ShutdownCoordinator::new();
        let mut rx1 = coordinator.subscribe();
        let mut rx2 = coordinator.subscribe();

        coordinator.shutdown();

        assert!(rx1.recv().await.is_ok());
        assert!(rx2.recv().await.is_ok());
    }
}
