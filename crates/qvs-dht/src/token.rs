use std::net::SocketAddr;
use std::time::Instant;

use sha1::{Digest, Sha1};

pub struct TokenManager {
    secret_a: [u8; 16],
    secret_b: [u8; 16],
    last_rotation: Instant,
}

impl TokenManager {
    pub fn new() -> Self {
        let mut secret_a = [0u8; 16];
        let mut secret_b = [0u8; 16];
        rand::Rng::fill(&mut rand::thread_rng(), &mut secret_a);
        rand::Rng::fill(&mut rand::thread_rng(), &mut secret_b);
        Self {
            secret_a,
            secret_b,
            last_rotation: Instant::now(),
        }
    }

    pub fn generate_token(&self, addr: &SocketAddr) -> [u8; 4] {
        self.generate_with_secret(addr, &self.secret_a)
    }

    fn generate_with_secret(&self, addr: &SocketAddr, secret: &[u8; 16]) -> [u8; 4] {
        let mut hasher = Sha1::new();
        hasher.update(addr.ip().to_string().as_bytes());
        hasher.update(b":");
        hasher.update(addr.port().to_string().as_bytes());
        hasher.update(b":");
        hasher.update(secret);
        let result = hasher.finalize();
        let mut token = [0u8; 4];
        token.copy_from_slice(&result[..4]);
        token
    }

    pub fn verify_token(&self, addr: &SocketAddr, token: &[u8; 4]) -> bool {
        let current = self.generate_token(addr);
        if &current == token {
            return true;
        }
        let previous = self.generate_with_secret(addr, &self.secret_b);
        &previous == token
    }

    pub fn rotate_secret(&mut self) {
        self.secret_b = self.secret_a;
        rand::Rng::fill(&mut rand::thread_rng(), &mut self.secret_a);
        self.last_rotation = Instant::now();
    }

    pub fn check_rotation(&mut self) {
        if self.last_rotation.elapsed().as_secs() >= 600 {
            self.rotate_secret();
        }
    }
}

impl Default for TokenManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn test_addr() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), 8621)
    }

    #[test]
    fn test_generate_and_verify() {
        let mgr = TokenManager::new();
        let addr = test_addr();
        let token = mgr.generate_token(&addr);
        assert!(mgr.verify_token(&addr, &token));
    }

    #[test]
    fn test_reject_wrong_token() {
        let mgr = TokenManager::new();
        let addr = test_addr();
        let wrong = [0u8; 4];
        assert!(!mgr.verify_token(&addr, &wrong));
    }

    #[test]
    fn test_rotate() {
        let mut mgr = TokenManager::new();
        let addr = test_addr();
        let old_token = mgr.generate_token(&addr);
        mgr.rotate_secret();
        let new_token = mgr.generate_token(&addr);
        assert_ne!(old_token, new_token);
        // old token still verifiable via secret_b
        assert!(mgr.verify_token(&addr, &old_token));
    }
}
