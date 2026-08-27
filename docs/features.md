# 项目功能全景与实现清单

本文档基于当前仓库源码、配置模板、脚本和 Docker 夹具整理，目标是回答：

1. 项目当前实际提供了哪些功能；
2. 每项功能的入口、处理流程、数据和失败语义是什么；
3. 哪些能力已经接入默认运行路径，哪些只是可选模块或迁移基础；
4. 当前实现的边界和运维注意事项是什么。

## 1. 项目定位

`weighing-data-sync` 是一个 Rust Edition 2024 异步同步守护进程，二进制名称为
`sync-daemon`。它面向工厂、港口、矿山、粮仓等现场的地磅系统，把本地 SQL Server
中的称重记录批量发送到云端，并在云端成功确认后更新源记录状态。

项目的核心原则是“本地数据优先、网络恢复后补传”：

- 默认生产链路从 SQL Server `yunfu.dbo.tbl_weightInfo` 读取未上传记录；
- 备用链路使用 SQLite 缓存保存待同步记录；
- HTTP 接收端可以把完整入站批次原样写入 SQLite，形成接收审计和重放材料；
- 网络错误、超时、`5xx`、`408`、`429` 采用指数退避；不可通过重试修复的 `4xx` 立即失败。

默认运行路径是：

```text
SQL Server tbl_weightInfo
    │ 读取 isUploadCloud=0 且 del_flag=0 的记录
    ▼
sync-daemon（批量、鉴权、重试）
    │ HTTP POST JSON
    ▼
云端或本地 HTTP 接收端
    │ 成功确认
    ▼
SQL Server 回写 isUploadCloud=1
```

## 2. 功能总览

| 编号 | 功能 | 默认是否启用 | 当前状态 | 主要实现 |
| --- | --- | --- | --- | --- |
| 1 | 守护进程生命周期与 CLI | 是 | 已接入 | `src/main.rs` |
| 2 | TOML + 环境变量配置 | 是 | 已接入 | `src/config.rs` |
| 3 | SQL Server 数据源 | 是 | 默认生产链路 | `src/source/sqlserver.rs` |
| 4 | 本地 SQLite 缓存 | 是 | 备用同步链路 | `src/db/`、`src/sync/engine.rs` |
| 5 | HTTP 批量上传 | 是 | 两条同步链路共用 | `src/sync/*_engine.rs` |
| 6 | 重试与错误分类 | 是 | 已接入 | `src/sync/error.rs`、`retry.rs` |
| 7 | cron 定时同步 | 是（`cron` feature） | 默认每 5 分钟 | `src/main.rs` |
| 8 | HTTP 接收服务 | 否（可单独 `serve`） | 已接入 | `src/server.rs` |
| 9 | 入站批次 SQLite 审计 | 否 | 接收服务可选 | `server.persist`、`inbound_payloads` |
| 10 | WebSocket 批量客户端 | 否（`websocket` feature） | 独立可复用 API，未接入引擎 | `src/client/websocket.rs` |
| 11 | gzip 请求体 | 否（`compression` feature） | 独立编译选项 | 两个同步引擎 |
| 12 | Windows 计划任务部署 | 不适用 | 推荐现场方案 | `scripts/install-windows.ps1` |
| 13 | Windows Run 项自启动 | 不适用 | 可选入口 | `src/windows/autostart.rs` |
| 14 | Windows 原生服务注册 | 不适用 | 保留入口，非推荐路径 | `src/windows/service.rs` |
| 15 | SQL Server Docker E2E | 不适用 | 开发/验收夹具 | `docker/`、`scripts/` |
| 16 | `tbl_weightPhoto` 映射 | 不适用 | 结构映射/迁移基础，当前未读取 | `src/entity/weight_photo.rs` |

## 3. CLI 与进程生命周期

### 3.1 全局参数

```text
sync-daemon [--config <路径>] <子命令>
```

