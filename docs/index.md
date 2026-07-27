# 称重数据同步系统

本文档是安徽尚恒科技有限公司“称重数据同步系统”的工程交付索引。当前仓库已经落地一套可编译运行的 Rust Edition 2024 单机同步服务骨架，默认从 SQL Server `yunfu.dbo.tbl_weightInfo` 读取地磅数据并通过 JSON 上报到 `https://api.xmb.xyz/weighing-data-sync/put`，同时保留 SeaORM 2.0 + SQLite 本地缓存模式、HTTP 批量上传、指数退避、cron 定时调度、JSON 结构化日志、uom 重量单位转换，以及 Windows 服务/注册表自启动的条件编译入口。

## 1. 项目定位

称重数据同步系统是尚恒智能称重系统的核心子系统，负责把单机 Windows 环境中产生的称重记录可靠同步到云端。系统按“本地先持久化，网络可用后批量补传”的原则设计，适合工厂、港口、矿山、粮食仓储等无人值守现场。

核心目标：

- 从 SQL Server `tbl_weightInfo` 读取 `isUploadCloud = 0` 且未删除的地磅称重记录。
- HTTP REST API 批量上传待同步记录，服务端确认后可回写 `isUploadCloud = 1` 防止重复上报。
- 使用 SeaORM 2.0 管理本地 SQLite 持久化缓存，网络中断时数据不丢失。
- 使用指数退避重试，初始 1 秒，最大 60 秒，默认重试预算 300 秒。
- 使用 cron 表达式调度，默认每 5 分钟同步一次。
- 使用 tracing 输出 JSON 日志，便于 Windows 事件采集、日志平台和现场排障。
- 使用 uom 做千克/磅类型安全转换，避免单位混用。
- Windows 侧支持服务安装入口与用户登录自启动 Run 项入口。

## 2. 完整 Cargo 配置

完整配置文件位于仓库根目录 `Cargo.toml`。关键设计如下：

- `edition = "2024"`，`rust-version = "1.95"`。
- 默认特性：`sqlite`、`http`、`cron`，保证非 Windows 开发机也能直接编译。
- Windows 部署启用：`--features windows`。
- 可选 gzip 请求体压缩启用：`--features compression`。
- 可选 WebSocket 依赖入口启用：`--features websocket`。
- Windows 专用依赖放在 `target.'cfg(windows)'.dependencies` 下，避免 macOS/Linux 编译被 `winreg` 或 `windows-service` 阻塞。

常用依赖：

| 能力 | Crate |
| --- | --- |
| HTTP 服务端 | `axum` |
| 异步运行时 | `tokio` |
| HTTP 客户端 | `reqwest` |
| 本地数据库 | `sea-orm` 2.0 + SQLite |
| SQL Server 读取 | `tiberius` |
| 配置 | `config` + `dotenvy` |
| CLI | `clap` |
| 定时调度 | `tokio-cron-scheduler` |
| 重试 | `backoff` |
| 日志 | `tracing` + `tracing-subscriber` |
| Decimal 保真 | `rust_decimal` |
| 重量单位 | `uom` |
| Windows 服务 | `windows-service` |
| 注册表自启动 | `winreg` |

## 3. 项目目录结构

当前已落地结构：

```text
weighing-data-sync/
├── .gitignore
├── Cargo.lock
├── Cargo.toml
├── config/
│   └── default.toml
├── docs/
│   ├── index.md
│   ├── mysql_sync_schema.sql
│   ├── sync_field_mapping.md
│   └── sync_structs.rs
└── src/
    ├── lib.rs
    ├── main.rs
    ├── config.rs
    ├── server.rs
    ├── entity/
    │   ├── mod.rs
    │   ├── weighing_record.rs
    │   ├── weight_info.rs
    │   └── weight_photo.rs
    ├── source/
    │   ├── mod.rs
    │   └── sqlserver.rs
    ├── db/
    │   ├── mod.rs
    │   └── models.rs
    ├── sync/
    │   ├── mod.rs
    │   ├── engine.rs
    │   └── sqlserver_engine.rs
    └── windows/
        ├── mod.rs
        ├── autostart.rs
        └── service.rs
```

建议后续生产化扩展结构：

```text
weighing-data-sync/
├── migrations/
│   └── 20260718000000_init.sql
├── src/
│   ├── client/
│   │   ├── http.rs
│   │   └── websocket.rs
│   ├── scheduler/
│   │   └── mod.rs
│   └── hardware/
│       ├── serial.rs
│       └── usb.rs
├── tests/
│   └── sync_engine_test.rs
└── scripts/
    ├── install-service.ps1
    └── uninstall-service.ps1
```

