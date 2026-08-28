# A SQL Server Uploader

`a-uploader` is a standalone Go executable for machine A. It reads pending `tbl_weightInfo` weighing rows and `tbl_weightPhoto` image rows from SQL Server, signs a `POST` upload to C, and records only C-confirmed SQL Server primary keys in a local JSONL state file. It is intended to run directly as a normal Windows `.exe`; it has no cgo dependency.

## Build

```powershell
cd a-uploader
go build -o bin\a-uploader.exe .\cmd\a-uploader
```

The only database driver is the pure-Go SQL Server driver `github.com/denisenkom/go-mssqldb`.

## Required configuration

```powershell
$env:C_ENDPOINT = "http://c-server:18081/weighing-data-sync/put"
$env:INGEST_API_TOKEN = "replace-with-c-ingest-token"
$env:INGEST_SIGN_SECRET = "replace-with-c-ingest-hmac-secret"
$env:SQLSERVER_HOST = "127.0.0.1"
$env:SQLSERVER_PORT = "1433"
$env:SQLSERVER_DATABASE = "yunfu"
$env:SQLSERVER_USERNAME = "sa"
$env:SQLSERVER_PASSWORD = "replace-me"
.\bin\a-uploader.exe
```

`C_ENDPOINT` must point at C's configured `POST_ROUTE` (the current receiver default is `/weighing-data-sync/put`). `INGEST_API_TOKEN` and `INGEST_SIGN_SECRET` correspond to C's same-named ingest credentials.

## Optional configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `SQLSERVER_DSN` | unset | Complete `sqlserver://` DSN; takes precedence over individual connection variables. |
| `SQLSERVER_SCHEMA` | `dbo` | Source schema. |
| `SQLSERVER_INFO_TABLE` | `tbl_weightInfo` | Weighing information table. Legacy `SQLSERVER_TABLE` is still accepted as a fallback. |
| `SQLSERVER_PHOTO_TABLE` | `tbl_weightPhoto` | Weighing image table. |
| `SQLSERVER_INFO_PRIMARY_KEY` | `serialNo` | Stable weighing-information row key persisted in the state file. Legacy `SQLSERVER_PRIMARY_KEY` is still accepted as a fallback. |
| `SQLSERVER_PHOTO_PRIMARY_KEY` | `id` | Stable image row key persisted in the state file. |
| `SQLSERVER_INFO_SERIAL_COLUMN` | `serialNo` | Weighing serial number column in `tbl_weightInfo`. Legacy `SQLSERVER_SERIAL_COLUMN` is still accepted as a fallback. |
| `SQLSERVER_PHOTO_SERIAL_COLUMN` | `serialNo` | Weighing serial number column in `tbl_weightPhoto`. |
| `SQLSERVER_INFO_PENDING_WHERE` | default info predicate | Trusted local SQL predicate for pending `tbl_weightInfo` rows. Legacy `SQLSERVER_PENDING_WHERE` is still accepted as a fallback. |
| `SQLSERVER_PHOTO_PENDING_WHERE` | default photo predicate | Trusted local SQL predicate for pending `tbl_weightPhoto` rows. |
| `BATCH_SIZE` | `100` | Maximum records selected per cycle. |
| `POLL_INTERVAL_SECONDS` | `30` | Delay after each cycle. |
| `HTTP_TIMEOUT_SECONDS` | `30` | SQL Server query and C request deadline. |
| `STATE_FILE` | `data/a-uploader-state.jsonl` | Append-only confirmation log. Use an absolute path for a service install. |
| `SOURCE_NAME` | `sqlserver-<database>-<table>` | Payload `source` value. |
| `RUN_ONCE` | `false` | One-cycle mode for a scheduled task or diagnostics. |

## Authentication and delivery

Every upload sends Bearer authentication plus `X-Timestamp`, `X-Nonce`, and lowercase-hex `X-Signature`. HMAC-SHA256 signs exactly `METHOD`, `PATH`, sorted canonical query, timestamp, nonce, and SHA256 body hash, each separated by a newline. This matches `go-receiver`; the upload method is `POST`, because C registers its upload route as POST.

The JSONL state is appended only after C confirms success. A 204, an empty successful response, or a successful response with both acknowledgement lists empty confirms the whole submitted batch. Otherwise records are matched with `accepted_record_keys`; old `accepted_serial_nos` responses are still accepted for weighing-information rows. Record keys are namespaced as `weight_info:<serialNo>` and `weight_photo:<id>`, so SQL Server image IDs cannot collide with weighing serial numbers. Binary SQL Server image columns such as `captureImage` are sent as base64 strings in JSON. If A exits after C commits but before the local append, retrying is safe because C upserts by record key. Do not delete the state file unless intentionally replaying confirmed source rows.
