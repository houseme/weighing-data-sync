# A/B/C 三机 Go 服务部署指南

本文档面向三台机器分工部署：

- A 机器：运行 `a-uploader`，从本机或局域网 SQL Server 读取称重记录，签名上报到 C。
- C 机器：运行 `go-receiver`，接收 A 上报，写入本地 SQLite 或 MySQL/MariaDB，供 B 查询；数据默认长期保留。
- B 机器：运行 `b-replicator`，从 C 查询数据，写入本机 MySQL；C 上的数据后续通过独立 cleanup 凭据手动清理。

三个服务都位于仓库 `cmd/` 目录下，并且各自是独立 Go module，可分别构建和部署：

| 机器 | 目录 | Module path | 可执行文件 |
| --- | --- | --- | --- |
| A | `cmd/uploader` | `github.com/houseme/weighing-data-sync/uploader` | `a-uploader.exe` |
| C | `cmd/receiver` | `github.com/houseme/weighing-data-sync/go-receiver` | `go-receiver` / `go-receiver.exe` |
| B | `cmd/replicator` | `github.com/houseme/weighing-data-sync/replicator` | `b-replicator.exe` |

数据库驱动均为纯 Go 驱动，不依赖 CGO、ODBC 或本机 C 编译环境。

## 数据流

```text
    A Windows                         C Linux / Windows                      B Windows
┌────────────────┐              ┌────────────────────┐              ┌────────────────────┐
│ SQL Server      │              │ go-receiver         │              │ b-replicator        │
│ tbl_weightInfo  │              │ SQLite              │              │ MySQL               │
│ tbl_weightPhoto │              │                      │              │                      │
└───────┬────────┘              └─────────┬──────────┘              └─────────┬──────────┘
        │                                  │                                   │
        │ SELECT pending rows              │                                   │
        ▼                                  │                                   │
┌────────────────┐  signed POST             │                                   │
│ a-uploader      ├────────────────────────▶│ wds_receive_records               │
│ state JSONL     │◀────────────────────────┤ accepted_record_keys              │
└────────────────┘                          │                                   │
                                             │ signed GET include_raw=true       │
                                             │◀──────────────────────────────────┤
                                             │──────────────────────────────────▶│ upsert business row
                                             │                                   │
```

核心一致性约定：

- A 只在 C 明确确认后，把 SQL Server 主键写入本地 `STATE_FILE`，并按 `entity_type` 区分称重信息和称重图片。
- C 以 `record_key` 幂等保存 A 上报内容；称重信息使用 `weight_info:<serialNo>`，称重图片使用 `weight_photo:<id>`。
- B 将业务记录幂等写入 MySQL。
- C 不会因 B 同步成功自动删除数据；需要清理时走独立 cleanup 接口和密码，并且只能删除一周以前的数据。
- C 为了让 B 能完整写入 MySQL，必须启用 `STORE_RAW_RECORDS=true`；为了减少 C 数据量，保持 `STORE_RAW_PAYLOAD=false`。

## 交互数据内容

本次数据结构按两份实体类文档落地：

- `tbl_weightInfo`：称重主记录，以 `serialNo` 为系统唯一称重编号，包含车牌、业务类型、运输/发货/收货单位、货物、毛皮净扣实重、单价金额、毛重/皮重磅号和司磅员、称重时间、备用字段、作废/完成/删除状态等。
- `tbl_weightPhoto`：称重图片记录，以 SQL Server `id` 为图片行主键，以 `serialNo` 关联 `tbl_weightInfo`；`captureImage` 二进制内容在 A 中转为 base64 后上报；`imageType` 保存 `gross1`、`tare1`、`grossplateNumber`、`tarecaptureScreen` 等图片类型。

A 上报到 C 的 payload 使用双实体数组：

```json
{
  "source": "sqlserver-shweight",
  "database": "shweight",
  "uploaded_at": "2026-08-28T00:00:00Z",
  "weight_info_records": [],
  "weight_photo_records": []
}
```

C 的接收库保存 `entity_type`、`record_key`、`serial_no`、`plate_no`、时间和原始 JSON，可使用 SQLite 或 MySQL/MariaDB。B 从 C 拉取 `include_raw=true` 后写入三张 MySQL 表：

- `wds_replicated_records`：完整原始 JSON 和中转元数据。
- `wds_weight_info_records`：按 `tbl_weightInfo` 实体字段展开后的业务表。
- `wds_weight_photo_records`：按 `tbl_weightPhoto` 实体字段展开后的图片表，其中 `capture_image_base64` 保存图片 base64。

