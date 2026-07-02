use crate::types::InfoHash;

#[derive(Debug, thiserror::Error)]
pub enum QvodError {
    #[error("网络错误: {0}")]
    Network(#[from] std::io::Error),

    #[error("协议错误: {0}")]
    Protocol(String),

    #[error("元数据解析失败")]
    MetadataParse,

    #[error("DHT 超时")]
    DhtTimeout,

    #[error("DHT 路由失败: {0}")]
    DhtRoutingFailed(String),

    #[error("Tracker 连接超时")]
    TrackerTimeout,

    #[error("Tracker 协议错误: {0}")]
    TrackerProtocol(String),

    #[error("资源不存在: {0}")]
    ResourceNotFound(InfoHash),

    #[error("没有可用的 peer")]
    NoPeers,

    #[error("NAT 穿透失败")]
    NatFailed,

    #[error("缓存空间不足")]
    CacheFull,

    #[error("缓存损坏: {0}")]
    CacheCorrupted(String),

    #[error("不支持的格式: {0}")]
    UnsupportedFormat(String),

    #[error("解码错误: {0}")]
    Decode(String),

    #[error("无效 URI: {0}")]
    InvalidUri(String),

    #[error("Bencode 错误: {0}")]
    Bencode(String),

    #[error("Piece 校验失败 index={index}")]
    PieceVerificationFailed {
        index: u32,
        expected: [u8; 20],
        got: [u8; 20],
    },

    #[error("达到最大连接数")]
    ConnectionLimitReached,

    #[error("超时: {0}")]
    Timeout(String),

    #[error("操作已取消")]
    Cancelled,

    #[error("服务器错误: {0}")]
    Server(String),
}

impl From<QvodError> for String {
    fn from(e: QvodError) -> String {
        e.to_string()
    }
}