- `--config` 默认是 `config/default.toml`；也可以使用 `WDS_CONFIG`。
- 程序启动时尝试加载当前工作目录下的 `.env`，然后初始化 JSON tracing 日志。
- 未指定子命令时等价于 `run`。

### 3.2 子命令逐项说明

| 子命令 | 行为 | 成功后进程状态 |
| --- | --- | --- |
| `run` | 加载配置、初始化同步引擎；若 `server.enabled=true` 则启动接收服务；按 `sync_on_start` 执行一次同步；启动 cron；等待 Ctrl+C | 常驻 |
| `serve` | 仅启动 axum HTTP 接收服务，可选连接 SQLite 做入站持久化 | 常驻 |
| `sync-now` | 初始化所选数据源并执行一次 `sync_once()` | 完成后退出 |
| `install-service` | Windows 上注册 `WeighingDataSync` 原生服务 | 完成后退出 |
| `uninstall-service` | Windows 上删除原生服务 | 完成后退出 |
| `enable-autostart` | 当前用户 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 写入启动项 | 完成后退出 |
| `disable-autostart` | 删除上述启动项（不存在时视为成功） | 完成后退出 |
| `autostart-status` | 查询当前用户启动项是否存在 | 完成后退出 |

`run` 的同步任务由 `Arc<Mutex<AppEngine>>` 保护。cron 回调在持有锁期间完成一次
同步，因此同一个进程不会并发执行两轮同步，但一次同步的网络重试会占用该轮的锁。

### 3.3 关闭行为

- 主进程等待 `Ctrl+C`，收到信号后记录 `shutdown` 日志并返回。
- 接收服务使用 axum graceful shutdown 等待 `Ctrl+C`。
- `run` 中启动的接收服务是 Tokio 后台任务；主进程退出时由运行时回收。

## 4. 配置和凭据注入

### 4.1 配置分层

配置由 `config` crate 构建，层级为：

1. 可选 TOML 文件；
2. `WDS__SECTION__KEY` 环境变量覆盖 TOML；
3. `dotenvy` 在启动时读取工作目录的 `.env`。

例如：

```bash
WDS__SYNC__SOURCE=local
WDS__SYNC__BATCH_SIZE=200
WDS__API__API_KEY='cloud-token'
```

### 4.2 配置项功能

| 配置段 | 关键项 | 当前作用 |
| --- | --- | --- |
| `[api]` | `endpoint`、`api_key`、`timeout_seconds` | 云端 HTTP 地址、可选 Bearer Token、单请求超时 |
| `[database]` | `url`、`max_connections` | SQLite 缓存连接串和连接池上限 |
| `[server]` | `enabled`、`bind`、`route`、`max_body_bytes`、`api_key`、`persist` | 接收服务开关、监听、路由、请求体限制、鉴权和入站落库 |
| `[sqlserver]` | `host`、`port`、`database`、`username`、`password`、`table` | TDS 数据源连接与表名 |
| `[sqlserver]` | `trust_cert`、`encryption` | TLS 证书信任和 TDS 加密级别 |
| `[sqlserver]` | `mark_uploaded` | 成功后是否回写 `isUploadCloud=1` |
| `[sync]` | `source`、`batch_size`、`cron`、`sync_on_start` | 数据源、批大小、调度和启动同步 |
| `[sync]` | `retry_initial_delay_ms`、`retry_max_delay_ms`、`retry_max_elapsed_seconds` | 每轮同步的重试预算 |

默认值包括：SQL Server 源、批大小 100、每 5 分钟（`0 */5 * * * *`）、启动即同步、
1 秒初始退避、60 秒退避上限、300 秒单轮预算、16 MiB 接收请求体上限。

`source` 的实现判断是“仅当值不区分大小写等于 `sqlserver` 时选择 SQL Server，否则进入
本地 SQLite 引擎”。因此生产配置应明确写 `sqlserver` 或 `local`，不要依赖未知字符串的回退行为。

