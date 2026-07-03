use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};

#[derive(Debug, Clone)]
pub struct LocalServerConfig {
    pub preferred_port: u16,
    pub max_retry: u8,
    /// IP address to bind the server to.
    /// Use `0.0.0.0` to accept connections from all interfaces (required for
    /// remote GUI clients); use `127.0.0.1` (default) for local-only access.
    pub bind_address: IpAddr,
}

impl Default for LocalServerConfig {
    fn default() -> Self {
        Self {
            preferred_port: 8621,
            max_retry: 10,
            bind_address: IpAddr::V4(Ipv4Addr::LOCALHOST),
        }
    }
}

impl LocalServerConfig {
    #[must_use]
    pub fn new(preferred_port: u16) -> Self {
        Self {
            preferred_port,
            max_retry: 10,
            bind_address: IpAddr::V4(Ipv4Addr::LOCALHOST),
        }
    }

    /// Set a custom bind address (builder-style).
    #[must_use]
    pub fn with_bind_address(mut self, addr: IpAddr) -> Self {
        self.bind_address = addr;
        self
    }

    #[must_use]
    pub fn port_available(port: u16) -> bool {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
        TcpListener::bind(addr).is_ok()
    }

    #[must_use]
    pub fn find_available_port(&self) -> u16 {
        if Self::port_available(self.preferred_port) {
            return self.preferred_port;
        }
        for offset in 1..=self.max_retry {
            let port = self.preferred_port + u16::from(offset);
            if Self::port_available(port) {
                return port;
            }
        }
        0
    }

    pub async fn find_available_port_async(&self) -> Option<u16> {
        for offset in 0..=self.max_retry {
            let port = self.preferred_port + u16::from(offset);
            if port_available_async(self.bind_address, port).await {
                return Some(port);
            }
        }
        None
    }
}

async fn port_available_async(bind_addr: IpAddr, port: u16) -> bool {
    let addr = SocketAddr::new(bind_addr, port);
    tokio::net::TcpListener::bind(addr).await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = LocalServerConfig::default();
        assert_eq!(config.preferred_port, 8621);
    }

    #[test]
    fn test_port_available() {
        let result = LocalServerConfig::port_available(9876);
        assert!(result);
    }

    #[test]
    fn test_find_available_port() {
        let config = LocalServerConfig::new(9877);
        let port = config.find_available_port();
        assert!(port > 0);
    }
}