## 4. 核心模块实现

核心代码已经写入真实源码文件：

- `src/main.rs`：CLI 入口、JSON 日志初始化、服务运行、手动同步、cron 调度、Windows 命令分发。
- `src/config.rs`：`AppConfig`、`ApiConfig`、`DatabaseConfig`、`SqlServerConfig`、`SyncConfig`，支持 TOML 文件和 `WDS__...` 环境变量覆盖。
- `src/server.rs`：基于 `axum` 的 HTTP 接收服务，提供 `POST /weighing-data-sync/put`，接收 JSON 上报并写入结构化日志。
- `src/source/sqlserver.rs`：使用 `tiberius` 连接 SQL Server，读取 `tbl_weightInfo` 文档字段，动态转换为 JSON，成功后可按 `serialNo` 回写 `isUploadCloud = 1`。
- `src/entity/weighing_record.rs`：本地 SQLite 缓存表 `weighing_records` 的 SeaORM Entity。
- `src/entity/weight_info.rs`：SQL Server `tbl_weightInfo` 的 SeaORM Entity/表结构映射，字段使用 Rust snake_case，列名用 `column_name` 显式对齐原表。
- `src/entity/weight_photo.rs`：SQL Server `tbl_weightPhoto` 的 SeaORM Entity/表结构映射，保留 `serialNo` 关联和 `captureImage` 二进制字段。
- `src/db/models.rs`：称重记录模型、同步状态、重量单位，使用 `uom::si::f64::Mass` 做千克/磅转换。
- `src/db/mod.rs`：SeaORM 2.0 `DatabaseConnection`、本地 SQLite 表初始化、插入待同步记录、查询待同步记录、标记成功/失败。
- `src/sync/engine.rs`：同步引擎，批量读取待同步记录，HTTP 上传，指数退避重试，成功后标记 synced，失败后累加 retry_count。
- `src/sync/sqlserver_engine.rs`：SQL Server 同步引擎，批量读取 `tbl_weightInfo`，上报 JSON，2xx 成功后按响应或整批标记已上传。
- `src/windows/autostart.rs`：Windows 注册表 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 写入、删除、查询。
- `src/windows/service.rs`：Windows 服务安装/卸载入口；非 Windows 平台返回明确错误。
- `docs/sync_structs.rs`：完整同步 DTO 结构体，覆盖 `tbl_weightInfo`、`tbl_weightPhoto` 和本地缓存表。
- `docs/mysql_sync_schema.sql`：高性能 MySQL 建表、索引、批量 upsert 和待同步扫描 SQL。
- `docs/sync_field_mapping.md`：所有结构体字段的 JSON/Rust/MySQL 映射与同步处理规则。

### 4.1 配置示例

`config/default.toml`：

```toml
[api]
endpoint = "https://api.xmb.xyz/weighing-data-sync/put"
timeout_seconds = 30

[database]
url = "sqlite://data/weighing_sync.db"
max_connections = 5

[server]
enabled = false
bind = "0.0.0.0:8080"
route = "/weighing-data-sync/put"
max_body_bytes = 16777216

[sqlserver]
host = "192.168.10.251"
port = 1433
database = "yunfu"
username = "sa"
password = "yusilong"
table = "tbl_weightInfo"
trust_cert = true
encryption = "off"
mark_uploaded = true

[sync]
source = "sqlserver"
batch_size = 100
cron = "0 */5 * * * *"
sync_on_start = true
retry_initial_delay_ms = 1000
retry_max_delay_ms = 60000
retry_max_elapsed_seconds = 300
```

环境变量覆盖示例：

```powershell
$env:WDS__API__ENDPOINT = "https://api.xmb.xyz/weighing-data-sync/put"
$env:WDS__API__API_KEY = "replace-with-production-token"
$env:WDS__SQLSERVER__HOST = "192.168.10.251"
$env:WDS__SQLSERVER__DATABASE = "yunfu"
$env:WDS__SQLSERVER__USERNAME = "sa"
$env:WDS__SQLSERVER__PASSWORD = "replace-with-secure-secret"
```

### 4.2 HTTP 上传协议

请求方向：

```http
POST /weighing-data-sync/put
Authorization: Bearer <api-key>
Content-Type: application/json
```

请求体：