## 构建

在任意有 Go 1.26+ 的构建机上构建 Windows amd64 可执行文件：

```powershell
cd cmd\uploader
$env:CGO_ENABLED = "0"
$env:GOOS = "windows"
$env:GOARCH = "amd64"
go build -o bin\a-uploader.exe .

cd ..\receiver
$env:CGO_ENABLED = "0"
$env:GOOS = "windows"
$env:GOARCH = "amd64"
go build -o bin\go-receiver.exe .

cd ..\replicator
$env:CGO_ENABLED = "0"
$env:GOOS = "windows"
$env:GOARCH = "amd64"
go build -o bin\b-replicator.exe .
```

如果 C 部署在 Linux x86_64：

```bash
cd cmd/receiver
CGO_ENABLED=0 GOOS=linux GOARCH=amd64 go build -o bin/go-receiver .
```

驱动说明：

| 项目 | 数据库 | 驱动 |
| --- | --- | --- |
| `a-uploader` | SQL Server | `github.com/denisenkom/go-mssqldb` |
| `go-receiver` | SQLite 或 MySQL/MariaDB | `modernc.org/sqlite` / `github.com/go-sql-driver/mysql` |
| `b-replicator` | MySQL | `github.com/go-sql-driver/mysql` |

## 凭据规划

生产建议三套独立凭据，不要共用：

| 方向 | C 上角色 | Token 变量 | HMAC 变量 | 权限 |
| --- | --- | --- | --- | --- |
| A -> C | ingest | `INGEST_API_TOKEN` | `INGEST_SIGN_SECRET` | 只能上报 |
| B -> C | query | `QUERY_API_TOKEN` | `QUERY_SIGN_SECRET` | 只能查询 |
| B -> C | cleanup | `CLEANUP_API_TOKEN` | `CLEANUP_SIGN_SECRET` | 只能删除 |

三个服务使用同一套签名算法：

```text
METHOD
PATH
CANONICAL_QUERY
TIMESTAMP
NONCE
SHA256_BODY_HEX
```

说明：

- `CANONICAL_QUERY` 使用 URL query 排序后的编码结果，排除 `signature` 和 `sign`。
- `POST` 使用真实 body 的 SHA256。
- `GET` / `DELETE` 使用空 body 的 SHA256。
- `X-Timestamp` 是 Unix 秒级时间戳。
- `X-Nonce` 在 C 的签名窗口内不能重复。
- `X-Signature` 是 HMAC-SHA256 小写十六进制。

## 部署 C

C 是中心接收服务。推荐先部署 C，再部署 B，最后部署 A。

Linux 推荐使用仓库脚本安装和管理 systemd：

```bash
cd cmd/receiver
CGO_ENABLED=0 GOOS=linux GOARCH=amd64 go build -o bin/go-receiver .

sudo EXE_PATH="$PWD/bin/go-receiver" \
  SERVER_ADDR=127.0.0.1:18081 \
  DB_DRIVER=mysql \
  MYSQL_DSN='weighing_receiver:password@tcp(127.0.0.1:3306)/weighing_receiver?charset=utf8mb4&parseTime=false&loc=Local' \
  INGEST_API_TOKEN=replace-with-a-token \
  INGEST_SIGN_SECRET=replace-with-a-secret \
  QUERY_API_TOKEN=replace-with-b-query-token \
  QUERY_SIGN_SECRET=replace-with-b-query-secret \
  CLEANUP_API_TOKEN=replace-with-b-cleanup-token \
  CLEANUP_SIGN_SECRET=replace-with-b-cleanup-secret \
  ../../scripts/go-receiver-linux-systemd.sh install
```

常用管理命令：

```bash
sudo ../../scripts/go-receiver-linux-systemd.sh status
sudo ../../scripts/go-receiver-linux-systemd.sh restart
sudo ../../scripts/go-receiver-linux-systemd.sh logs
sudo ../../scripts/go-receiver-linux-systemd.sh uninstall
```

脚本默认：

- 服务名：`go-receiver`
- 程序目录：`/opt/weighing/go-receiver`
- 配置文件：`/etc/weighing/go-receiver.env`
- SQLite：`/var/lib/weighing/go-receiver/receiver.db`
- 监听端口：`127.0.0.1:18081`，公网入口由 Caddy 反向代理
- `STORE_RAW_RECORDS=true`
- `STORE_RAW_PAYLOAD=false`

Caddy 默认站点可只转发 API 路径到 receiver，其余路径继续保留静态页：

