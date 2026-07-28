# 配置说明

配置通过 `config` crate 分层加载：**TOML 文件（可选）** 为底，**`WDS__*` 环境变量** 覆盖其上。
环境变量用双下划线分隔层级，例如 `WDS__API__ENDPOINT` 覆盖 `[api] endpoint`。

## 凭据注入

> ⚠️ 现场凭据（SQL Server 主机 / 账号 / 密码、云端 API Key、接收服务 Token）
> **不随仓库分发**。`config/default.toml` 与 `src/config.rs` 默认值均为占位符。

`SqlServerConfig::validate()`（在 `SqlServerSource::new` 中调用）会拒绝空值或占位符，
启动 SQL Server 链路前必须配置真实值。推荐用环境变量注入（PowerShell 示例）：

```powershell
$env:WDS__SQLSERVER__HOST     = "现场 SQL Server 主机"
$env:WDS__SQLSERVER__DATABASE = "yunfu"
$env:WDS__SQLSERVER__USERNAME = "sa"
$env:WDS__SQLSERVER__PASSWORD = "现场密码"
$env:WDS__API__API_KEY        = "云端 Bearer Token"
$env:WDS__SERVER__API_KEY     = "接收服务 Bearer Token（开启鉴权时）"
```

生产环境建议用 **Windows 凭据管理器** 或服务账号的环境变量注入，不写入磁盘配置，见
[Windows 部署](deployment-windows.md)。

## 完整配置示例

`config/default.toml`：

```toml
[api]
endpoint = "https://api.xmb.xyz/weighing-data-sync/put"
timeout_seconds = 30
# api_key = "云端 Bearer Token（推荐用环境变量注入）"

[database]
url = "sqlite://data/weighing_sync.db"
max_connections = 5

[server]
enabled = false
bind = "0.0.0.0:8080"
route = "/weighing-data-sync/put"
max_body_bytes = 16777216
# 开启后接收端点要求 `Authorization: Bearer <api_key>`
# api_key = "接收服务 Token"
# 开启后每批上报同时写入本地 inbound_payloads 表
persist = false

[sqlserver]
host = "your-sqlserver-host"      # 占位符，必须覆盖
port = 1433
database = "yunfu"
username = "sa"
password = "CHANGE_ME_VIA_ENV"    # 占位符，必须覆盖
table = "tbl_weightInfo"
trust_cert = true
encryption = "off"
mark_uploaded = true

[sync]
source = "sqlserver"              # 或 "local"
batch_size = 100
cron = "0 */5 * * * *"
sync_on_start = true
retry_initial_delay_ms = 1000
retry_max_delay_ms = 60000
retry_max_elapsed_seconds = 300
```

## 配置项详解

### `[api]` 云端上报

| 键 | 默认 | 说明 |
| --- | --- | --- |
| `endpoint` | `https://api.xmb.xyz/weighing-data-sync/put` | 云端接收地址 |
| `timeout_seconds` | `30` | 单次 HTTP 请求超时 |
| `api_key` | _(无)_ | 可选 Bearer Token，设置后上报自动带 `Authorization: Bearer <api_key>` |

### `[database]` 本地 SQLite 缓存

| 键 | 默认 | 说明 |
| --- | --- | --- |
| `url` | `sqlite://data/weighing_sync.db` | SQLite 连接串，支持 `sqlite::memory:` |
| `max_connections` | `5` | 连接池上限 |

### `[server]` HTTP 接收服务

| 键 | 默认 | 说明 |
| --- | --- | --- |
| `enabled` | `false` | 守护进程启动时是否同时拉起接收服务 |
| `bind` | `0.0.0.0:8080` | 监听地址 |
| `route` | `/weighing-data-sync/put` | 接收路由 |
| `max_body_bytes` | `16777216`（16 MiB） | 请求体上限 |
| `api_key` | _(无)_ | 设置后接收端点强制 Bearer 鉴权 |
| `persist` | `false` | 设置后每批上报以原始 JSON 写入 `inbound_payloads` |

### `[sqlserver]` SQL Server 源

| 键 | 默认 | 说明 |
| --- | --- | --- |
| `host` | `your-sqlserver-host`（占位符） | 主机，必须覆盖 |
| `port` | `1433` | 端口 |
| `database` | `yunfu` | 库名 |
| `username` | `sa` | 账号，必须覆盖为真实值 |
| `password` | _(空，占位符)_ | 密码，必须覆盖 |
| `table` | `tbl_weightInfo` | 源表（仅允许字母/数字/`_`/`.`） |
| `trust_cert` | `true` | 是否信任服务端证书 |
| `encryption` | `off` | TDS 加密级别：见下表 |
| `mark_uploaded` | `true` | 上报成功后是否回写 `isUploadCloud = 1` |

`encryption` 取值映射（`SqlServerConfig::is_plaintext` / `is_required_encryption`）：

| 取值 | 行为 |
| --- | --- |
| `off` | `EncryptionLevel::Off` |
| `none` / `plaintext` / `not_supported` | `EncryptionLevel::NotSupported`（明文） |
| `required` / `true` / `on` | `EncryptionLevel::Required`（强制加密） |

### `[sync]` 同步调度

| 键 | 默认 | 说明 |
| --- | --- | --- |
| `source` | `sqlserver` | 数据源：`sqlserver` 或 `local`（本地缓存链路） |
| `batch_size` | `100` | 每批拉取/上报记录数 |
| `cron` | `0 */5 * * * *` | 6 字段 cron（秒 分 时 日 月 周） |
| `sync_on_start` | `true` | 启动时是否立即同步一次 |
| `retry_initial_delay_ms` | `1000` | 首次重试延迟 |
| `retry_max_delay_ms` | `60000` | 重试延迟上限 |
| `retry_max_elapsed_seconds` | `300` | 重试总预算，超过即放弃 |

## 重试策略要点

- **transient（可重试）**：网络错误、超时、`5xx`、`408`、`429` → 按指数退避重试，直到预算耗尽。
- **permanent（立即放弃）**：`400` / `401` / `403` / `404` / `413` 等其它 `4xx` → 立即返回，
  不消耗重试预算（详见 [架构 · 重试分类](architecture.md#重试错误分类)）。
