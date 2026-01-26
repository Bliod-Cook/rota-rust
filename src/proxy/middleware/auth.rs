//! Proxy authentication middleware
//!
//! Handles Basic authentication for the proxy server.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use hyper::header::{PROXY_AUTHENTICATE, PROXY_AUTHORIZATION};
use hyper::{Request, Response, StatusCode};
use tracing::{debug, warn};

use crate::error::{Result, RotaError};

/// Proxy authentication handler
#[derive(Clone)]
pub struct ProxyAuth {
    /// Whether authentication is enabled
    enabled: bool,
    /// Expected username
    username: String,
    /// Expected password
    password: String,
}

impl ProxyAuth {
    /// Create a new proxy auth handler
    pub fn new(enabled: bool, username: String, password: String) -> Self {
        Self {
            enabled,
            username,
            password,
        }
    }

    /// Create a disabled auth handler
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            username: String::new(),
            password: String::new(),
        }
    }

    /// Check if authentication is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Validate the Proxy-Authorization header
    pub fn validate<T>(&self, req: &Request<T>) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let auth_header = req
            .headers()
            .get(PROXY_AUTHORIZATION)
            .ok_or(RotaError::AuthenticationFailed)?;

        let auth_str = auth_header
            .to_str()
            .map_err(|_| RotaError::AuthenticationFailed)?;

        // Parse "Basic <base64>"
        if !auth_str.starts_with("Basic ") {
            warn!("Invalid auth scheme, expected Basic");
            return Err(RotaError::AuthenticationFailed);
        }

        let encoded = &auth_str[6..];
        let decoded = BASE64
            .decode(encoded)
            .map_err(|_| RotaError::AuthenticationFailed)?;

        let credentials = String::from_utf8(decoded).map_err(|_| RotaError::AuthenticationFailed)?;

        let (user, pass) = credentials
            .split_once(':')
            .ok_or(RotaError::AuthenticationFailed)?;

        if user == self.username && pass == self.password {
            debug!("Proxy authentication successful for user: {}", user);
            Ok(())
        } else {
            warn!("Proxy authentication failed for user: {}", user);
            Err(RotaError::AuthenticationFailed)
        }
    }

    /// Create a 407 Proxy Authentication Required response
    pub fn challenge_response<T>(&self) -> Response<T>
    where
        T: Default,
    {
        Response::builder()
            .status(StatusCode::PROXY_AUTHENTICATION_REQUIRED)
            .header(PROXY_AUTHENTICATE, "Basic realm=\"Proxy\"")
            .body(T::default())
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http_body_util::Full;

    fn create_request_with_auth(auth: Option<&str>) -> Request<Full<Bytes>> {
        let mut builder = Request::builder().uri("http://example.com/");

        if let Some(auth_value) = auth {
            builder = builder.header(PROXY_AUTHORIZATION, auth_value);
        }

        builder.body(Full::new(Bytes::new())).unwrap()
    }

    #[test]
    fn test_auth_disabled() {
        let auth = ProxyAuth::disabled();
        let req = create_request_with_auth(None);
        assert!(auth.validate(&req).is_ok());
    }

    #[test]
    fn test_auth_missing_header() {
        let auth = ProxyAuth::new(true, "user".to_string(), "pass".to_string());
        let req = create_request_with_auth(None);
        assert!(matches!(
            auth.validate(&req),
            Err(RotaError::AuthenticationFailed)
        ));
    }

    #[test]
    fn test_auth_valid_credentials() {
        let auth = ProxyAuth::new(true, "user".to_string(), "pass".to_string());
        let credentials = BASE64.encode(b"user:pass");
        let req = create_request_with_auth(Some(&format!("Basic {}", credentials)));
        assert!(auth.validate(&req).is_ok());
    }

    #[test]
    fn test_auth_invalid_credentials() {
        let auth = ProxyAuth::new(true, "user".to_string(), "pass".to_string());
        let credentials = BASE64.encode(b"wrong:wrong");
        let req = create_request_with_auth(Some(&format!("Basic {}", credentials)));
        assert!(matches!(
            auth.validate(&req),
            Err(RotaError::AuthenticationFailed)
        ));
    }

    #[test]
    fn test_auth_invalid_scheme() {
        let auth = ProxyAuth::new(true, "user".to_string(), "pass".to_string());
        let req = create_request_with_auth(Some("Bearer token123"));
        assert!(matches!(
            auth.validate(&req),
            Err(RotaError::AuthenticationFailed)
        ));
    }
}