```caddyfile
:80 {
  handle /weighing-data-sync/* {
    reverse_proxy 127.0.0.1:18081
  }
  handle /health {
    reverse_proxy 127.0.0.1:18081
  }
  handle {
    root * /data/web/default
    file_server
  }
}
```

再次执行 `install` 会保留已有 `/etc/weighing/go-receiver.env`；确认要重写配置时设置 `OVERWRITE_ENV=1`。卸载默认保留配置和 SQLite 数据；确认删除时可设置 `REMOVE_CONFIG=1 REMOVE_DATA=1`。

Linux systemd 示例：

```ini
[Unit]
Description=Weighing Data Go Receiver
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
WorkingDirectory=/opt/weighing/go-receiver
ExecStart=/opt/weighing/go-receiver/go-receiver
Restart=always
RestartSec=5
Environment=SERVER_ADDR=127.0.0.1:18081
Environment=DB_DRIVER=mysql
Environment=MYSQL_DSN=weighing_receiver:password@tcp(127.0.0.1:3306)/weighing_receiver?charset=utf8mb4&parseTime=false&loc=Local
Environment=SQLITE_PATH=/opt/weighing/go-receiver/data/receiver.db
Environment=STORE_RAW_RECORDS=true
Environment=STORE_RAW_PAYLOAD=false
Environment=MAX_BODY_BYTES=67108864
Environment=INGEST_API_TOKEN=replace-with-a-token
Environment=INGEST_SIGN_SECRET=replace-with-a-secret
Environment=QUERY_API_TOKEN=replace-with-b-query-token
Environment=QUERY_SIGN_SECRET=replace-with-b-query-secret
Environment=CLEANUP_API_TOKEN=replace-with-b-cleanup-token
Environment=CLEANUP_SIGN_SECRET=replace-with-b-cleanup-secret

[Install]
WantedBy=multi-user.target
```

Windows PowerShell 临时启动示例：

```powershell
cd C:\weighing\go-receiver
$env:SERVER_ADDR = "127.0.0.1:18081"
$env:DB_DRIVER = "sqlite"
$env:SQLITE_PATH = "C:\weighing\go-receiver\data\receiver.db"
$env:STORE_RAW_RECORDS = "true"
$env:STORE_RAW_PAYLOAD = "false"
$env:MAX_BODY_BYTES = "67108864"
$env:INGEST_API_TOKEN = "replace-with-a-token"
$env:INGEST_SIGN_SECRET = "replace-with-a-secret"
$env:QUERY_API_TOKEN = "replace-with-b-query-token"
$env:QUERY_SIGN_SECRET = "replace-with-b-query-secret"
$env:CLEANUP_API_TOKEN = "replace-with-b-cleanup-token"
$env:CLEANUP_SIGN_SECRET = "replace-with-b-cleanup-secret"
.\go-receiver.exe
```

C 启动后检查：

```bash
curl -fsS http://c-server:18081/health
```

预期返回 JSON，其中 `status` 为 `ok`。

## 部署 B

B 机器需要提前准备本地 MySQL，并创建目标数据库。`b-replicator` 启动时会自动创建四张表：

- `wds_replicated_records`：保存从 C 拉取的完整业务记录 JSON。
- `wds_weight_info_records`：保存称重信息实体字段。
- `wds_weight_photo_records`：保存称重图片实体字段，图片二进制为 base64。
- `wds_c_delete_queue`：保存异步删除 C 记录的 outbox。

Windows PowerShell 示例：

```powershell
cd C:\weighing\b-replicator
$env:MYSQL_DSN = "user:password@tcp(127.0.0.1:3306)/weighing?charset=utf8mb4&parseTime=true&loc=Local"
$env:C_BASE_URL = "http://c-server:18081"
$env:QUERY_API_TOKEN = "replace-with-b-query-token"
$env:QUERY_SIGN_SECRET = "replace-with-b-query-secret"
$env:CLEANUP_API_TOKEN = "replace-with-b-cleanup-token"
$env:CLEANUP_SIGN_SECRET = "replace-with-b-cleanup-secret"
$env:FETCH_BATCH_SIZE = "100"
$env:FETCH_INTERVAL_SECONDS = "5"
$env:DELETE_INTERVAL_SECONDS = "2"
.\b-replicator.exe
```

部署注意：

