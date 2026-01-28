use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_socks::tcp::Socks5Stream;

use crate::config::{EgressProxyConfig, EgressProxyProtocol};
use crate::error::{Result, RotaError};

pub async fn connect_to_addr(
    egress_proxy: Option<&EgressProxyConfig>,
    addr: &str,
) -> Result<TcpStream> {
    let (host, port) = parse_host_port(addr)?;
    connect_to_host_port(egress_proxy, &host, port).await
}

pub async fn connect_to_host_port(
    egress_proxy: Option<&EgressProxyConfig>,
    host: &str,
    port: u16,
) -> Result<TcpStream> {
    let direct_addr = format_tcp_addr(host, port);

    let Some(egress_proxy) = egress_proxy else {
        return TcpStream::connect(&direct_addr)
            .await
            .map_err(|e| RotaError::ProxyConnectionFailed(format!("TCP connect failed: {}", e)));
    };

    let proxy_addr = format_tcp_addr(&egress_proxy.host, egress_proxy.port);

    match egress_proxy.protocol {
        EgressProxyProtocol::Http => connect_via_http_proxy(egress_proxy, &proxy_addr, host, port)
            .await
            .map_err(|e| {
                RotaError::ProxyConnectionFailed(format!(
                    "Egress HTTP proxy connect failed ({} -> {}): {}",
                    proxy_addr, direct_addr, e
                ))
            }),
        EgressProxyProtocol::Socks5 => {
            connect_via_socks5_proxy(egress_proxy, &proxy_addr, host, port)
                .await
                .map_err(|e| {
                    RotaError::ProxyConnectionFailed(format!(
                        "Egress SOCKS5 proxy connect failed ({} -> {}): {}",
                        proxy_addr, direct_addr, e
                    ))
                })
        }
    }
}

async fn connect_via_http_proxy(
    proxy: &EgressProxyConfig,
    proxy_addr: &str,
    target_host: &str,
    target_port: u16,
) -> std::result::Result<TcpStream, anyhow::Error> {
    let mut stream = TcpStream::connect(proxy_addr).await?;

    let authority = format_connect_authority(target_host, target_port);
    let mut request = format!("CONNECT {} HTTP/1.1\r\nHost: {}\r\n", authority, authority);

    if let Some(username) = &proxy.username {
        let password = proxy.password.as_deref().unwrap_or("");
        let credentials = format!("{}:{}", username, password);
        request.push_str(&format!(
            "Proxy-Authorization: Basic {}\r\n",
            BASE64.encode(credentials.as_bytes())
        ));
    }

    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).await?;

    let mut response = vec![0u8; 1024];
    let n = stream.read(&mut response).await?;
    if n == 0 {
        anyhow::bail!("empty CONNECT response");
    }

    let response_str = String::from_utf8_lossy(&response[..n]);
    if !response_str.starts_with("HTTP/1.1 200") && !response_str.starts_with("HTTP/1.0 200") {
        anyhow::bail!(
            "CONNECT failed: {}",
            response_str.lines().next().unwrap_or("Unknown error")
        );
    }

    Ok(stream)
}

async fn connect_via_socks5_proxy(
    proxy: &EgressProxyConfig,
    proxy_addr: &str,
    target_host: &str,
    target_port: u16,
) -> std::result::Result<TcpStream, anyhow::Error> {
    let socket = TcpStream::connect(proxy_addr).await?;

    let stream = match (&proxy.username, &proxy.password) {
        (Some(username), Some(password)) => {
            Socks5Stream::connect_with_password_and_socket(
                socket,
                (target_host, target_port),
                username,
                password,
            )
            .await?
        }
        _ => Socks5Stream::connect_with_socket(socket, (target_host, target_port)).await?,
    };

    Ok(stream.into_inner())
}

