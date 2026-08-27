# Go SQLite Receiver

这是一个部署在 C 机器上的独立 Go 接收服务：A 机器主动上报称重 JSON，服务验签鉴权后幂等写入 SQLite；B 机器可以查询数据，并按数据库 `id` 或业务唯一标识清理记录。目录内是单独的 Go module，不依赖 Rust 代码；SQLite driver 使用 `modernc.org/sqlite`，纯 Go 实现，不需要 CGO。

## 功能

- `POST /weighing-data-sync/put`：接收批量 JSON，上报体兼容现有 Rust 同步协议。
- `GET /weighing-data-sync/records`：按 `id` / `record_key` / `plateNo` / `serialNo` / `from` / `to` 查询。
- `DELETE /weighing-data-sync/records/{id}`：B 机器按 SQLite 自增 `id` 清理单条记录。
- `DELETE /weighing-data-sync/records/by-key/{record_key}`：B 机器按业务唯一标识清理单条记录。
- `DELETE /weighing-data-sync/records?id=...` 或 `?record_key=...`：兼容 query 形式清理。
- SQLite 幂等落库：表内都有 `INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id >= 0)` 自增 `id`；业务幂等优先使用 `serialNo`，没有 `serialNo` 时退化使用来源 `id` 或 `ticket_no`。
- 最小存储：默认只保存查询/清理必要字段，不保存完整原始记录和完整批次 payload；需要审计时可显式打开。
- 鉴权分权：A 写入、B 查询、B 清理分别配置 Bearer Token 和 HMAC-SHA256 签名密钥，默认要求两者同时存在。
- 自动建表：默认启动时创建 `wds_receive_records` 和 `wds_receive_batches`。

## 运行

启动服务：

```bash
cd go-receiver
export SQLITE_PATH='data/receiver.db'
export INGEST_API_TOKEN='a-write-token'
export INGEST_SIGN_SECRET='a-write-sign-secret'
export QUERY_API_TOKEN='b-read-token'
export QUERY_SIGN_SECRET='b-read-sign-secret'
export CLEANUP_API_TOKEN='b-delete-token'
export CLEANUP_SIGN_SECRET='b-delete-sign-secret'
go run ./cmd/receiver
```

生产默认 `REQUIRE_AUTH=true`，每个角色都必须同时配置 Bearer Token 和 HMAC 签名密钥。`API_TOKEN` / `SIGN_SECRET` 仍可作为三类角色的兼容兜底，但不建议生产共用。

## 配置项

| 环境变量 | 默认值 | 说明 |
| --- | --- | --- |
| `SQLITE_PATH` | `data/receiver.db` | SQLite 数据库文件路径，会自动创建父目录 |
| `SQLITE_DSN` | 空 | SQLite driver DSN；设置后优先于 `SQLITE_PATH`，例如 `file:data/receiver.db?cache=shared` |
| `SERVER_ADDR` | `:18081` | HTTP 监听地址 |
| `POST_ROUTE` | `/weighing-data-sync/put` | POST 接收路径 |
| `QUERY_ROUTE` | `/weighing-data-sync/records` | GET 查询路径 |
| `INGEST_API_TOKEN` / `INGEST_SIGN_SECRET` | 空 | A 机器上报专用 Bearer Token / HMAC 密钥 |
| `QUERY_API_TOKEN` / `QUERY_SIGN_SECRET` | 空 | B 机器查询专用 Bearer Token / HMAC 密钥 |
| `CLEANUP_API_TOKEN` / `CLEANUP_SIGN_SECRET` | 空 | B 机器清理专用 Bearer Token / HMAC 密钥 |
| `API_TOKEN` / `SIGN_SECRET` | 空 | 兼容兜底：角色专用变量未设置时使用 |
| `REQUIRE_AUTH` | `true` | 启动时强制校验三类角色都配置 Token 和签名密钥 |
| `SIGN_SKEW_SECONDS` | `300` | 签名时间戳允许偏移 |
| `MAX_BODY_BYTES` | `16777216` | 最大请求体大小 |
| `QUERY_DEFAULT_LIMIT` | `100` | 默认查询数量 |
| `QUERY_MAX_LIMIT` | `1000` | 最大查询数量 |
| `STORE_RAW_RECORDS` | `false` | 是否保存每条完整原始记录 JSON |
| `STORE_RAW_PAYLOAD` | `false` | 是否保存每批完整原始 payload JSON |
| `RETURN_RAW_RECORDS` | `false` | 查询结果是否默认返回 `record` 原始 JSON；也可用 `include_raw=true` 临时打开 |
| `AUTO_MIGRATE` | `true` | 启动时自动建表 |
| `SQLITE_MAX_OPEN_CONNS` | `1` | SQLite 最大连接数，默认串行写入，避免锁竞争 |
| `SQLITE_MAX_IDLE_CONNS` | `1` | SQLite 空闲连接数 |