```json
{
  "source": "sqlserver-yunfu-tbl_weightInfo",
  "database": "yunfu",
  "table": "tbl_weightInfo",
  "uploaded_at": "2026-07-18T08:00:00Z",
  "records": [
    {
      "serialNo": "202607180001",
      "plateNo": "皖H12345",
      "weightType": "销售",
      "goodsName": "砂石",
      "grossWeight": "12500.00",
      "tareWeight": "5000.00",
      "netWeight": "7500.00",
      "actualWeight": "7500.00",
      "weightUnit": "kg",
      "grossTime": "2026-07-18 08:00:00",
      "tareTime": "2026-07-18 08:10:00",
      "updateTime": "2026-07-18 08:10:30",
      "isUploadCloud": 0,
      "del_flag": 0
    }
  ]
}
```

响应体：

```json
{
  "request_id": "03825b1a-b925-4592-afd4-b982c5ce21de",
  "accepted": true,
  "accepted_serial_nos": ["202607180001"],
  "failed_serial_nos": [],
  "records_count": 1,
  "received_at": "2026-07-18T14:50:17.240835+00:00"
}
```

如果云端返回 `204 No Content` 或返回空 JSON，SQL Server 同步引擎会按“整批成功”处理，并在 `mark_uploaded = true` 时执行：

```sql
UPDATE [tbl_weightInfo] SET [isUploadCloud] = 1 WHERE [serialNo] = @P1
```

### 4.3 Word 实体文档对应关系

本次读取了两份实体类文档，并在 `src/entity/` 中创建了对应 SeaORM Entity：

- `称重信息实体类.docx`：确认 `tbl_weightInfo` 的主要字段，包括 `serialNo`、`plateNo`、`grossWeight`、`tareWeight`、`netWeight`、`actualWeight`、`grossTime`、`tareTime`、`updateTime`、`isUploadCloud`、`del_flag` 等。
- `称重图片实体类.docx`：确认图片表 `tbl_weightPhoto` 通过 `serialNo` 关联称重单，图片二进制字段为 `captureImage`。当前需求只要求上报 `tbl_weightInfo`，图片表暂未纳入默认载荷。

说明：开源 SeaORM 2.0 当前覆盖 SQLite/MySQL/PostgreSQL；SQL Server 一等支持属于 SeaORM X 商业版能力。当前项目保持“本地缓存使用开源 SeaORM 2.0、SQL Server 源读取使用 `tiberius`”的可编译方案；若生产授权 SeaORM X，可把 `src/entity/weight_info.rs` 和 `src/entity/weight_photo.rs` 直接作为迁移基础，将读取通道切换到 `mssql://` 连接。

## 5. 启动提示语设计

日志输出为 JSON，每条日志包含 `stage` 字段，用于现场定位阶段。

| 阶段 | `stage` | 信息 |
| --- | --- | --- |
| 启动开始 | `startup.begin` | 称重数据同步系统正在启动 |
| 配置加载 | `config.loaded` | 配置文件加载成功 |
| SQL Server 源 | `sqlserver.ready` | SQL Server 数据源已初始化 |
| 数据库就绪 | `database.ready` | 本地 SQLite 缓存数据库已就绪 |
| HTTP 客户端 | `client.ready` | 云端同步客户端已初始化 |
| 启动同步 | `sync.startup` | 启动同步完成或失败，失败后等待调度器重试 |
| 调度器 | `scheduler.ready` | 同步调度器已启动 |
| 服务就绪 | `startup.ready` | 服务已就绪，等待同步信号 |
| 定时同步 | `sync.scheduled` | 定时同步完成或失败 |
| 手动同步 | `sync.manual` | 手动同步开始和完成 |
| 重试 | `sync.retry` | 上传失败，准备按指数退避重试 |
| SQL Server 拉取 | `sqlserver.fetch` | 没有待上报的 SQL Server 称重记录 |
| SQL Server 上报 | `sqlserver.upload.*` | SQL Server 数据开始上报、重试、完成 |
| SQL Server 回写 | `sqlserver.mark_uploaded` | SQL Server 云上传状态已更新 |
| 接收服务启动 | `server.ready` | 称重数据接收服务已启动 |
| 接收服务上报 | `server.put` | 收到称重数据上报，记录摘要和完整 payload |
| 关闭 | `shutdown` | 收到停止信号，服务正在关闭 |
| 自启动 | `windows.autostart.*` | 用户登录自启动项写入、移除或查询 |
| 服务安装 | `windows.service.*` | Windows 服务安装或卸载 |

示例 JSON 日志：

