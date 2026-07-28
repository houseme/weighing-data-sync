# 架构

## 两条同步链路

`sync.source` 选择数据源（默认 `sqlserver`）。两条链路共用同一云端上报协议与重试策略，
最终都产出统一的 [`SyncOutcome`](#同步结果同步结果) 供 CLI 日志输出。

```text
                 ┌───────────────────────┐
   tbl_weightInfo│  source/sqlserver.rs  │  (tiberius, SELECT TOP ...)
   isUploadCloud │  fetch_pending()      │
        = 0      └──────────┬────────────┘
                              │  Vec<SqlServerWeightRecord{serial_no, data}>
                              ▼
                    ┌─────────────────────┐    backoff retry
                    │ sqlserver_engine.rs │ ───────────────▶  POST /weighing-data-sync/put
                    │  upload_batch()     │ ◀──────────────  (204/2xx/4xx/5xx)
                    └──────────┬──────────┘
                               │  accepted_serial_nos
                               ▼
                    UPDATE tbl_weightInfo SET isUploadCloud = 1 WHERE serialNo IN (...)
                    （mark_uploaded，1000 条分块，mark_uploaded = true 时）

另一种链路（sync.source = "local"）：

   SQLite weighing_records          ┌─────────────────┐
   status IN ('pending','failed') ─▶│  db/mod.rs      │  fetch_pending()
                                   └────────┬────────┘
                                            ▼
                                  ┌─────────────────┐   backoff retry
                                  │  sync/engine.rs │ ────────────▶  POST /.../put
                                  │  upload_batch() │ ◀────────────
                                  └────────┬────────┘
                                           │  accepted_ids / failed_ids
                                           ▼
                                  bulk UPDATE weighing_records
                                  SET status='synced'|'failed', retry_count=retry_count+1
```

## 模块职责

| 模块 | 职责 |
| --- | --- |
| `config` | 配置分层加载（TOML + `WDS__*`），`SqlServerConfig::validate()` 守卫占位符凭据 |
| `db` | SeaORM 2.0 SQLite：连接、迁移、批量 CRUD、`inbound_payloads` 落库 |
| `db::models` | `WeighingRecord` 领域模型、`WeightUnit`/`SyncStatus`、uom kg/lb 换算 |
| `entity` | SeaORM Entity：本地 `weighing_records`，以及 `tbl_weightInfo`/`tbl_weightPhoto` 的表结构映射（迁移基础） |
| `source::sqlserver` | tiberius 读取 `tbl_weightInfo`（动态类型 → JSON）、批量回写、SQL 注入防护 |
| `sync::engine` | 本地缓存 → 云端 同步引擎 |
| `sync::sqlserver_engine` | SQL Server → 云端 同步引擎 |
| `sync::error` | `UploadError`：permanent / transient 分类 |
| `sync`（mod） | `SyncOutcome` 统一结果类型 |
| `server` | axum HTTP 接收服务：Bearer 鉴权、空体 400、可选 `inbound_payloads` 落库 |
| `client`（websocket 特性） | WebSocket 客户端占位 |
| `windows` | 注册表 Run 项自启动、Windows 服务安装/卸载（`cfg(windows)` + `windows` 特性） |

## 同步结果（SyncOutcome）

两个引擎的 `sync_once()` 都返回统一的 `SyncOutcome`，`main.rs` 的 `AppEngine` 枚举分发
（`Local` / `SqlServer`）直接透传该类型，避免此前 `serde_json::Value` 抹平带来的类型信息丢失：

```rust
pub struct SyncOutcome {
    pub fetched: usize,                 // 本批拉取数
    pub synced: usize,                  // 云端确认数
    pub failed: usize,                  // 云端拒绝数
    pub marked_uploaded: Option<bool>,  // 是否回写源（仅 SQL Server 链路）
    pub no_data: bool,                  // 源无待同步数据
}
```

## 重试错误分类

`upload_batch` 返回 `Result<_, UploadError>`，`UploadError` 区分：

- `Transient`：网络错误、请求发送失败、`5xx`、`408`、`429`、（读取响应体的）I/O 错误。
- `Permanent`：`4xx`（除 408/429）、响应反序列化失败、构建请求体失败。

`UploadError::into_backoff()` 映射到 `backoff::Error::transient` / `permanent`：
permanent 错误被 backoff 立即返回（不重试），transient 错误按指数退避重试直到预算耗尽。
`classify` 规则在 `src/sync/error.rs`，并有单元测试覆盖。

## 写回优化

- 本地缓存 `mark_synced` / `mark_failed` 使用 SeaORM `update_many()` 单条
  `UPDATE ... WHERE id IN (...)`，避免逐行 load-and-save 的 N 次往返；
  `mark_failed` 通过 `retry_count = retry_count + 1` 原子自增。
- SQL Server `mark_uploaded` 使用参数化 `WHERE [serialNo] IN (@P1, ...)` 批量更新，
  按 1000 条分块，规避 TDS 2100 参数上限，替代此前的逐行 UPDATE。

## SeaORM-X 限制与 tiberius 方案

开源 SeaORM 2.0 覆盖 SQLite/MySQL/PostgreSQL；SQL Server 一等支持属于 SeaORM X 商业版能力。
本项目保持“本地缓存用开源 SeaORM 2.0 + SQLite、SQL Server 源读取用 `tiberius`”的可编译方案。

- `src/source/sqlserver.rs` 用 `tiberius` 原生 TDS 读取，动态把 `ColumnData` 转为 JSON。
- `src/entity/weight_info.rs` 与 `src/entity/weight_photo.rs` 是 `tbl_weightInfo` / `tbl_weightPhoto`
  的表结构映射，目前作为**迁移基础**保留（开源 SeaORM 2.0 无 SQL Server 后端，暂不直接查询）。
  若生产授权 SeaORM X，可把它们直接作为迁移基础，把读取通道切换到 `mssql://` 连接。

## 安全设计要点

- 现场凭据不随仓库分发，`validate()` 守卫占位符与空值。
- SQL Server 表名通过 `validate_identifier` 校验（仅字母/数字/`_`/`.`），列名用 `[]` 包裹并
  转义 `]`，参数化查询，防 SQL 注入。
- HTTP 接收服务可选 Bearer 鉴权与请求体上限。
- 客户端用 rustls（无 OpenSSL 依赖），`reqwest` / `tiberius` 均启用 rustls。
