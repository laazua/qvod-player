use std::{sync::Arc, time::Duration};
use tokio::{net::TcpStream, sync::Mutex, time::timeout};

use qvs_core::{InfoHash, QvodError};

use crate::handshake::Handshake;
use crate::message::PeerMessage;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
const READ_TIMEOUT: Duration = Duration::from_secs(30);
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

pub struct TcpStreamManager {
    stream: Arc<Mutex<Option<TcpStream>>>,
    connected: bool,
}

impl TcpStreamManager {
    #[must_use]
    pub fn new() -> Self {
        Self {
            stream: Arc::new(Mutex::new(None)),
            connected: false,
        }
    }

    pub async fn connect(&mut self, addr: &std::net::SocketAddr) -> Result<(), QvodError> {
        let stream = timeout(HANDSHAKE_TIMEOUT, TcpStream::connect(addr))
            .await
            .map_err(|_| QvodError::Timeout("connect timeout".into()))?
            .map_err(|e| QvodError::Network(e))?;
        stream.set_nodelay(true).ok();
        *self.stream.lock().await = Some(stream);
        self.connected = true;
        Ok(())
    }

    pub async fn send_handshake(
        &self,
        info_hash: InfoHash,
        peer_id: [u8; 20],
    ) -> Result<(), QvodError> {
        let hs = Handshake::new(info_hash, peer_id);
        let encoded = hs.encode();
        self.write_all(&encoded).await
    }

    pub async fn receive_handshake(&self) -> Result<(InfoHash, [u8; 20], [u8; 8]), QvodError> {
        let mut pstrlen_buf = [0u8; 1];
        self.read_exact(&mut pstrlen_buf).await?;
        let pstrlen = pstrlen_buf[0] as usize;
        let total_len = 1 + pstrlen + 8 + 20 + 20;
        let mut buf = vec![0u8; total_len];
        buf[0] = pstrlen_buf[0];
        self.read_exact(&mut buf[1..]).await?;
        let hs = Handshake::decode(&buf)?;
        Ok((hs.info_hash, hs.peer_id, hs.reserved))
    }

    pub async fn send_message(&self, msg: &PeerMessage) -> Result<(), QvodError> {
        let encoded = msg.encode();
        self.write_all(&encoded).await
    }

    pub async fn send_raw(&self, data: &[u8]) -> Result<(), QvodError> {
        self.write_all(data).await
    }

    pub async fn read_message(&self) -> Result<PeerMessage, QvodError> {
        let mut len_buf = [0u8; 4];
        self.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;
        if len == 0 {
            return Err(QvodError::Protocol("keep-alive".into()));
        }
        let mut rest = vec![0u8; len];
        self.read_exact(&mut rest).await?;
        let mut full = Vec::with_capacity(4 + len);
        full.extend_from_slice(&len_buf);
        full.extend_from_slice(&rest);
        PeerMessage::decode(&full)
    }

    pub async fn read_keep_alive(&self) -> Result<bool, QvodError> {
        let mut buf = [0u8; 4];
        self.read_exact(&mut buf).await?;
        Ok(u32::from_be_bytes(buf) == 0)
    }

    pub async fn close(&mut self) {
        if let Some(mut stream) = self.stream.lock().await.take() {
            use tokio::io::AsyncWriteExt;
            stream.writable().await.ok();
            let _ = stream.shutdown().await;
        }
        self.connected = false;
    }

    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    async fn write_all(&self, data: &[u8]) -> Result<(), QvodError> {
        let mut stream_lock = self.stream.lock().await;
        let stream = stream_lock
            .as_mut()
            .ok_or_else(|| QvodError::Protocol("not connected".into()))?;
        timeout(
            WRITE_TIMEOUT,
            tokio::io::AsyncWriteExt::write_all(stream, data),
        )
        .await
        .map_err(|_| QvodError::Timeout("write timeout".into()))?
        .map_err(|e| QvodError::Network(e))
    }

    async fn read_exact(&self, buf: &mut [u8]) -> Result<(), QvodError> {
        let mut stream_lock = self.stream.lock().await;
        let stream = stream_lock
            .as_mut()
            .ok_or_else(|| QvodError::Protocol("not connected".into()))?;
        timeout(
            READ_TIMEOUT,
            tokio::io::AsyncReadExt::read_exact(stream, buf),
        )
        .await
        .map_err(|_| QvodError::Timeout("read timeout".into()))?
        .map_err(|e| QvodError::Network(e))?;
        Ok(())
    }
}

impl Default for TcpStreamManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let mgr = TcpStreamManager::new();
        assert!(!mgr.is_connected());
    }
}
