# SQL Server Docker 端到端验证

本项目提供一套 Docker 端到端夹具，用固定样本模拟现场 SQL Server `yunfu.dbo.tbl_weightInfo`
数据源，并和 `cmd/receiver` Go 接收端、Rust 一次性同步器、验收器一起启动。验证覆盖：

- SQL Server 源表启动时初始化 100 条样本，全部满足 `isUploadCloud = 0 AND del_flag = 0`。
- `sync-daemon sync-now` 从 SQL Server 一次读取 100 条待同步记录，并通过 HTTP 上报到接收端。
- `cmd/receiver` Go 接收端写入 SQLite `wds_receive_batches` / `wds_receive_records`，并在 E2E 中开启 `STORE_RAW_RECORDS=true`。
- SQL Server 验收器确认 100 条待同步记录已回写为 `isUploadCloud = 1`。
- SQLite 验收器确认 Go receiver 收到并幂等保存了这 100 条待读取数据，且保存了可供 B 端复制使用的原始记录 JSON。

## 运行

```bash
scripts/validate_sqlserver_e2e.sh
```

脚本会使用 `docker/docker-compose.e2e.yml`，构建三个镜像/target：

| 镜像 | 作用 |
| --- | --- |
| 根目录 `Dockerfile` 默认 target | 构建 Rust `sync-daemon`，用于一次性 SQL Server 同步器 |
| 根目录 `Dockerfile` 的 `go-receiver` target | 从 `cmd/receiver` 构建 Go 接收端，并携带 SQLite 验收脚本 |
| `docker/sqlserver/Dockerfile` | 基于 SQL Server 2022 Linux 容器，内置建表、样本数据和 SQL Server 验收脚本 |

可选环境变量：

| 变量 | 默认 | 说明 |
| --- | --- | --- |
| `WDS_E2E_SQLSERVER_PASSWORD` | `WdsE2ePassw0rd_2026` | SQL Server `sa` 密码，必须满足 SQL Server 密码复杂度 |
| `WDS_E2E_SQLSERVER_PORT` | `11433` | 暴露到宿主机的 SQL Server 端口 |
| `WDS_E2E_RECEIVER_PORT` | `18080` | 暴露到宿主机的接收端 HTTP 端口 |
| `WDS_E2E_PROJECT_NAME` | `weighing-data-sync-e2e` | Docker Compose project 名 |
| `WDS_E2E_ALLOW_UNSUPPORTED_SQLSERVER_EMULATION` | _(空)_ | 非 x86-64 主机上设为 `1` 后才尝试运行 |

## 服务编排

| 服务 | 行为 |
| --- | --- |
| `sqlserver` | 启动 SQL Server、执行 `docker/sqlserver/init.sql`，healthcheck 等待 100 条待同步样本就绪 |
| `receiver` | 执行从 `cmd/receiver` 构建的 `go-receiver`，监听 `/health` 和 `/weighing-data-sync/put`，写入 `/data/receiver.db` |
| `sync-runner` | 执行一次 `sync-daemon sync-now`，连接 `sqlserver` 并上报到 `receiver` |
| `receiver-verify` | 读取 Go receiver 的 SQLite，验证批次、流水号范围和 `raw_record` 持久化 |
| `sqlserver-verify` | 查询 SQL Server，验证回写和过滤行为 |
| `e2e` | 等两个验收器成功后输出最终通过消息 |

## 平台边界

微软 SQL Server Linux 容器官方支持范围是运行在 Intel/AMD x86-64 CPU 上的 Linux 主机；Rosetta 2、
QEMU 等仿真/翻译环境不属于受支持路径。Apple Silicon 机器可以通过
`WDS_E2E_ALLOW_UNSUPPORTED_SQLSERVER_EMULATION=1` 尝试运行，但失败时应优先视为平台限制，
不要直接归因到同步代码。

## 手动调试

脚本失败后可临时直接使用 Compose 查看日志：

```bash
docker compose -p weighing-data-sync-e2e -f docker/docker-compose.e2e.yml up --build
docker compose -p weighing-data-sync-e2e -f docker/docker-compose.e2e.yml logs -f sync-runner
docker compose -p weighing-data-sync-e2e -f docker/docker-compose.e2e.yml down -v --remove-orphans
```