Cargo 中的 `sqlite` 和 `http` 是默认启用的标记特性：当前对应依赖并未声明为 optional，关闭
这两个特性不会像 `websocket`、`compression`、`cron`、`windows` 那样移除对应实现；真正改变
编译路径的主要是后四个特性。

### 4.3 凭据安全行为

- SQL Server 默认 host 是 `your-sqlserver-host`，默认密码为空或
  `CHANGE_ME_VIA_ENV` 占位符；`SqlServerConfig::validate()` 会拒绝这些值。
- 云端 API Key 和接收服务 API Key 均为可选，但设置后必须匹配 Bearer Token。
- 仓库不应提交真实密码或 Token；Windows 安装脚本把密码写入受 ACL 保护的 `.env`。

## 5. SQL Server 数据源功能

### 5.1 连接与安全

`SqlServerSource::new` 在建立源对象前执行：

- 凭据校验；
- 表名标识符校验，只允许 ASCII 字母、数字、下划线和点号；
- 查询中的列名统一用 `[]` 引用，右方括号会转义；
- `UPDATE` 的流水号值使用 TDS 参数，不拼接用户输入。

连接使用 `tiberius`/TDS，设置应用名 `weighing-data-sync` 和 `TCP_NODELAY`。
`encryption` 映射为 `Off`、`NotSupported` 或 `Required`；`trust_cert` 控制是否信任服务端证书。

### 5.2 待读取条件和排序

程序固定选择 `tbl_weightInfo` 的 66 个字段，并执行等价于：

```sql
SELECT TOP (@batch_size) ...
FROM [tbl_weightInfo]
WHERE ISNULL([isUploadCloud], 0) = 0
  AND ISNULL([del_flag], 0) = 0
ORDER BY ISNULL([updateTime],
         ISNULL([secondTime], ISNULL([firstTime], [grossTime]))) ASC
```

这意味着：

- 空 `isUploadCloud` 被视为未上传；
- 空 `del_flag` 被视为未删除；
- 越早更新（没有更新时间时回退到称重时间）的记录越先处理；
- `batch_size` 是单轮最多读取数，不是全表一次性加载数。

每行必须有非空 `serialNo`，否则该轮读取失败。

### 5.3 SQL 类型转 JSON

源表使用动态 `Map<String, serde_json::Value>`，因此上游增加字段不要求同步器新增 Rust
字段。转换规则如下：

| SQL Server 类型 | JSON 表示 |
| --- | --- |
| 整数、布尔、有限浮点 | JSON number / boolean |
| `Decimal`、`Numeric`、Money | 保留精度的十进制字符串 |
| 字符串、GUID、XML | JSON string |
| 日期/时间 | `YYYY-MM-DD HH:MM:SS` 字符串 |
| 二进制 | `"<N bytes>"` 占位，不上传原始图片/字节 |
| NULL | JSON null |

非有限浮点（NaN、Infinity）会被拒绝，避免生成非法 JSON。

### 5.4 成功回写

启用 `mark_uploaded` 且云端接受某些流水号时，程序执行参数化批量更新：

```sql
UPDATE [tbl_weightInfo]
SET [isUploadCloud] = 1
WHERE [serialNo] IN (@P1, @P2, ...)
```

每 1000 个流水号一批，低于 SQL Server/TDS 2100 参数上限。关闭 `mark_uploaded` 时仍会
上报，但不改变源表标志，下一轮会再次读取相同记录。

## 6. 本地 SQLite 缓存功能

### 6.1 数据库初始化

`db::connect` 会：

- 自动创建 SQLite 文件父目录和文件；
- 设置 SeaORM 连接池上限；
- 创建 `weighing_records` 表和状态/时间索引；
- 创建 `inbound_payloads` 表。

支持 `sqlite::memory:`，便于测试。

### 6.2 `weighing_records` 状态机

```text
insert_record
     │
     ▼
 pending ──云端接受──▶ synced
     │
     └──批量失败/拒绝──▶ failed ──下一轮再次读取──┘
```