fn parse_host_port(addr: &str) -> Result<(String, u16)> {
    // Use URL parsing to properly handle bracketed IPv6 like "[::1]:8080".
    let url = url::Url::parse(&format!("http://{}", addr)).map_err(|e| {
        RotaError::InvalidProxyAddress(format!("Invalid address '{}': {}", addr, e))
    })?;

    let host = url.host_str().ok_or_else(|| {
        RotaError::InvalidProxyAddress(format!("Invalid address '{}': missing host", addr))
    })?;
    let host = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);

    let port = url.port().ok_or_else(|| {
        RotaError::InvalidProxyAddress(format!("Invalid address '{}': missing port", addr))
    })?;

    Ok((host.to_string(), port))
}

fn format_tcp_addr(host: &str, port: u16) -> String {
    if host.contains(':') && !(host.starts_with('[') && host.ends_with(']')) {
        format!("[{}]:{}", host, port)
    } else {
        format!("{}:{}", host, port)
    }
}

fn format_connect_authority(host: &str, port: u16) -> String {
    if host.contains(':') && !(host.starts_with('[') && host.ends_with(']')) {
        format!("[{}]:{}", host, port)
    } else {
        format!("{}:{}", host, port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tokio::net::TcpListener;
    use tokio::time::{timeout, Duration};

    #[test]
    fn parse_host_port_rejects_missing_port() {
        let err = parse_host_port("example.com").unwrap_err();
        assert!(matches!(err, RotaError::InvalidProxyAddress(_)));
    }

    #[test]
    fn parse_host_port_supports_ipv6() {
        let (host, port) = parse_host_port("[::1]:8080").unwrap();
        assert_eq!(host, "::1");
        assert_eq!(port, 8080);
    }

    #[tokio::test]
    async fn connect_via_http_proxy_tunnels_bytes() {
        // Start an echo target.
        let target_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_addr = target_listener.local_addr().unwrap();
        let target_task = tokio::spawn(async move {
            let (mut stream, _) = target_listener.accept().await.unwrap();
            let mut buf = [0u8; 64];
            let n = stream.read(&mut buf).await.unwrap();
            stream.write_all(&buf[..n]).await.unwrap();
        });

        // Start a minimal HTTP CONNECT forward proxy.
        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = proxy_listener.local_addr().unwrap();
        let proxy_task = tokio::spawn(async move {
            let (mut client, _) = proxy_listener.accept().await.unwrap();

            // Read CONNECT request.
            let mut buf = vec![0u8; 2048];
            let n = client.read(&mut buf).await.unwrap();
            let req = String::from_utf8_lossy(&buf[..n]);

            assert!(req.starts_with("CONNECT 127.0.0.1:"));
            assert!(req.contains("Proxy-Authorization: Basic "));

            // Dial target and acknowledge.
            let mut server = TcpStream::connect(target_addr).await.unwrap();
            client
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await
                .unwrap();

            // Relay one round-trip (enough for this test).
            let mut relay_buf = [0u8; 64];
            let n = client.read(&mut relay_buf).await.unwrap();
            server.write_all(&relay_buf[..n]).await.unwrap();
            let n = server.read(&mut relay_buf).await.unwrap();
            client.write_all(&relay_buf[..n]).await.unwrap();
        });

        let cfg = EgressProxyConfig {
            protocol: EgressProxyProtocol::Http,
            host: proxy_addr.ip().to_string(),
            port: proxy_addr.port(),
            username: Some("user".to_string()),
            password: Some("pass".to_string()),
        };

        let mut stream = connect_to_host_port(Some(&cfg), "127.0.0.1", target_addr.port())
            .await
            .unwrap();

        stream.write_all(b"ping").await.unwrap();
        let mut out = [0u8; 4];
        timeout(Duration::from_secs(1), stream.read_exact(&mut out))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&out, b"ping");

        proxy_task.await.unwrap();
        target_task.await.unwrap();
    }

    #[tokio::test]
    async fn connect_via_socks5_proxy_tunnels_bytes() {
        // Start an echo target.
        let target_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_addr = target_listener.local_addr().unwrap();
        let target_task = tokio::spawn(async move {
            let (mut stream, _) = target_listener.accept().await.unwrap();
            let mut buf = [0u8; 64];
            let n = stream.read(&mut buf).await.unwrap();
            stream.write_all(&buf[..n]).await.unwrap();
        });

        // Start a minimal SOCKS5 forward proxy with username/password auth.
        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = proxy_listener.local_addr().unwrap();
        let proxy_task = tokio::spawn(async move {
            let (mut client, _) = proxy_listener.accept().await.unwrap();

            // Greeting: VER, NMETHODS, METHODS...
            let mut header = [0u8; 2];
            client.read_exact(&mut header).await.unwrap();
            assert_eq!(header[0], 0x05);
            let nmethods = header[1] as usize;
            let mut methods = vec![0u8; nmethods];
            client.read_exact(&mut methods).await.unwrap();
            assert!(methods.contains(&0x02));

            // Select username/password auth.
            client.write_all(&[0x05, 0x02]).await.unwrap();

            // Username/password auth request.
            let mut auth_head = [0u8; 2];
            client.read_exact(&mut auth_head).await.unwrap();
            assert_eq!(auth_head[0], 0x01);
            let ulen = auth_head[1] as usize;
            let mut uname = vec![0u8; ulen];
            client.read_exact(&mut uname).await.unwrap();
            let mut plen = [0u8; 1];
            client.read_exact(&mut plen).await.unwrap();
            let plen = plen[0] as usize;
            let mut passwd = vec![0u8; plen];
            client.read_exact(&mut passwd).await.unwrap();

            assert_eq!(std::str::from_utf8(&uname).unwrap(), "user");
            assert_eq!(std::str::from_utf8(&passwd).unwrap(), "pass");

            // Auth success.
            client.write_all(&[0x01, 0x00]).await.unwrap();

            // CONNECT request.
            let mut req_head = [0u8; 4];
            client.read_exact(&mut req_head).await.unwrap();
            assert_eq!(req_head[0], 0x05); // VER
            assert_eq!(req_head[1], 0x01); // CMD=CONNECT
            assert_eq!(req_head[2], 0x00); // RSV
            assert_eq!(req_head[3], 0x01); // ATYP=IPv4

            let mut dst_ip = [0u8; 4];
            client.read_exact(&mut dst_ip).await.unwrap();
            let mut dst_port = [0u8; 2];
            client.read_exact(&mut dst_port).await.unwrap();
            let port = u16::from_be_bytes(dst_port);

            let dest = std::net::SocketAddr::from((std::net::Ipv4Addr::from(dst_ip), port));
            assert_eq!(dest, target_addr);

            let mut server = TcpStream::connect(dest).await.unwrap();

            // Reply: success with bind addr 0.0.0.0:0
            client
                .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                .await
                .unwrap();

            // Relay one round-trip.
            let mut relay_buf = [0u8; 64];
            let n = client.read(&mut relay_buf).await.unwrap();
            server.write_all(&relay_buf[..n]).await.unwrap();
            let n = server.read(&mut relay_buf).await.unwrap();
            client.write_all(&relay_buf[..n]).await.unwrap();
        });

        let cfg = EgressProxyConfig {
            protocol: EgressProxyProtocol::Socks5,
            host: proxy_addr.ip().to_string(),
            port: proxy_addr.port(),
            username: Some("user".to_string()),
            password: Some("pass".to_string()),
        };

        let mut stream = connect_to_host_port(Some(&cfg), "127.0.0.1", target_addr.port())
            .await
            .unwrap();

        stream.write_all(b"ping").await.unwrap();
        let mut out = [0u8; 4];
        timeout(Duration::from_secs(1), stream.read_exact(&mut out))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&out, b"ping");

        proxy_task.await.unwrap();
        target_task.await.unwrap();
    }
}
