# 称重数据同步系统 · 文档总览

本文档是“智能称重系统”的工程交付索引。`weighing-data-sync` 是一套
Rust Edition 2024 单机同步守护进程（`sync-daemon`），把单机 Windows 环境中产生的地磅称重记录
可靠同步到云端，按“本地先持久化、网络可用后批量补传”的原则设计，适合工厂、港口、矿山、
粮仓等无人值守现场。

## 核心能力

- 从 SQL Server `tbl_weightInfo` 读取 `isUploadCloud = 0` 且未删除的称重记录，HTTP 批量上报，
  服务端确认后回写 `isUploadCloud = 1` 防止重复上报。
- 备选本地缓存链路：SeaORM 2.0 管理 SQLite，网络中断时数据不丢失。
- 指数退避重试（初始 1s、最大 60s、默认预算 300s）；**4xx 错误（除 408/429）为永久失败，
  立即返回，不再消耗重试预算**。
- cron 定时调度（默认每 5 分钟）；`sync_on_start` 启动即同步一次。
- tracing 输出 JSON 日志，带 `stage` 字段，便于事件采集与现场排障。
- uom 做千克/磅类型安全换算，避免单位混用。
- HTTP 接收服务支持可选 Bearer 鉴权与本地落库审计。
- Windows 侧支持服务安装入口与用户登录自启动 Run 项入口。

## 文档导航

| 文档 | 内容 |
| --- | --- |
| [快速开始](getting-started.md) | 构建、运行、子命令、本地快速验证 |
| [配置说明](configuration.md) | 配置项逐项说明、环境变量覆盖、凭据注入、调优 |
| [Windows 部署](deployment-windows.md) | EXE 打包、现场依赖、安装脚本、计划任务、SQL Server 前置确认 |
| [HTTP 协议](api-protocol.md) | 上报请求 / 响应、鉴权、204 / 空体语义 |
| [日志与排障](logging.md) | 结构化日志与 `stage` 目录、示例 |
| [架构](architecture.md) | 两条同步链路、模块职责、SeaORM-X 限制说明 |
| [SQL Server Docker E2E](sqlserver-docker-e2e.md) | 模拟 SQL Server 数据源、Compose 编排与端到端验收 |
| [SQL Server 转 MySQL SQL](sqlserver_to_mysql.sql) | 将 SQL Server 样本源表 DDL / DML 转为 MySQL 8.0 可执行脚本 |
| [常见问题](troubleshooting.md) | 连接失败、4xx、占位符凭据、队头阻塞 |

## 项目目录结构

```text
weighing-data-sync/
├── README.md
├── LICENSE-AGPL
├── Cargo.toml
├── config/
│   └── default.toml
├── docker/                     # SQL Server E2E Compose、样本数据、验收脚本
├── scripts/
│   └── validate_sqlserver_e2e.sh
├── data/                       # 运行期 SQLite（已 gitignore）
├── docs/                       # 本文档目录
└── src/
    ├── lib.rs                  # crate 文档与模块声明
    ├── main.rs                 # CLI 入口、日志、调度、子命令分发
    ├── config.rs               # AppConfig / Api / Database / Server / Sync / SqlServer
    ├── server.rs               # axum HTTP 接收服务（鉴权 + 可选落库）
    ├── client/                 # 可选 WebSocket 批量上报客户端（websocket 特性）
    ├── entity/
    │   ├── weighing_record.rs  # 本地 SQLite 缓存表 SeaORM Entity
    │   ├── weight_info.rs      # tbl_weightInfo 表结构映射（迁移基础）
    │   └── weight_photo.rs     # tbl_weightPhoto 表结构映射
    ├── db/
    │   ├── mod.rs              # SeaORM 连接、迁移、批量 CRUD、inbound 落库
    │   └── models.rs           # WeighingRecord / WeightUnit / SyncStatus（uom 换算）
    ├── source/
    │   └── sqlserver.rs        # tiberius 读取 tbl_weightInfo、批量回写
    ├── sync/
    │   ├── mod.rs              # SyncOutcome 统一结果
    │   ├── error.rs            # UploadError：permanent / transient 分类
    │   ├── engine.rs           # 本地缓存 → 云端 同步引擎
    │   └── sqlserver_engine.rs # SQL Server → 云端 同步引擎
    └── windows/
        ├── autostart.rs        # 注册表 Run 项自启动
        └── service.rs          # Windows 服务安装 / 卸载
```

## 安全约定

- 现场凭据**不随仓库分发**：`config/default.toml` 与 `src/config.rs` 默认值均为占位符，
  未配置真实凭据时启动会给出明确错误。详见 [配置说明 · 凭据注入](configuration.md#凭据注入)。
- 云端 API Key 与 SQL Server 密码通过环境变量或 Windows 安装脚本生成的受限 `.env` 注入，
  不写入源码或仓库配置模板。

## 编译与验证

```bash
cargo fmt --all && cargo check && cargo test
cargo check --all-features          # 覆盖 compression / websocket / windows 等特性
cargo clippy --all-targets --all-features -- -D warnings
scripts/validate_sqlserver_e2e.sh   # Docker SQL Server 端到端验证
```

具体运行命令见 [快速开始](getting-started.md)，Windows 部署见 [Windows 部署](deployment-windows.md)。