核心字段包括：UUID、票号、秤号、车牌、公斤值、原始单位、测量时间、状态、重试次数、
最后错误和创建/更新时间。

- `fetch_pending` 读取 `pending` 和 `failed`，按 `measured_at` 升序，最多 `batch_size` 条；
- `mark_synced` 使用单条批量 `UPDATE`，清空 `last_error`；
- `mark_failed` 使用单条批量 `UPDATE`，原子增加 `retry_count` 并记录错误；
- 空 ID 列表是无操作。

### 6.3 重量单位和校验

`WeighingRecord` 内部统一保存公斤值，同时保留 `kilogram` 或 `pound` 原始单位。
通过 `uom` 提供：

- `new_kg`：从公斤创建；
- `new_lb`：从磅创建并转换为公斤；
- `mass_kg`、`weight_lb`：类型安全的换算。

重量必须是有限且非负数；零合法，负数、NaN、Infinity 非法。单位解析兼容 `kg`、
`kilogram`、`lb`、`lbs`。

### 6.4 当前边界

SQLite 缓存 API 已实现，但当前二进制没有“从命令行插入一条
`weighing_records`”的子命令；接收服务也不会把入站 `records` 自动转换为该表，而是写入
`inbound_payloads` 原始 JSON。因此 `sync.source=local` 适合由其他代码/工具预先写入缓存，
不是一个完整的采集端。

## 7. 两条同步引擎

### 7.1 共同行为

两个引擎均：

1. 按批次读取源记录；
2. 构造 JSON POST 请求；
3. 可选添加 `Authorization: Bearer <api_key>`；
4. 使用配置的 HTTP 超时；
5. 对 transient 错误执行指数退避；
6. 返回统一的 `SyncOutcome`。

`SyncOutcome` 字段：

| 字段 | 含义 |
| --- | --- |
| `fetched` | 本轮从源读取的记录数 |
| `synced` | 云端确认的记录数 |
| `failed` | 云端明确拒绝的记录数 |
| `marked_uploaded` | SQL Server 是否执行源表回写；本地链路为 `None` |
| `no_data` | 本轮没有待处理记录 |

### 7.2 本地链路（`sync.source=local`）

请求体包含 `source=weighing-data-sync`、`uploaded_at` 和 `records`。每条记录同时发送
`weight_kg`、`weight_lb`、`original_unit` 和 RFC3339 时间。

响应处理：

- `204`：整批视为成功；
- 成功响应体中 `accepted_ids`、`failed_ids` 均为空：按整批视为成功；
- 有明确 `accepted_ids`：只将这些 ID 标记 `synced`；
- 有 `failed_ids`：将这些 ID 标记 `failed`；
- 整批请求最终失败：本批所有已读取 ID 标记 `failed`。

本地引擎对 `200 OK` 空响应体会尝试 JSON 解码并将解码失败视为永久错误；如果接收端希望
用无响应体表示成功，应返回 `204 No Content`。这与 SQL Server 引擎不同，后者明确把任意
成功状态的空响应体当作整批成功。

### 7.3 SQL Server 链路（`sync.source=sqlserver`）

请求体包含 `source=sqlserver-yunfu-tbl_weightInfo`、数据库名、表名、上传时间和完整
动态行 JSON。

响应处理：

- `204`、空成功响应体或两个确认数组都为空：整批流水号视为接受；
- 有 `accepted_serial_nos`：只回写这些流水号；
- 有 `failed_serial_nos`：计入结果，但失败流水号保持 `isUploadCloud=0`，下轮可重试；
- 请求最终失败：不回写源表，源记录保持未上传状态。

云端成功与源库回写不是同一个事务：如果云端已经落库但进程在回写前退出，下一轮可能
再次发送同一流水号。因此云端接收端应按 `serialNo`（SQL Server）或 `id`（本地缓存）实现
幂等 upsert；同步器本身不提供跨系统事务或分布式锁。

## 8. HTTP 接收服务

### 8.1 路由

