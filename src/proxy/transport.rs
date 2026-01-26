//! Proxy transport layer for HTTP and SOCKS protocols
//!
//! Handles establishing connections through upstream proxies.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use hyper::Uri;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_socks::tcp::Socks5Stream;
use tracing::{debug, instrument};

use crate::error::{Result, RotaError};
use crate::models::Proxy;

/// Proxy transport handler
///
/// Manages connections through various proxy protocols
pub struct ProxyTransport;

impl ProxyTransport {
    /// Connect to a target through the specified proxy
    #[instrument(skip(proxy), fields(proxy_id = proxy.id, target = %target_host))]
    pub async fn connect(
        proxy: &Proxy,
        target_host: &str,
        target_port: u16,
    ) -> Result<Box<dyn ProxyConnection>> {
        let protocol = proxy.protocol.to_lowercase();
        match protocol.as_str() {
            "http" | "https" => Self::connect_http(proxy, target_host, target_port).await,
            "socks4" => Self::connect_socks4(proxy, target_host, target_port).await,
            "socks4a" => Self::connect_socks4a(proxy, target_host, target_port).await,
            "socks5" => Self::connect_socks5(proxy, target_host, target_port).await,
            _ => Err(RotaError::UnsupportedProtocol(protocol)),
        }
    }

    /// Connect through HTTP CONNECT method
    async fn connect_http(
        proxy: &Proxy,
        target_host: &str,
        target_port: u16,
    ) -> Result<Box<dyn ProxyConnection>> {
        debug!("Connecting to HTTP proxy at {}", proxy.address);

        let stream = TcpStream::connect(&proxy.address)
            .await
            .map_err(|e| RotaError::ProxyConnectionFailed(format!("TCP connect failed: {}", e)))?;

        // Send CONNECT request
        let connect_request = Self::build_connect_request(proxy, target_host, target_port);

        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut stream = stream;
        stream.write_all(connect_request.as_bytes()).await.map_err(|e| {
            RotaError::ProxyConnectionFailed(format!("Failed to send CONNECT: {}", e))
        })?;

        // Read response
        let mut response = vec![0u8; 1024];
        let n = stream.read(&mut response).await.map_err(|e| {
            RotaError::ProxyConnectionFailed(format!("Failed to read CONNECT response: {}", e))
        })?;

        let response_str = String::from_utf8_lossy(&response[..n]);
        if !response_str.starts_with("HTTP/1.1 200") && !response_str.starts_with("HTTP/1.0 200") {
            return Err(RotaError::ProxyConnectionFailed(format!(
                "CONNECT failed: {}",
                response_str.lines().next().unwrap_or("Unknown error")
            )));
        }

        debug!("HTTP CONNECT tunnel established");
        Ok(Box::new(TcpConnection(stream)))
    }

    /// Build HTTP CONNECT request
    fn build_connect_request(proxy: &Proxy, target_host: &str, target_port: u16) -> String {
        let mut request = format!(
            "CONNECT {}:{} HTTP/1.1\r\nHost: {}:{}\r\n",
            target_host, target_port, target_host, target_port
        );

        // Add proxy authentication if credentials are provided
        if let (Some(username), Some(password)) = (&proxy.username, &proxy.password) {
            let credentials = format!("{}:{}", username, password);
            let encoded = BASE64.encode(credentials.as_bytes());
            request.push_str(&format!("Proxy-Authorization: Basic {}\r\n", encoded));
        }

        request.push_str("\r\n");
        request
    }

    /// Connect through SOCKS4 proxy
    async fn connect_socks4(
        proxy: &Proxy,
        target_host: &str,
        target_port: u16,
    ) -> Result<Box<dyn ProxyConnection>> {
        debug!("Connecting to SOCKS4 proxy at {}", proxy.address);

        // SOCKS4 requires IP address, not hostname
        let target_ip: std::net::Ipv4Addr = target_host.parse().map_err(|_| {
            RotaError::ProxyConnectionFailed(
                "SOCKS4 requires IP address, not hostname. Use SOCKS4a or SOCKS5 for DNS resolution"
                    .to_string(),
            )
        })?;

        let target_addr = std::net::SocketAddrV4::new(target_ip, target_port);

        // Parse proxy address
        let proxy_sock_addr: std::net::SocketAddr = proxy.address.parse().map_err(|_| {
            RotaError::InvalidProxyAddress(format!("Invalid proxy address: {}", proxy.address))
        })?;

        let stream = if let (Some(username), Some(password)) = (&proxy.username, &proxy.password) {
            Socks5Stream::connect_with_password(proxy_sock_addr, target_addr, username, password)
                .await
        } else {
            Socks5Stream::connect(proxy_sock_addr, target_addr).await
        }
        .map_err(|e| RotaError::ProxyConnectionFailed(format!("SOCKS4 connect failed: {}", e)))?;

        debug!("SOCKS4 connection established");
        Ok(Box::new(Socks5Connection(stream.into_inner())))
    }

