# 快速开始

## 环境要求

- Rust **1.95+**（Edition 2024）。
- 默认特性 `sqlite` / `http` / `cron` 保证非 Windows 开发机可直接编译。
- 连接 SQL Server 需要现场网络可达（默认链路 `sync.source = "sqlserver"`）。

## 编译

```bash
cargo fmt --all
cargo check
cargo check --all-features          # 覆盖 compression / websocket / windows 等特性
cargo build --release               # 发布构建
```

可选特性：

```bash
cargo build --release --features compression     # gzip 压缩上报请求体
cargo build --release --features websocket       # WebSocket 客户端占位（见架构文档）
cargo build --release --features windows         # Windows 专用入口（现场部署见 deployment-windows.md）
```

## 子命令

二进制名为 `sync-daemon`（`src/main.rs`）。全局参数 `--config`（或环境变量 `WDS_CONFIG`），
默认 `config/default.toml`。

| 子命令 | 作用 |
| --- | --- |
| `run`（默认） | 启动守护进程：可选启动接收服务 → 启动同步一次 → 启动 cron 调度 → 等待 Ctrl-C |
| `serve` | 只启动 HTTP 接收服务（`[server]`） |
| `sync-now` | 手动执行一次同步并退出 |
| `install-service` / `uninstall-service` | 原生 Windows 服务注册入口（需 `windows` 特性；现场部署默认用计划任务脚本） |
| `enable-autostart` / `disable-autostart` / `autostart-status` | 注册表 Run 项自启动管理 |

## 运行示例

```bash
# 守护进程（默认 SQL Server 链路；需先配置真实凭据，见 configuration.md）
cargo run -- run --config config/default.toml

# 用本地缓存链路手动同步一次（无需现场 SQL Server）
RUST_LOG=info WDS__SYNC__SOURCE=local cargo run -- sync-now --config config/default.toml

# 启动 HTTP 接收服务（本机验证）
RUST_LOG=info WDS__SERVER__BIND=127.0.0.1:18080 cargo run -- serve --config config/default.toml

# Docker 端到端验证：模拟 SQL Server → HTTP 接收端 → SQLite 落库 → SQL Server 回写
scripts/validate_sqlserver_e2e.sh
```

## 本机快速验证（HTTP 接收）

```bash
# 终端 1：启动接收服务
RUST_LOG=info WDS__SERVER__BIND=127.0.0.1:18080 cargo run -- serve --config config/default.toml

# 终端 2：上报一条数据
curl -sS -X POST http://127.0.0.1:18080/weighing-data-sync/put \
  -H 'Content-Type: application/json' \
  -d '{"source":"demo","database":"yunfu","table":"tbl_weightInfo","records":[{"serialNo":"202607280001","grossWeight":"12500.00"}]}'
```

预期返回包含 `accepted_serial_nos` 与 `records_count`，同时终端 1 输出
`stage = "server.put"` 的 JSON 日志。

开启鉴权后需携带 Token：

```bash
RUST_LOG=info WDS__SERVER__BIND=127.0.0.1:18080 WDS__SERVER__API_KEY=test-token \
  cargo run -- serve --config config/default.toml

curl -sS -X POST http://127.0.0.1:18080/weighing-data-sync/put \
  -H 'Authorization: Bearer test-token' -H 'Content-Type: application/json' -d '{...}'
```

## 单元测试

```bash
cargo test
```

覆盖重量单位换算、状态机往返、SQL 标识符转义、`Numeric` 格式化、重试错误分类、
SQLite 缓存的 insert / fetch / mark 往返等纯逻辑或内存 SQLite 路径，不依赖外部服务。

## Docker SQL Server 端到端验证

仓库内置 `docker/docker-compose.e2e.yml`，会启动模拟 SQL Server 数据源、HTTP 接收端、一次性
同步器和验收器。运行：

```bash
scripts/validate_sqlserver_e2e.sh
```

验证详情、样本数据和平台限制见 [SQL Server Docker 端到端验证](sqlserver-docker-e2e.md)。

## 下一步

- 完整配置项与凭据注入：[配置说明](configuration.md)。
- 现场上线部署：[Windows 部署](deployment-windows.md)。
- 日志与排障：[日志与排障](logging.md)。
