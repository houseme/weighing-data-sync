# HTTP 上传协议

云端接收端点默认 `POST https://api.xmb.xyz/weighing-data-sync/put`（本地接收服务为
`POST <bind>/weighing-data-sync/put`）。两条同步链路使用同一协议，仅在 `source` 字段上区分。

## 请求

```http
POST /weighing-data-sync/put
Authorization: Bearer <api-key>        ← 设置 api_key 时必填，否则 401
Content-Type: application/json
```

### 本地缓存链路（`source = "local"`）请求体

```json
{
  "source": "weighing-data-sync",
  "uploaded_at": "2026-07-28T08:00:00Z",
  "records": [
    {
      "id": "03825b1a-b925-4592-afd4-b982c5ce21de",
      "ticket_no": "202607280001",
      "scale_no": "1",
      "plate_no": "皖H12345",
      "weight_kg": 7500.0,
      "weight_lb": 16534.67,
      "original_unit": "kilogram",
      "measured_at": "2026-07-28T08:00:00Z"
    }
  ]
}
```

### SQL Server 链路（`source = "sqlserver"`）请求体

`records` 为 `tbl_weightInfo` 行的动态 JSON（字段名与原表一致，camelCase）：

```json
{
  "source": "sqlserver-yunfu-tbl_weightInfo",
  "database": "yunfu",
  "table": "tbl_weightInfo",
  "uploaded_at": "2026-07-28T08:00:00Z",
  "records": [
    {
      "serialNo": "202607280001",
      "plateNo": "皖H12345",
      "weightType": "销售",
      "goodsName": "砂石",
      "grossWeight": "12500.00",
      "tareWeight": "5000.00",
      "netWeight": "7500.00",
      "actualWeight": "7500.00",
      "weightUnit": "kg",
      "grossTime": "2026-07-28 08:00:00",
      "tareTime": "2026-07-28 08:10:00",
      "updateTime": "2026-07-28 08:10:30",
      "isUploadCloud": 0,
      "del_flag": 0
    }
  ]
}
```

字段说明：

- `Numeric`/`Decimal` 以**字符串**保留精度（如 `"12500.00"`），避免浮点丢失。
- 二进制（`Binary`）以 `"<N bytes>"` 占位，不直接传输。
- `DateTime*` 统一格式化为 `"YYYY-MM-DD HH:MM:SS"`。

## 响应

```json
{
  "request_id": "03825b1a-b925-4592-afd4-b982c5ce21de",
  "accepted": true,
  "accepted_serial_nos": ["202607280001"],
  "failed_serial_nos": [],
  "records_count": 1,
  "received_at": "2026-07-28T14:50:17.240835+00:00"
}
```

## 客户端处理约定

- **204 No Content** 或**空响应体** → 视为整批成功。
- **2xx 且有响应体** → 解析 `accepted_serial_nos` / `failed_serial_nos`：
  - 全部为空时按整批成功处理；
  - 否则仅对 `accepted_serial_nos` 执行回写，`failed_serial_nos` 计入失败。
- **4xx（除 408/429）** → 永久失败，立即放弃，不重试。
- **5xx / 408 / 429 / 网络错误** → transient，按指数退避重试，直到预算耗尽。
- 成功且 `mark_uploaded = true` 时，对 SQL Server 链路执行参数化批量回写：

```sql
UPDATE [tbl_weightInfo] SET [isUploadCloud] = 1 WHERE [serialNo] IN (@P1, @P2, ...)
```

回写按 1000 条分块，规避 TDS 2100 参数上限。

## 接收服务（本地 `[server]`）

当 `server.enabled = true`（或 `serve` 子命令）时，同一二进制可作为接收端：

- `server.api_key` 设置后强制 Bearer 鉴权，否则 401。
- `records` 缺失或为空返回 400。
- `server.persist = true` 时，每批以原始 JSON 写入本地 `inbound_payloads` 表（按请求整条落库），
  实现本地先持久化；落库失败仅记日志，不影响返回 200。

> 注：接收服务默认**只记录日志**（`persist = false`），不写本地缓存。如需本地先持久化，
> 显式开启 `persist`。
