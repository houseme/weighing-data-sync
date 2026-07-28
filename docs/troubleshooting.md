# 常见问题

## 启动报“SQL Server host/password is not configured”

现场凭据仍是占位符或为空。`SqlServerConfig::validate()` 在启用 `sqlserver` 链路时强制要求
真实的 host / username / password。注入方式（PowerShell）：

```powershell
$env:WDS__SQLSERVER__HOST     = "现场主机"
$env:WDS__SQLSERVER__USERNAME = "sa"
$env:WDS__SQLSERVER__PASSWORD = "现场密码"
```

或写入部署机的 `config/default.toml`（生产建议用凭据管理器，见
[Windows 部署 · 凭据管理](deployment-windows.md#凭据管理生产环境)）。

只想在开发机验证流程、不连现场库时，切到本地链路：

```bash
WDS__SYNC__SOURCE=local cargo run -- sync-now
```

## SQL Server 连接失败

`stage` 停在 `sqlserver.fetch` 之前，错误含 `failed to connect SQL Server` 或
`failed to login SQL Server`：

- 网络：开发机不在现场内网时，`your-sqlserver-host:1433` 不可达属预期。
- `encryption`：现场要求加密时设 `WDS__SQLSERVER__ENCRYPTION=required`；自签证书需配合
  `trust_cert = true`。
- 端口/防火墙：确认 1433 可达、SQL Server 已启用 TCP/IP。
- 账号：`sa` 被禁用或密码错误会返回登录失败。

## 云端返回 4xx（永久失败，不重试）

`400` / `401` / `403` / `404` / `413` 等（除 408/429）会被归类为 **permanent**，立即放弃，
**不会**消耗重试预算。常见原因：

- `401` / `403`：`api_key` 未配置或无效 → 设置 `WDS__API__API_KEY`。
- `400`：请求体不符合云端预期 → 对照 [HTTP 协议](api-protocol.md) 检查 payload。
- `404`：`endpoint` 路径错误。
- `413`：请求体过大 → 调小 `batch_size`，或开启 `compression` 特性压缩请求体。

这类错误在日志中表现为单次失败后立即返回，且 **不出现** `sqlserver.retry` / `sync.retry`。

## 云端返回 5xx / 超时（transient，会重试）

`5xx`、`408`、`429`、网络错误会按指数退避重试，直到 `retry_max_elapsed_seconds`（默认 300s）
耗尽。日志会反复出现 `*.retry`。若长期不恢复，检查云端可用性与网络。

## 接收服务返回 401 / 400

- `401`：`server.api_key` 已设置，但请求未带或带错 `Authorization: Bearer <token>`。
- `400`：请求体 `records` 缺失或为空数组。

## 接收服务落库未生效

确认 `server.persist = true` 且 SQLite 目录可写。落库失败会在 `server.put` 日志中输出
`接收数据落库失败`，且不影响返回 200（仅记日志）。查询落库内容：

```bash
sqlite3 data/weighing_sync.db 'SELECT request_id, source, records_count, received_at FROM inbound_payloads ORDER BY received_at DESC LIMIT 10;'
```

## 队头阻塞（大量积压）

`fetch_pending` 按 `batch_size`（默认 100）取最旧的一批上报。当积压量远大于 `batch_size`
且上报持续失败时，同一批最旧记录会被反复重试，后续记录暂时得不到处理（head-of-line blocking）。
缓解手段：

- 临时调大 `batch_size`。
- 优先修复上报失败（见上文 4xx/5xx），让队列正常消化。
- （未来可引入高水位游标或分页 `OFFSET`，目前未实现，以保持读取语义简单。）

## Windows 服务 / 自启动不生效

- `install-service` 需管理员权限。
- 自启动 Run 项写入的是 `HKCU`，仅对当前用户登录生效；服务方式更适合作业现场。
- 服务以 `run` 参数启动；如需指定配置，参考服务安装实现或部署机配置路径。

## 重新验证构建

```bash
cargo fmt --all && cargo check && cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo check --all-features
```