    /// Connect through SOCKS4a proxy (supports hostname)
    async fn connect_socks4a(
        proxy: &Proxy,
        target_host: &str,
        target_port: u16,
    ) -> Result<Box<dyn ProxyConnection>> {
        debug!("Connecting to SOCKS4a proxy at {}", proxy.address);

        // Parse proxy address
        let proxy_sock_addr: std::net::SocketAddr = proxy.address.parse().map_err(|_| {
            RotaError::InvalidProxyAddress(format!("Invalid proxy address: {}", proxy.address))
        })?;

        let target = format!("{}:{}", target_host, target_port);
        let target_sock_addr: std::net::SocketAddr = target.parse().map_err(|_| {
            RotaError::InvalidRequest(format!("Invalid target address: {}", target))
        })?;

        let stream = if let (Some(username), Some(password)) = (&proxy.username, &proxy.password) {
            Socks5Stream::connect_with_password(
                proxy_sock_addr,
                target_sock_addr,
                username,
                password,
            )
            .await
        } else {
            Socks5Stream::connect(proxy_sock_addr, target_sock_addr).await
        }
        .map_err(|e| RotaError::ProxyConnectionFailed(format!("SOCKS4a connect failed: {}", e)))?;

        debug!("SOCKS4a connection established");
        Ok(Box::new(Socks5Connection(stream.into_inner())))
    }

    /// Connect through SOCKS5 proxy
    async fn connect_socks5(
        proxy: &Proxy,
        target_host: &str,
        target_port: u16,
    ) -> Result<Box<dyn ProxyConnection>> {
        debug!("Connecting to SOCKS5 proxy at {}", proxy.address);

        // Parse proxy address
        let proxy_sock_addr: std::net::SocketAddr = proxy.address.parse().map_err(|_| {
            RotaError::InvalidProxyAddress(format!("Invalid proxy address: {}", proxy.address))
        })?;

        let target = format!("{}:{}", target_host, target_port);
        let target_sock_addr: std::net::SocketAddr = target.parse().map_err(|_| {
            RotaError::InvalidRequest(format!("Invalid target address: {}", target))
        })?;

        let stream = if let (Some(username), Some(password)) = (&proxy.username, &proxy.password) {
            Socks5Stream::connect_with_password(
                proxy_sock_addr,
                target_sock_addr,
                username,
                password,
            )
            .await
        } else {
            Socks5Stream::connect(proxy_sock_addr, target_sock_addr).await
        }
        .map_err(|e| RotaError::ProxyConnectionFailed(format!("SOCKS5 connect failed: {}", e)))?;

        debug!("SOCKS5 connection established");
        Ok(Box::new(Socks5Connection(stream.into_inner())))
    }

    /// Parse host and port from a URI
    pub fn parse_target(uri: &Uri) -> Result<(String, u16)> {
        let host = uri
            .host()
            .ok_or_else(|| RotaError::InvalidRequest("Missing host in URI".to_string()))?
            .to_string();

        let port = uri.port_u16().unwrap_or_else(|| match uri.scheme_str() {
            Some("https") => 443,
            _ => 80,
        });

        Ok((host, port))
    }

    /// Parse host and port from authority (for CONNECT requests)
    pub fn parse_authority(authority: &str) -> Result<(String, u16)> {
        if let Some((host, port_str)) = authority.rsplit_once(':') {
            let port = port_str
                .parse::<u16>()
                .map_err(|_| RotaError::InvalidRequest("Invalid port".to_string()))?;
            Ok((host.to_string(), port))
        } else {
            // Default to port 443 for CONNECT (typically HTTPS)
            Ok((authority.to_string(), 443))
        }
    }
}

/// Trait for proxy connections
pub trait ProxyConnection: AsyncRead + AsyncWrite + Unpin + Send + 'static {}

/// TCP connection wrapper
struct TcpConnection(TcpStream);

impl AsyncRead for TcpConnection {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for TcpConnection {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

impl ProxyConnection for TcpConnection {}

/// SOCKS5 connection wrapper
struct Socks5Connection(TcpStream);

impl AsyncRead for Socks5Connection {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for Socks5Connection {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

impl ProxyConnection for Socks5Connection {}
