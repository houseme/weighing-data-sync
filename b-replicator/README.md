# B Replicator

`b-replicator` is the standalone Go program for the B Windows machine. It fetches raw records from C, writes them idempotently to local MySQL, then deletes C records through a separate asynchronous worker.

It uses the pure-Go `github.com/go-sql-driver/mysql` driver and the standard-library HTTP client. C must be started with `STORE_RAW_RECORDS=true`: B always requests `include_raw=true`, and refuses to queue cleanup when C does not return the full `record` JSON.

## Delivery guarantee

The raw-record upsert, typed business-table upsert, and insert into `wds_c_delete_queue` share one MySQL transaction. A crash before commit leaves neither durable; a crash after commit leaves a durable cleanup job. Re-fetching is safe because `record_key` is unique in all replicated tables. Delete failures retry with exponential backoff from 1 to 128 seconds.

## Run

```powershell
cd b-replicator
$env:MYSQL_DSN = 'user:password@tcp(127.0.0.1:3306)/weighing?charset=utf8mb4&parseTime=true&loc=Local'
$env:C_BASE_URL = 'http://c-server:18081'
$env:QUERY_API_TOKEN = 'b-read-token'
$env:QUERY_SIGN_SECRET = 'b-read-sign-secret'
$env:CLEANUP_API_TOKEN = 'b-delete-token'
$env:CLEANUP_SIGN_SECRET = 'b-delete-sign-secret'
go run ./cmd/b-replicator
```

Windows build:

```powershell
go build -o b-replicator.exe ./cmd/b-replicator
```

## Configuration

| Variable | Default | Description |
| --- | --- | --- |
| `MYSQL_DSN` | required | `go-sql-driver/mysql` DSN |
| `C_BASE_URL` | required | C receiver base URL |
| `C_QUERY_ROUTE` | `/weighing-data-sync/records` | C GET route |
| `C_CLEANUP_ROUTE` | `/weighing-data-sync/records` | C DELETE route; B appends the C id |
| `QUERY_API_TOKEN` / `QUERY_SIGN_SECRET` | required | C query credentials |
| `CLEANUP_API_TOKEN` / `CLEANUP_SIGN_SECRET` | required | C cleanup credentials |
| `FETCH_BATCH_SIZE` | `100` | C records per fetch/delete pass |
| `FETCH_INTERVAL_SECONDS` | `5` | Delay between fetch passes |
| `DELETE_INTERVAL_SECONDS` | `2` | Delay between cleanup passes |
| `HTTP_TIMEOUT_SECONDS` | `30` | Per C request timeout |
| `DB_TIMEOUT_SECONDS` | `30` | Per MySQL operation timeout |
| `AUTO_MIGRATE` | `true` | Create B's two tables at startup |

## Authentication and signature

Query and cleanup use distinct Bearer tokens and HMAC secrets. The canonical HMAC payload exactly matches C's `go-receiver`:

```text
METHOD
PATH
CANONICAL_QUERY
TIMESTAMP
NONCE
SHA256_BODY_HEX
```

Query values are sorted and URL-escaped; `signature` and `sign` are excluded. GET and DELETE use the SHA-256 digest of an empty request body.

## Local tables

- `wds_replicated_records` contains the full raw record JSON, keyed by `record_key`, with `entity_type` identifying `weight_info` or `weight_photo`.
- `wds_weight_info_records` contains the fields from `tbl_weightInfo`, including serial number, plate number, goods, weight values, fee/amount fields, timestamps, backup fields, finish/cancel/delete flags, and raw JSON.
- `wds_weight_photo_records` contains the fields from `tbl_weightPhoto`, including source image id, `serialNo`, base64 `captureImage`, plate number, image type, upload/delete flags, client id, consignee unit, forwarding unit, and raw JSON.
- `wds_c_delete_queue` is the durable cleanup outbox. Rows with `status='failed'` retain the latest error and are retried automatically.
