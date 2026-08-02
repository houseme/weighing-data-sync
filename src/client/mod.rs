//! 可选的云端客户端实现。
//!
//! 仅在启用 `websocket` 特性时编译。当前提供批量 JSON 上报与服务端 ACK 解析；
//! 主生产传输通道仍为 HTTP，由 [`crate::sync`] 同步引擎内部直接实现。

pub mod websocket;