```json
{"timestamp":"2026-07-18T08:00:00Z","level":"INFO","stage":"startup.begin","version":"0.1.0","message":"称重数据同步系统正在启动"}
{"timestamp":"2026-07-18T08:00:00Z","level":"INFO","stage":"config.loaded","path":"config/default.toml","message":"配置文件加载成功"}
{"timestamp":"2026-07-18T08:00:00Z","level":"INFO","stage":"database.ready","max_connections":5,"message":"本地 SQLite 缓存数据库已就绪"}
{"timestamp":"2026-07-18T08:00:00Z","level":"INFO","stage":"scheduler.ready","cron":"0 */5 * * * *","message":"同步调度器已启动"}
```

## 6. Windows 部署与安装指南

### 6.1 推荐目录

```text
C:\Program Files\WeighingDataSync\sync-daemon.exe
C:\ProgramData\WeighingDataSync\config\default.toml
C:\ProgramData\WeighingDataSync\data\weighing_sync.db
C:\ProgramData\WeighingDataSync\logs\
```

### 6.2 编译 Windows 版本

在 Windows 本机：

```powershell
cargo build --release --features windows
```

在 macOS/Linux 交叉准备目标：

```bash
rustup target add x86_64-pc-windows-msvc
cargo build --release --target x86_64-pc-windows-msvc --features windows
```

### 6.3 前台运行

```powershell
.\target\release\sync-daemon.exe run --config C:\ProgramData\WeighingDataSync\config\default.toml
```

### 6.4 手动同步一次

```powershell
.\target\release\sync-daemon.exe sync-now --config C:\ProgramData\WeighingDataSync\config\default.toml
```

### 6.5 用户登录自启动

写入注册表 Run 项：

```powershell
.\target\release\sync-daemon.exe enable-autostart
```

查询状态：

```powershell
.\target\release\sync-daemon.exe autostart-status
```

移除自启动：

```powershell
.\target\release\sync-daemon.exe disable-autostart
```

注册表位置：

```text
HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run
  WeighingDataSync = "<exe-path>" run
```

### 6.6 Windows 服务安装

以管理员身份运行 PowerShell：

```powershell
.\target\release\sync-daemon.exe install-service
```

卸载：

```powershell
.\target\release\sync-daemon.exe uninstall-service
```

生产环境建议：

- 服务账号使用专用低权限账号。
- 数据目录放在 `C:\ProgramData\WeighingDataSync\data`。
- 云端 API Key 使用环境变量或 Windows 凭据管理器注入，不写入源码。
- 现场部署前用真实 API 沙箱环境跑 `sync-now`。
- Windows 服务控制分发和事件日志上报应作为下一步生产化加固项单独验证。

## 7. 编译与运行命令

开发机默认检查：

```bash
cargo fmt --all
cargo check
```

检查所有可选特性：

```bash
cargo check --all-features
```

发布构建：

```bash
cargo build --release
```

启用 gzip 请求体压缩：

```bash
cargo build --release --features compression
```

启用 WebSocket 依赖入口：

```bash
cargo build --release --features websocket
```

启用 Windows 服务和注册表能力：

```bash
cargo build --release --features windows
```

运行：

```bash
cargo run -- run --config config/default.toml
cargo run -- sync-now --config config/default.toml
cargo run -- serve --config config/default.toml
```

本机开发验证可临时覆盖监听地址：

```bash
RUST_LOG=info WDS__SERVER__BIND=127.0.0.1:18080 cargo run -- serve --config config/default.toml
```

## 8. 已验证结果

当前在 macOS 开发环境完成以下校验：

```bash
cargo fmt --all
cargo check
cargo check --all-features
```

三项均已通过。

补充检查：

- `RUST_LOG=info WDS__SERVER__BIND=127.0.0.1:18080 cargo run -- serve --config config/default.toml` 已通过，并用 `curl` 验证 `POST /weighing-data-sync/put` 能返回 `accepted_serial_nos`，同时输出 `stage = "server.put"` 的 JSON 日志。
- `RUST_LOG=info WDS__SYNC__SOURCE=local cargo run -- sync-now --config config/default.toml` 已通过，能完成配置加载、SQLite 初始化和空队列同步路径。
- 默认 `sync.source = "sqlserver"` 会连接现场内网 `192.168.10.251:1433`，当前开发机未连接该现场网络，因此没有执行真实 SQL Server 拉取/接口上报。
- `cargo check --target x86_64-pc-windows-msvc --features windows` 未完成，原因是当前机器未安装 `x86_64-pc-windows-msvc` Rust 标准库目标；安装命令为 `rustup target add x86_64-pc-windows-msvc`。