- `GET /`：返回服务名、`status=ok` 和当前 UTC 时间；
- `POST <server.route>`：接收批量 JSON，默认路由为
  `/weighing-data-sync/put`；
- 使用 axum `DefaultBodyLimit` 限制请求体，默认 16 MiB。

接收服务可以通过 `serve` 单独运行，也可以由 `run` 在 `server.enabled=true` 时后台启动。

### 8.2 鉴权和校验

- `server.api_key` 为空：开发/日志模式，允许请求；
- 设置 `server.api_key`：必须提供匹配的 `Authorization: Bearer <token>`（兼容 `Bearer` 和
  `bearer` 前缀）；
- 缺少或错误 Token 返回 `401`；
- `records` 缺失或为空数组返回 `400`；
- 其他结构良好的记录默认全部接受，不执行逐条业务校验。

响应包含请求 UUID、接受状态、从 `serialNo` 或 `serial_no` 提取的流水号、记录数和接收时间。
接收端不会计算逐条拒绝集合，因此 `failed_serial_nos` 固定为空。

### 8.3 入站持久化

`server.persist=true` 且 SQLite 连接成功时，每个请求以一行原始 JSON 写入
`inbound_payloads`，包含请求 ID、来源、数据库、表名、记录数、完整 `payload_json` 和接收时间。

该落库是 best-effort：SQLite 连接失败或写入失败只记录告警，仍返回接收成功。这样可以避免
接收端数据库故障阻断网络入口，但也意味着调用方不能仅凭 HTTP 200 断言审计数据已落盘。

## 9. 重试和错误分类

`UploadError` 将失败分为：

| 类型 | 包含 | 行为 |
| --- | --- | --- |
| `Transient` | 网络/发送错误、请求超时、`5xx`、`408`、`429`、读取响应体 I/O 错误 | 按指数退避，直到本轮总耗时预算 |
| `Permanent` | 除 `408`/`429` 外的所有 `4xx`、请求体构造失败、成功响应 JSON 解码失败 | 立即返回，不重试 |

退避从 `max(initial_delay, 1ms)` 开始，每次翻倍并受 `max_delay` 限制；最后一次等待不会
超过 `max_elapsed` 剩余时间。重试预算是“每次 `sync_once` 的预算”，不是某条记录的永久最大次数。

重要结果：

- 本地链路对最终失败批次增加 `retry_count` 并保持 `failed`；
- SQL Server 链路不修改源标志，下一轮按过滤条件再次读取；
- 4xx（例如错误 Token、错误路径、非法 payload）不会因为重复等待而自愈，应先修配置或协议。

## 10. 可选 WebSocket 客户端

启用 `websocket` feature 后编译 `WebSocketClient`。它是一个独立的、无连接状态客户端，
每次 `push_batch` 都建立一次连接、发送一批 JSON、读取 ACK 后关闭。

提供的能力：

- 可选 Bearer Token；
- 连接超时默认 10 秒，ACK 超时默认 30 秒；
- 最大消息默认 16 MiB，写缓冲默认 128 KiB；
- 支持 Text/Binary ACK，处理 Ping/Pong；
- 接受 `true`、`{"accepted":true}`、`ok`、`success`、`status=accepted` 等常见 ACK；
- 拒绝 `failed`/`failed_ids` 非零或 ACK 中 `received` 与发送数不一致的批次。

当前主同步引擎仍使用 HTTP，WebSocket 模块没有被 `AppEngine` 或调度器自动选择，也不包含
独立重试策略。

## 11. gzip 请求体特性

启用 `compression` feature 后，两个 HTTP 同步引擎会：

- 先序列化 JSON；
- 使用 gzip 压缩；
- 设置 `Content-Encoding: gzip` 和 `Content-Type: application/json`；
- 仍沿用同样的状态码、响应解析和重试策略。

云端必须明确支持 gzip，否则应保持默认未启用状态。

## 12. Windows 运维功能

### 12.1 推荐：计划任务

`scripts/install-windows.ps1` 以管理员权限执行，完成：