- C 必须启用 `STORE_RAW_RECORDS=true`，否则 B 会拒绝没有完整 `record` JSON 的响应。
- MySQL 表的 `record_key` 有唯一索引，重复从 C 拉取不会重复写业务记录。
- B 不再把清理 C 数据作为同步成功的必要步骤；C 数据按手动 cleanup 策略保留。
- B 已经把 JSON 同步拆入 `wds_weight_info_records` / `wds_weight_photo_records`，同时保留 `wds_replicated_records.raw_record` 便于追溯和后续字段校正。

## 部署 A

A 机器需要能访问 SQL Server 和 C。`a-uploader` 通过本地 JSONL 文件记录 C 已确认的 SQL Server 主键。

Windows PowerShell 示例：

```powershell
cd C:\weighing\a-uploader
$env:C_ENDPOINT = "http://c-server:18081/weighing-data-sync/put"
$env:INGEST_API_TOKEN = "replace-with-a-token"
$env:INGEST_SIGN_SECRET = "replace-with-a-secret"
$env:SQLSERVER_HOST = "127.0.0.1"
$env:SQLSERVER_PORT = "1433"
$env:SQLSERVER_DATABASE = "shweight"
$env:SQLSERVER_USERNAME = "sa"
$env:SQLSERVER_PASSWORD = "replace-with-sqlserver-password"
$env:SQLSERVER_SCHEMA = "dbo"
$env:SQLSERVER_INFO_TABLE = "tbl_weightInfo"
$env:SQLSERVER_PHOTO_TABLE = "tbl_weightPhoto"
$env:SQLSERVER_INFO_PRIMARY_KEY = "serialNo"
$env:SQLSERVER_PHOTO_PRIMARY_KEY = "id"
$env:SQLSERVER_INFO_SERIAL_COLUMN = "serialNo"
$env:SQLSERVER_PHOTO_SERIAL_COLUMN = "serialNo"
$env:STATE_FILE = "C:\weighing\a-uploader\data\a-uploader-state.jsonl"
$env:BATCH_SIZE = "100"
$env:POLL_INTERVAL_SECONDS = "30"
.\a-uploader.exe
```

如果现场 SQL Server 的待上传条件不同，可以设置：

```powershell
$env:SQLSERVER_INFO_PENDING_WHERE = "ISNULL([isUploadCloud], 0) = 0 AND ISNULL([del_flag], 0) = 0"
$env:SQLSERVER_PHOTO_PENDING_WHERE = "ISNULL([isUploadCloud], 0) = 0 AND ISNULL([delFlag], 0) = 0"
```

A 状态文件约定：

- 只记录 C 已确认 accepted 的主键。
- 追加写入后执行文件同步，减少断电导致的确认丢失。
- 不要删除 `STATE_FILE`，除非明确要重放历史数据。
- 如果 A 在 C 已写入后、本地状态文件写入前崩溃，下一轮可能重发；C 通过 `record_key` upsert，B 通过 MySQL 唯一键 upsert，重复发送是安全的。

## Windows 常驻运行

三个 Go 程序都可以作为普通 exe 运行。A/B 当前不是原生 Windows Service Control Manager 程序，因此仓库提供的服务脚本使用 Windows 计划任务承载，效果是开机自动启动、失败可重试、可手工启动/停止，不额外依赖 NSSM。

### 安装 A 开机自启

```powershell
PowerShell -ExecutionPolicy Bypass -File .\scripts\install-a-uploader-windows-service.ps1 `
  -ExePath .\a-uploader.exe `
  -CEndpoint "http://c-server/weighing-data-sync/put" `
  -SqlServerHost "127.0.0.1" `
  -SqlServerDatabase "shweight" `
  -SqlServerUsername "sa"
```

脚本会交互输入 `INGEST_API_TOKEN`、`INGEST_SIGN_SECRET` 和 SQL Server 密码，随后安装到：

- 程序目录：`C:\Program Files\WeighingAUploader`
- 数据目录：`C:\ProgramData\WeighingAUploader`
- 状态文件：`C:\ProgramData\WeighingAUploader\data\a-uploader-state.jsonl`
- 日志目录：`C:\ProgramData\WeighingAUploader\logs`
- 计划任务：`WeighingAUploader`

卸载 A：

```powershell
PowerShell -ExecutionPolicy Bypass -File .\scripts\uninstall-a-uploader-windows-service.ps1
```

默认保留 `C:\ProgramData\WeighingAUploader`，避免误删状态文件；确认要删除数据时再加 `-RemoveData`。

### 安装 B 开机自启

```powershell
PowerShell -ExecutionPolicy Bypass -File .\scripts\install-b-replicator-windows-service.ps1 `
  -ExePath .\b-replicator.exe `
  -CBaseUrl "http://c-server"
