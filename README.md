# 称重数据同步系统（weighing-data-sync）

安徽尚恒科技有限公司智能称重系统的核心同步子系统：把单机 Windows 环境中产生的
地磅（weighbridge）称重记录，可靠地同步到云端。按“本地先持久化、网络可用后批量补传”
的原则设计，适用于工厂、港口、矿山、粮仓等无人值守现场。

- 默认从 SQL Server `tbl_weightInfo` 读取 `isUploadCloud = 0` 的记录，HTTP 批量上报，
  成功后回写 `isUploadCloud = 1`。
- 备选本地 SQLite 缓存链路（SeaORM 2.0），网络中断时数据不丢失。
- 指数退避重试（**4xx 不再无谓消耗重试预算**）、cron 定时调度、JSON 结构化日志、
  uom 千克/磅类型安全换算、Windows 服务/注册表自启动。

## 快速开始

```bash
cargo fmt --all && cargo check
cargo run -- run --config config/default.toml          # 守护进程（默认 SQL Server 链路）
WDS__SYNC__SOURCE=local cargo run -- sync-now           # 用本地缓存链路手动同步一次
cargo run -- serve --config config/default.toml         # 启动 HTTP 接收服务
```

> ⚠️ 现场凭据（SQL Server 主机/账号/密码）**不再随仓库分发**。请通过
> `config/default.toml` 或 `WDS__SQLSERVER__*` 环境变量注入，详见
> [配置说明](docs/configuration.md#凭据注入)。

## 文档索引

完整文档位于 [`docs/`](docs/) 目录：

| 文档 | 内容 |
| --- | --- |
| [总览](docs/index.md) | 系统定位、目录结构、文档导航 |
| [快速开始](docs/getting-started.md) | 构建、运行、`run` / `sync-now` / `serve`、本地验证 |
| [配置说明](docs/configuration.md) | 全部配置项、`WDS__*` 环境变量覆盖、凭据注入、重试 / cron 调优 |
| [Windows 部署](docs/deployment-windows.md) | 目录约定、交叉编译、服务安装 / 卸载、登录自启动、凭据管理 |
| [HTTP 协议](docs/api-protocol.md) | 上报请求 / 响应 schema、Bearer 鉴权、204 / 空体语义 |
| [日志与排障](docs/logging.md) | 结构化日志与 `stage` 目录、示例、排障流程 |
| [架构](docs/architecture.md) | 两条同步链路、模块职责、SeaORM-X 限制与 tiberius 方案 |
| [常见问题](docs/troubleshooting.md) | SQL Server 连接失败、云端 4xx、占位符凭据、队头阻塞 |

## 技术栈

Rust Edition 2024 · tokio · axum · reqwest · SeaORM 2.0 (SQLite) · tiberius (SQL Server) ·
config + dotenvy · clap · tokio-cron-scheduler · backoff · tracing · uom

## 许可证

MIT OR Apache-2.0，见 [`LICENSE-MIT`](LICENSE-MIT) 与 [`LICENSE-APACHE`](LICENSE-APACHE)。