- 安装 `sync-daemon.exe` 到 `Program Files`；
- 在 `ProgramData` 创建配置、SQLite 数据和日志目录；
- 生成生产 TOML 和受 ACL 保护的 `.env`；
- 可选检查 SQL Server TCP 端口；
- 注册名为 `WeighingDataSync` 的开机计划任务，以 `SYSTEM`、最高权限运行；
- 设为 `MultipleInstances=IgnoreNew`，失败后最多重启 3 次；
- 可选立即运行一次 `sync-now`。

卸载脚本默认删除程序和任务、保留 `ProgramData` 数据；只有传入 `-RemoveData` 才删除数据目录。

### 12.2 注册表自启动

`enable-autostart` 在当前用户 Run 项写入当前 exe 的 `run` 命令，适合交互式用户登录场景。
它不是系统级服务，且命令未显式传入配置路径，实际运行依赖工作目录和 `.env`，不建议作为无人值守
现场的默认方式。

### 12.3 原生 Windows Service 入口

`install-service` 创建自动启动的 `WeighingDataSync` 服务并设置描述，`uninstall-service` 删除服务。
仓库当前保留该入口，但生产文档推荐计划任务；服务入口不是完整的 SCM 生命周期实现替代品。

## 13. 数据实体和迁移基础

### 13.1 本地实体

`src/entity/weighing_record.rs` 对应 SQLite `weighing_records`，领域层由
`db::models::WeighingRecord` 表示。

### 13.2 SQL Server 实体映射

`src/entity/weight_info.rs` 描述 `tbl_weightInfo` 的完整字段（包括重量、时间、备用字段、
上传标志、业务关联字段等）。实际生产读取使用 `tiberius` 动态列映射，而不是 SeaORM 直接查询。

`src/entity/weight_photo.rs` 描述 `tbl_weightPhoto`，包含 `captureImage` 二进制字段，但当前
没有对应的 SQL Server source 或同步引擎调用，因此它是后续迁移/扩展基础，不是当前默认同步范围。

仓库中的 `docs/sync_structs.rs`、`docs/mysql_sync_schema.sql` 和
`docs/sync_field_mapping.md` 是云端/下游 MySQL 映射参考，不能单独视为已经接入的第二条图片同步链路。

## 14. Docker 与端到端验收

`scripts/validate_sqlserver_e2e.sh` 编排 `docker/docker-compose.e2e.yml`，覆盖完整的默认生产路径：

1. `sqlserver` 启动 SQL Server 2022 Linux 容器，建立 `yunfu.dbo.tbl_weightInfo` 66 列表并写入 100 条样本；
2. `receiver` 启动本项目 HTTP 接收端，启用 Bearer 鉴权和 `inbound_payloads` 持久化；
3. `sync-runner` 运行一次 `sync-now`，从 SQL Server 读取 100 条并上传；
4. `receiver-verify` 检查 SQLite 中存在一批 100 条完整流水号；
5. `sqlserver-verify` 检查 100 条源记录已回写；
6. `e2e` 在两个验收器成功后输出通过消息。

SQL Server 容器声明 `linux/amd64`。非 x86-64 主机默认退出并返回平台限制；只有显式设置
`WDS_E2E_ALLOW_UNSUPPORTED_SQLSERVER_EMULATION=1` 才尝试仿真运行，仿真失败不能等同于同步代码失败。

## 15. 日志和可观测性

日志由 `tracing_subscriber` 输出 JSON，默认级别为 `info`，可用 `RUST_LOG` 调整。关键
`stage` 包括：

- 启动/配置：`startup.begin`、`config.loaded`、`startup.ready`、`shutdown`；
- 组件就绪：`database.ready`、`sqlserver.ready`、`client.ready`、`scheduler.ready`；
- 同步：`sync.fetch`、`sync.upload.begin/done`、`sync.retry`、
  `sqlserver.fetch/upload/ retry/mark_uploaded`；
