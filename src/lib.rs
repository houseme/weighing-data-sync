//! **weighing-data-sync** — 可靠的称重数据同步守护进程。
//!
//! 从单机 Windows 环境读取地磅称重记录，按“本地先持久化、网络可用后批量补传”的原则
//! 同步到云端，适用于工厂、港口、矿山、粮仓等无人值守现场。
//!
//! # 两条同步链路
//!
//! - **SQL Server（默认生产链路）**：通过 [`source::sqlserver`] 从
//!   `tbl_weightInfo` 读取 `isUploadCloud = 0` 的记录，批量上报云端，成功后回写
//!   `isUploadCloud = 1`。引擎见 [`sync::sqlserver_engine`]。
//! - **本地缓存（备选链路）**：通过 [`db`] 的 SeaORM 2.0 + SQLite 缓存待同步记录，
//!   批量上报后标记 `synced` / `failed`。引擎见 [`sync::engine`]。
//!
//! 两种引擎都返回统一的 [`sync::SyncOutcome`]，便于 CLI 结构化日志输出。
//!
//! # 特性（Cargo features）
//!
//! - `sqlite` / `http` / `cron`：默认启用，保证非 Windows 开发机可直接编译。
//! - `compression`：gzip 压缩上报请求体。
//! - `websocket`：WebSocket 批量上报客户端入口（见 [`client`]）。
//! - `windows`：Windows 服务安装与注册表自启动（仅 `cfg(windows)`）。
//!
//! 配置详见 [`config::AppConfig`]，HTTP 接收服务详见 [`server`]。

pub mod config;
pub mod db;
pub mod entity;
pub mod server;
pub mod source;
pub mod sync;
pub mod windows;

/// 可选的云端客户端实现（WebSocket 等）。
///
/// 仅在启用 `websocket` 特性时编译。当前提供批量 JSON 上报与服务端 ACK 解析；
/// 主生产传输通道仍为 HTTP，由同步引擎内部直接实现。
#[cfg(feature = "websocket")]
pub mod client;
