//! 可选的云端客户端实现。
//!
//! 仅在启用 `websocket` 特性时编译。当前只包含 WebSocket 客户端的占位实现；
//! 主传输通道仍为 HTTP，由 [`crate::sync`] 同步引擎内部直接实现。

pub mod websocket;