```

脚本会交互输入 `MYSQL_DSN`、`QUERY_API_TOKEN`、`QUERY_SIGN_SECRET`、`CLEANUP_API_TOKEN`、`CLEANUP_SIGN_SECRET`。`MYSQL_DSN` 示例：

```text
root:root@tcp(127.0.0.1:3306)/weighing?charset=utf8mb4&parseTime=true&loc=Local
```

B 安装位置：

- 程序目录：`C:\Program Files\WeighingBReplicator`
- 日志目录：`C:\ProgramData\WeighingBReplicator\logs`
- 计划任务：`WeighingBReplicator`
- `AUTO_MIGRATE=true` 默认开启，目标 database 已存在时会自动创建或检查四张 MySQL 表。

卸载 B：

```powershell
PowerShell -ExecutionPolicy Bypass -File .\scripts\uninstall-b-replicator-windows-service.ps1
```

默认保留 `C:\ProgramData\WeighingBReplicator` 日志；确认要删除时再加 `-RemoveData`。

常用运维命令：

```powershell
Start-ScheduledTask -TaskName WeighingAUploader
Stop-ScheduledTask -TaskName WeighingAUploader
Get-ScheduledTask -TaskName WeighingAUploader

Start-ScheduledTask -TaskName WeighingBReplicator
Stop-ScheduledTask -TaskName WeighingBReplicator
Get-ScheduledTask -TaskName WeighingBReplicator
```

脚本会把生产环境变量写入安装目录下受限 ACL 的 `.env`，真实密钥不要提交到仓库。

## 启动顺序

1. 启动 C，并确认 `/health` 返回 `ok`。
2. 启动 B，确认能连接 MySQL，日志出现 `b replicator started`。
3. 启动 A，确认能连接 SQL Server，并开始上报。
4. 观察 C 的 SQLite 数据量先增长。
5. 观察 B 的 MySQL `wds_replicated_records` 增长。
6. 再查 C，确认数据仍保留，可供后续追溯或手动清理。

## 验收

C 健康检查：

```bash
curl -fsS http://c-server:18081/health
```

B MySQL 检查：

```sql
SELECT COUNT(*) FROM wds_replicated_records;
SELECT record_key, c_record_id, replicated_at
FROM wds_replicated_records
ORDER BY replicated_at DESC
LIMIT 10;
```

A 状态文件检查：

```powershell
Get-Content C:\weighing\a-uploader\data\a-uploader-state.jsonl -Tail 10
```

C 保留检查：

- B 正常工作时，C 的接收数据仍会保留。
- `wds_receive_batches.raw_payload` 默认为空。
- `wds_receive_records.raw_record` 在 `STORE_RAW_RECORDS=true` 时保留，供 B 拉取和后续追溯。
- cleanup 接口必须使用独立 `CLEANUP_API_TOKEN` / `CLEANUP_SIGN_SECRET`，且只能删除一周以前的数据。

## 升级和回滚

升级建议：

1. 先停 A，避免继续向 C 写入新数据。
2. 等 B 把 C 中已有数据尽量消费。
3. 停 B。
4. 升级 C，保留 SQLite 数据文件。
5. 启动 C 并检查 `/health`。
6. 升级并启动 B。
7. 升级并启动 A。

回滚原则：

- A 回滚时保留 `STATE_FILE`，否则会重放已确认记录。
- B 回滚时保留 MySQL 业务表和 `wds_c_delete_queue`。
- C 回滚时保留 SQLite 数据文件和 WAL 文件。
- 如需更换签名密钥，应先在 C 上更新，再同步更新 A/B，避免 401。

## 常见问题

### A 上报返回 401

检查 A 的 `INGEST_API_TOKEN` / `INGEST_SIGN_SECRET` 是否和 C 一致，确认 A/C 系统时间差没有超过 `SIGN_SKEW_SECONDS`。

### B 报 `has no raw record`

C 没有启用 `STORE_RAW_RECORDS=true`，或历史数据是在启用前写入的。启用后让 A 重发缺失记录，或人工补齐后再让 B 消费。

### C 数据没有下降

这是当前设计内行为。C 默认保留接收到的数据，需要人工调用 cleanup 接口清理；服务会拒绝删除一周以内的数据。

### A 重启后重复上报

确认 `STATE_FILE` 使用绝对路径，且计划任务的运行账号对该目录有读写权限。相对路径会受 `WorkingDirectory` 影响。

### MySQL 写入成功但 C 仍保留数据

这是设计内行为。C 的清理是独立运维动作，不影响 B 业务表幂等写入。
