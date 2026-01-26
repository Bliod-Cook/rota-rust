//! CONNECT tunnel implementation for HTTPS proxying
//!
//! Handles bidirectional data transfer between client and target server.

use std::sync::Arc;

use hyper::upgrade::Upgraded;
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, instrument};

use crate::error::{Result, RotaError};
use crate::models::Proxy;
use crate::proxy::transport::ProxyTransport;

/// Handles CONNECT tunnel requests
pub struct TunnelHandler;

impl TunnelHandler {
    /// Establish a tunnel through the upstream proxy to the target
    #[instrument(skip(proxy), fields(proxy_id = proxy.id))]
    pub async fn tunnel_through_proxy(
        proxy: &Proxy,
        target_host: &str,
        target_port: u16,
    ) -> Result<impl AsyncRead + AsyncWrite + Unpin + Send> {
        ProxyTransport::connect(proxy, target_host, target_port).await
    }

    /// Establish a direct tunnel to the target (no upstream proxy)
    #[instrument]
    pub async fn tunnel_direct(target_host: &str, target_port: u16) -> Result<TcpStream> {
        let addr = format!("{}:{}", target_host, target_port);
        debug!("Establishing direct tunnel to {}", addr);

        TcpStream::connect(&addr)
            .await
            .map_err(|e| RotaError::ProxyConnectionFailed(format!("Direct connect failed: {}", e)))
    }

    /// Copy data bidirectionally between two streams
    #[instrument(skip(client, server))]
    pub async fn copy_bidirectional<C, S>(client: C, server: S) -> Result<(u64, u64)>
    where
        C: AsyncRead + AsyncWrite + Unpin + Send,
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        let (mut client_read, mut client_write) = tokio::io::split(client);
        let (mut server_read, mut server_write) = tokio::io::split(server);

        let client_to_server = async {
            let result = tokio::io::copy(&mut client_read, &mut server_write).await;
            let _ = server_write.shutdown().await;
            result
        };

        let server_to_client = async {
            let result = tokio::io::copy(&mut server_read, &mut client_write).await;
            let _ = client_write.shutdown().await;
            result
        };

        let (client_to_server_result, server_to_client_result) =
            tokio::join!(client_to_server, server_to_client);

        let bytes_sent = client_to_server_result.unwrap_or_else(|e| {
            debug!("Client to server copy ended: {}", e);
            0
        });

        let bytes_received = server_to_client_result.unwrap_or_else(|e| {
            debug!("Server to client copy ended: {}", e);
            0
        });

        debug!(
            bytes_sent = bytes_sent,
            bytes_received = bytes_received,
            "Tunnel closed"
        );

        Ok((bytes_sent, bytes_received))
    }

    /// Handle an upgraded connection (from hyper) and tunnel it
    #[instrument(skip(upgraded, proxy), fields(proxy_id = proxy.id))]
    pub async fn handle_upgraded(
        upgraded: Upgraded,
        proxy: &Proxy,
        target_host: &str,
        target_port: u16,
    ) -> Result<(u64, u64)> {
        // Connect to target through proxy
        let server = Self::tunnel_through_proxy(proxy, target_host, target_port).await?;

        // Wrap Upgraded with TokioIo to get tokio AsyncRead/AsyncWrite traits
        let client = TokioIo::new(upgraded);

        // Bidirectional copy
        Self::copy_bidirectional(client, server).await
    }

    /// Handle an upgraded connection directly (no upstream proxy)
    #[instrument(skip(upgraded))]
    pub async fn handle_upgraded_direct(
        upgraded: Upgraded,
        target_host: &str,
        target_port: u16,
    ) -> Result<(u64, u64)> {
        // Connect directly to target
        let server = Self::tunnel_direct(target_host, target_port).await?;

        // Wrap Upgraded with TokioIo to get tokio AsyncRead/AsyncWrite traits
        let client = TokioIo::new(upgraded);

        // Bidirectional copy
        Self::copy_bidirectional(client, server).await
    }
}

/// Guard for tracking active tunnel connections
pub struct TunnelGuard {
    proxy_id: i64,
    selector: Arc<dyn crate::proxy::rotation::ProxySelector>,
}

impl TunnelGuard {
    pub fn new(proxy_id: i64, selector: Arc<dyn crate::proxy::rotation::ProxySelector>) -> Self {
        selector.acquire(proxy_id);
        Self { proxy_id, selector }
    }
}

impl Drop for TunnelGuard {
    fn drop(&mut self) {
        self.selector.release(self.proxy_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::time::Duration;

    #[tokio::test]
    async fn test_copy_bidirectional() {
        // Create a pair of duplex streams for testing
        let (client, mut server) = tokio::io::duplex(1024);
        let (mut target_client, target_server) = tokio::io::duplex(1024);

        // Spawn the bidirectional copy
        let copy_handle = tokio::spawn(async move {
            TunnelHandler::copy_bidirectional(client, target_server).await
        });

        server.write_all(b"hello from client").await.unwrap();
        server.shutdown().await.unwrap();

        target_client.write_all(b"hello from server").await.unwrap();
        target_client.shutdown().await.unwrap();

        let mut buf = vec![0u8; 100];
        let n = target_client.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"hello from client");

        let mut buf = vec![0u8; 100];
        let n = server.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"hello from server");

        // Wait for copy to complete (should not hang)
        let result = tokio::time::timeout(Duration::from_secs(1), copy_handle)
            .await
            .expect("copy_bidirectional timed out")
            .unwrap();
        assert!(result.is_ok());
    }
}
