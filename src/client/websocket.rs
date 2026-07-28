//! WebSocket 云端客户端（占位实现）。
//!
//! `websocket` 特性当前仅保留依赖与模块名，真正的流式上报尚未实现；今日支持的
//! 传输通道是 [`crate::sync`] 中的 HTTP 批量上报。本模块先行落地 API 形状，待实现
//! 到位时可平滑替换。

use anyhow::bail;

/// 尚未实现的 WebSocket 云端客户端。
///
/// 仅用于固定未来实现的 API 形状；当前所有写操作都会返回 [`anyhow::Error`]。
#[derive(Debug)]
pub struct WebSocketClient {
    endpoint: String,
}

impl WebSocketClient {
    /// 创建一个指向 `endpoint` 的占位客户端。
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }

    /// 已配置的 WebSocket 端点。
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// 推送一批记录。**尚未实现。**
    pub async fn push_batch(&self, _records: &[serde_json::Value]) -> anyhow::Result<()> {
        bail!(
            "WebSocket upload to {} is not implemented yet; use the HTTP sync path",
            self.endpoint
        );
    }
}