## POST 示例

```bash
curl -sS -X POST 'http://127.0.0.1:18081/weighing-data-sync/put' \
  -H 'Authorization: Bearer a-write-token' \
  -H 'X-Timestamp: 1787846400' \
  -H 'X-Nonce: unique-nonce-from-a' \
  -H 'X-Signature: <hex-hmac-signature>' \
  -H 'Content-Type: application/json' \
  -d '{
    "source": "sqlserver-yunfu-tbl_weightInfo",
    "database": "yunfu",
    "table": "tbl_weightInfo",
    "uploaded_at": "2026-07-28T08:00:00Z",
    "records": [
      {
        "serialNo": "202607280001",
        "plateNo": "皖H12345",
        "grossWeight": "12500.00",
        "tareWeight": "5000.00",
        "netWeight": "7500.00",
        "updateTime": "2026-07-28 08:10:30"
      }
    ]
  }'
```

响应：

```json
{
  "accepted": true,
  "accepted_serial_nos": ["202607280001"],
  "failed_serial_nos": [],
  "records_count": 1
}
```

## GET 查询示例

```bash
curl -sS 'http://127.0.0.1:18081/weighing-data-sync/records?plateNo=皖H12345&from=2026-07-28%2000:00:00&to=2026-07-29%2000:00:00&limit=20' \
  -H 'Authorization: Bearer b-read-token' \
  -H 'X-Timestamp: 1787846400' \
  -H 'X-Nonce: unique-query-nonce-from-b' \
  -H 'X-Signature: <hex-hmac-signature>'
```

也可以按流水号查：

```bash
curl -sS 'http://127.0.0.1:18081/weighing-data-sync/records?serialNo=202607280001' \
  -H 'Authorization: Bearer b-read-token' \
  -H 'X-Timestamp: 1787846400' \
  -H 'X-Nonce: unique-query-nonce-from-b-2' \
  -H 'X-Signature: <hex-hmac-signature>'
```

## DELETE 清理示例

按 SQLite 自增 `id` 清理：

```bash
curl -sS -X DELETE 'http://127.0.0.1:18081/weighing-data-sync/records/123' \
  -H 'Authorization: Bearer b-delete-token' \
  -H 'X-Timestamp: 1787846400' \
  -H 'X-Nonce: unique-delete-nonce-from-b' \
  -H 'X-Signature: <hex-hmac-signature>'
```

按业务唯一键清理：

```bash
curl -sS -X DELETE 'http://127.0.0.1:18081/weighing-data-sync/records/by-key/202607280001' \
  -H 'Authorization: Bearer b-delete-token' \
  -H 'X-Timestamp: 1787846400' \
  -H 'X-Nonce: unique-delete-nonce-from-b-2' \
  -H 'X-Signature: <hex-hmac-signature>'
```

## 签名规则

所有受保护接口都使用同一套签名算法，但不同角色使用不同密钥。签名参数可以放在 Header，也可以放在 query：

- `X-Timestamp` 或 `timestamp`：Unix 秒级时间戳。
- `X-Nonce` 或 `nonce`：随机字符串，同一服务进程内、有效期内不可重复。
- `X-Signature` 或 `signature` / `sign`：小写十六进制 HMAC-SHA256。

签名原文：

```text
METHOD
PATH
CANONICAL_QUERY
TIMESTAMP
NONCE
SHA256_BODY_HEX
```

其中 `CANONICAL_QUERY` 会排序 query 参数，并排除 `signature` / `sign`；GET 和 DELETE 的 body hash 使用空 body。

Go 生成签名示例：

```go
bodyHash := sha256.Sum256(body)
canonical := strings.Join([]string{
    "POST",
    "/weighing-data-sync/put",
    "",
    timestamp,
    nonce,
    hex.EncodeToString(bodyHash[:]),
}, "\n")
mac := hmac.New(sha256.New, []byte(secret))
mac.Write([]byte(canonical))
signature := hex.EncodeToString(mac.Sum(nil))
```

## 表结构

服务使用两张表：

- `wds_receive_records`：每条称重记录一行，`id` 为非负自增主键，`record_key` 为唯一业务幂等键；抽取 `serial_no`、`plate_no`、`source_time` 用于查询；`raw_record` 默认为空，仅在 `STORE_RAW_RECORDS=true` 时保存。
- `wds_receive_batches`：每次 POST 请求一行，`id` 为非负自增主键，`request_id` 为唯一请求键，用于追踪批次、来源和数量；`raw_payload` 默认为空，仅在 `STORE_RAW_PAYLOAD=true` 时保存。

建表 SQL 见 [migrations/001_init.sql](migrations/001_init.sql)。
