# 日志与排障

日志输出为 **JSON**（`tracing_subscriber` JSON 格式），每条日志包含 `stage` 字段，便于
Windows 事件采集、日志平台检索和现场定位阶段。日志级别由 `RUST_LOG` 控制，默认 `info`。

## `stage` 目录

| 阶段 | `stage` | 含义 |
| --- | --- | --- |
| 启动开始 | `startup.begin` | 称重数据同步系统正在启动 |
| 配置加载 | `config.loaded` | 配置文件加载成功 |
| SQL Server 源 | `sqlserver.ready` | SQL Server 数据源已初始化 |
| 数据库就绪 | `database.ready` | 本地 SQLite 缓存数据库已就绪 |
| HTTP 客户端 | `client.ready` | 云端同步客户端已初始化 |
| 启动同步 | `sync.startup` | 启动同步完成或失败（失败后等待调度器重试） |
| 调度器 | `scheduler.ready` | 同步调度器已启动 |
| 调度器未启用 | `scheduler.disabled` | cron 特性未启用，定时同步不会启动 |
| 服务就绪 | `startup.ready` | 服务已就绪，等待同步信号 |
| 关闭 | `shutdown` | 收到停止信号，服务正在关闭 |
| 定时同步 | `sync.scheduled` | 定时同步完成或失败 |
| 手动同步 | `sync.manual` | 手动同步开始和完成 |
| 本地拉取 | `sync.fetch` | 没有待同步的本地称重记录 |
| 本地上报 | `sync.upload.*` | 本地数据开始上报 / 重试 / 完成 |
| 重试 | `sync.retry` | 上传失败，准备按指数退避重试 |
| SQL Server 拉取 | `sqlserver.fetch` | 没有待上报的 SQL Server 称重记录 |
| SQL Server 上报 | `sqlserver.upload.*` | SQL Server 数据开始上报 / 重试 / 完成 |
| SQL Server 回写 | `sqlserver.mark_uploaded` | SQL Server 云上传状态已更新 |
| 接收服务启动 | `server.startup` / `server.ready` | 接收服务准备启动 / 已启动 |
| 接收服务上报 | `server.put` | 收到称重数据上报，含摘要与完整 payload |
| 接收服务关闭 | `server.shutdown` | 接收服务正在关闭 |
| 自启动 | `windows.autostart` | 用户登录自启动项写入 / 移除 / 查询 |
| 服务安装 | `windows.service.*` | Windows 服务安装或卸载 |

## 示例

```json
{"timestamp":"2026-07-28T08:00:00Z","level":"INFO","stage":"startup.begin","version":"0.1.0","message":"称重数据同步系统正在启动"}
{"timestamp":"2026-07-28T08:00:00Z","level":"INFO","stage":"config.loaded","path":"config/default.toml","source":"sqlserver","message":"配置文件加载成功"}
{"timestamp":"2026-07-28T08:00:00Z","level":"INFO","stage":"database.ready","max_connections":5,"message":"本地 SQLite 缓存数据库已就绪"}
{"timestamp":"2026-07-28T08:00:00Z","level":"INFO","stage":"scheduler.ready","cron":"0 */5 * * * *","message":"同步调度器已启动"}
{"timestamp":"2026-07-28T08:00:00Z","level":"INFO","stage":"sqlserver.upload.done","outcome":{"fetched":12,"synced":12,"failed":0,"marked_uploaded":true,"no_data":false},"message":"SQL Server 数据上报完成"}
{"timestamp":"2026-07-28T08:00:00Z","level":"WARN","stage":"sync.retry","error":"...","retry_after_ms":2000,"message":"上传失败，准备按指数退避重试"}
```

## 排障流程

1. **启动即失败**：看第一条 `startup.begin` 之后的错误。若是
   `SQL Server host/password is not configured`，说明凭据仍是占位符，按
   [配置说明 · 凭据注入](configuration.md#凭据注入) 注入 `WDS__SQLSERVER__*`。
2. **同步一直失败**：按 `stage` 区分：
   - `sqlserver.fetch` 之前失败 → SQL Server 连接/查询问题，见
     [常见问题](troubleshooting.md#sql-server-连接失败)。
   - `sqlserver.retry` 反复出现 → 网络/云端问题；看错误中的 HTTP 状态码：
     - `4xx`（非 408/429）→ 永久失败，**不会**重试，需修正请求（鉴权、payload、地址）。
     - `5xx` / 408 / 429 → transient，会重试直到 `retry_max_elapsed_seconds` 耗尽。
3. **接收服务返回 401**：`server.api_key` 已设置但请求未带或带错 `Authorization: Bearer`。
4. **接收服务返回 400**：请求体 `records` 缺失或为空。
5. **落库未生效**：确认 `server.persist = true` 且 SQLite 可写；落库失败会在
   `server.put` 中输出 `接收数据落库失败` 警告。

## 调整日志级别

```bash
RUST_LOG=debug cargo run -- run            # 更详细
RUST_LOG=warn cargo run -- run             # 只看告警及以上
RUST_LOG="weighing_data_sync=debug" cargo run -- run
```
