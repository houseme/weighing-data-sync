# A SQL Server Uploader

`a-uploader` is a standalone Go executable for machine A. It reads pending `tbl_weightInfo` rows from SQL Server, signs a `POST` upload to C, and records only C-confirmed SQL Server primary keys in a local JSONL state file. It is intended to run directly as a normal Windows `.exe`; it has no cgo dependency.

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
| `SQLSERVER_TABLE` | `tbl_weightInfo` | Source table. |
| `SQLSERVER_PRIMARY_KEY` | `serialNo` | Stable SQL Server row key persisted in the state file. |
| `SQLSERVER_SERIAL_COLUMN` | `serialNo` | Column matched with C's `accepted_serial_nos`. |
| `SQLSERVER_PENDING_WHERE` | default pending predicate | Trusted local SQL predicate for pending rows. |
| `BATCH_SIZE` | `100` | Maximum records selected per cycle. |
| `POLL_INTERVAL_SECONDS` | `30` | Delay after each cycle. |
| `HTTP_TIMEOUT_SECONDS` | `30` | SQL Server query and C request deadline. |
| `STATE_FILE` | `data/a-uploader-state.jsonl` | Append-only confirmation log. Use an absolute path for a service install. |
| `SOURCE_NAME` | `sqlserver-<database>-<table>` | Payload `source` value. |
| `RUN_ONCE` | `false` | One-cycle mode for a scheduled task or diagnostics. |

## Authentication and delivery

Every upload sends Bearer authentication plus `X-Timestamp`, `X-Nonce`, and lowercase-hex `X-Signature`. HMAC-SHA256 signs exactly `METHOD`, `PATH`, sorted canonical query, timestamp, nonce, and SHA256 body hash, each separated by a newline. This matches `go-receiver`; the upload method is `POST`, because C registers its upload route as POST.

The JSONL state is appended only after C confirms success. A 204, an empty successful response, or a successful response with both acknowledgement lists empty confirms the whole submitted batch. Otherwise only records whose `serialNo` occurs in `accepted_serial_nos` are recorded. If A exits after C commits but before the local append, retrying is safe because C upserts by record key. Do not delete the state file unless intentionally replaying confirmed source rows.