- 接收：`server.startup`、`server.ready`、`server.put`、`server.shutdown`；
- Windows：`windows.autostart.*`、`windows.service.*`。

每轮同步结果会结构化输出 `SyncOutcome`。需要特别注意：接收服务的 `server.put` 当前会
记录完整 payload，生产环境应评估日志权限、日志保留周期和车牌/业务数据脱敏要求。

## 16. 安全能力与限制

已实现的防护：

- 默认凭据占位符和启动校验；
- HTTP 客户端 rustls，SQL Server 可配置 TDS 加密；
- 接收端可选 Bearer 鉴权；
- 请求体大小上限；
- SQL 标识符白名单、列名引用和参数化回写；
- JSON 数字拒绝非有限浮点。

当前限制：

- 接收服务未对每条记录做业务 schema 校验，只校验 `records` 非空；
- 入站持久化是 best-effort，不是强一致写入；
- SQL Server 二进制/图片列只发送大小占位符；
- WebSocket 客户端不在默认同步链路中，也没有内建重试；
- `failed` 记录没有永久尝试次数上限，会随定时任务继续参与同步；
- 同一进程不并发执行两轮同步，但跨进程/多实例部署没有分布式锁。
- 云端确认和源端状态回写之间没有两阶段提交，异常退出可能导致重复上报，需由云端幂等处理。

## 17. 验证现状

当前源码包含配置、单位换算、SQLite CRUD 往返、SQL 标识符转义、Numeric 格式化、HTTP
错误分类、指数重试和 WebSocket ACK 的单元测试。执行：

```bash
cargo test --all-features
```

在本次文档整理时，结果为 **25 个测试通过**，`src/main.rs` 没有独立单元测试，Docker SQL
Server E2E 仍需在支持的 x86-64 Linux 主机上执行才能验证真实 TDS/容器路径。

建议交付前继续执行：

```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
scripts/validate_sqlserver_e2e.sh
```

## 18. 源码功能索引

| 路径 | 责任 |
| --- | --- |
| `src/main.rs` | CLI、日志、引擎选择、启动同步、cron 和关闭 |
| `src/config.rs` | 配置结构、默认值、环境变量覆盖、凭据/加密校验 |
| `src/source/sqlserver.rs` | TDS 连接、66 列查询、动态 JSON 转换、批量回写 |
| `src/sync/engine.rs` | SQLite 缓存到 HTTP 云端 |
| `src/sync/sqlserver_engine.rs` | SQL Server 到 HTTP 云端 |
| `src/sync/error.rs` / `retry.rs` | 上传错误分类与指数退避 |
| `src/db/mod.rs` | SQLite 连接、迁移、批量状态更新、入站 payload 落库 |
| `src/db/models.rs` | 重量单位、状态机、领域模型和校验 |
| `src/server.rs` | 健康检查、HTTP 接收、鉴权、体积限制、审计落库 |
| `src/client/websocket.rs` | 可选 WebSocket 批量客户端与 ACK 解析 |
| `src/entity/*.rs` | SQLite/SQL Server 表结构映射 |
| `src/windows/*.rs` | Windows 服务和 Run 项入口 |
| `scripts/*.ps1` | Windows 构建、安装、卸载 |
| `docker/`、`scripts/validate_sqlserver_e2e.sh` | SQL Server Docker E2E |
| `docs/` | 配置、协议、架构、字段映射、部署和排障资料 |

## 19. 相关文档

- [架构](architecture.md)：两条同步链路和模块职责；
- [配置说明](configuration.md)：完整配置项和凭据注入；
- [HTTP 协议](api-protocol.md)：请求/响应 schema 和状态码语义；
- [快速开始](getting-started.md)：构建、运行和本机验证；
- [Windows 部署](deployment-windows.md)：现场安装和升级；
- [日志与排障](logging.md)、[常见问题](troubleshooting.md)：运行诊断；
- [SQL Server Docker E2E](sqlserver-docker-e2e.md)：容器验收路径。
