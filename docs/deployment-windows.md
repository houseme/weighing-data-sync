# Windows EXE 现场部署指南

本文档面向最终在 Windows 电脑上部署 `sync-daemon.exe` 的场景。现场 Windows 电脑已经有
SQL Server 服务，本项目只部署同步程序：从本机或局域网 SQL Server 读取
`yunfu.dbo.tbl_weightInfo`，上报到云端接口，成功后回写 `isUploadCloud = 1`。

## 交付物

部署前需要提前准备这些文件：

| 文件 | 来源 | 说明 |
| --- | --- | --- |
| `sync-daemon.exe` | 编译产物 | 必需，最终运行的同步程序 |
| `scripts/install-windows.ps1` | 仓库脚本 | 必需，安装 exe、配置、计划任务 |
| `scripts/uninstall-windows.ps1` | 仓库脚本 | 建议携带，现场回滚 / 卸载 |
| `config/production.toml` | 模板生成或手工配置 | 可选，安装脚本会自动生成 |
| `deployment-windows.md` | 本文档 | 建议随包携带 |

推荐通过打包脚本生成：

```powershell
.\scripts\build-windows-release.ps1 -StaticCrt
```

GitHub Actions 的 `build.yml` 在 Windows 构建时会同时上传完整 `.zip` 部署包和独立
`.exe` 二进制文件，独立文件名形如 `weighing-data-sync-v0.1.0-windows-x86_64.exe`。

输出目录：

```text
dist\weighing-data-sync-windows-x64\
```

`-StaticCrt` 会尽量静态链接 MSVC CRT，减少现场安装 Visual C++ Runtime 的需求。

## 需要提前编译哪些代码

生产部署只需要提前编译主二进制：

```powershell
cargo build --locked --release --target x86_64-pc-windows-msvc --features windows
```

编译结果：

```text
target\x86_64-pc-windows-msvc\release\sync-daemon.exe
```

说明：

- 必须编译 `sync-daemon` 这个 bin，它包含 SQL Server 读取、HTTP 上报、SQLite 本地缓存、
  cron 调度和 Windows 自启动入口。
- 建议启用 `windows` feature，用于 Windows 注册表 / 服务相关入口。
- 默认特性已经包含 `sqlite` / `http` / `cron`，不要关闭。
- 不需要把 Docker SQL Server E2E 镜像放到现场；Docker 夹具只用于开发/验收模拟。
- 如果云端明确支持 gzip 请求体，可额外启用 `compression`：

```powershell
cargo build --locked --release --target x86_64-pc-windows-msvc --features windows,compression
```

### 编译机要求

推荐直接在 Windows 编译机上构建：

- Windows 10 / Windows Server 2016 及以上。
- Rust 1.95+。
- Visual Studio Build Tools 2022，安装 `Desktop development with C++` 工作负载。
- Windows SDK。

安装目标：

```powershell
rustup target add x86_64-pc-windows-msvc
```

macOS / Linux 交叉编译到 `x86_64-pc-windows-msvc` 需要额外准备 MSVC 兼容 linker 和 Windows SDK，
不建议作为常规交付路径。

## Windows 现场电脑依赖

现场电脑需要准备：

| 依赖 | 是否必需 | 说明 |
| --- | --- | --- |
| Windows 10 / Windows Server 2016+ | 必需 | 建议 64 位系统 |
| SQL Server 服务 | 必需 | 已由现场提供，本程序通过 TCP 读取 |
| SQL Server TCP/IP | 必需 | SQL Server Configuration Manager 中启用 TCP/IP，默认端口 1433 |
| PowerShell 5.1+ | 必需 | Windows 自带，用于安装脚本和计划任务 |
| 管理员权限 | 必需 | 创建 `Program Files` 目录和计划任务需要 |
| 出站 HTTPS 网络 | 必需 | 能访问云端 `api.endpoint` |
| Visual C++ 2015-2022 Redistributable | 条件必需 | 若 exe 未使用 `-StaticCrt` 构建，建议安装 |

不需要安装：

- SQL Server ODBC Driver。
- `sqlcmd`。
- .NET Runtime。
- Java。
- Docker。

本程序使用 Rust `tiberius` 通过 TDS 协议直连 SQL Server，只要求 SQL Server TCP 端口可达。

## 推荐目录

安装脚本默认使用：

```text
C:\Program Files\WeighingDataSync\sync-daemon.exe
C:\Program Files\WeighingDataSync\.env
C:\Program Files\WeighingDataSync\run-weighing-data-sync.ps1
C:\ProgramData\WeighingDataSync\config\production.toml
C:\ProgramData\WeighingDataSync\data\weighing_sync.db
C:\ProgramData\WeighingDataSync\logs\sync-daemon.log
```

`.env` 存放 SQL Server 密码和可选 API Key，安装脚本会把 ACL 限制为 `SYSTEM` 和本机管理员组。

## SQL Server 前置确认

现场 SQL Server 需要满足：

- `tbl_weightInfo` 表存在，默认库名 `yunfu`，默认表名 `tbl_weightInfo`。
- 同步账号可执行：
  - `SELECT`：读取待同步记录。
  - `UPDATE`：成功后回写 `isUploadCloud = 1`。
- TCP/IP 已开启，端口可达。
- 待读取条件为：

```sql
WHERE ISNULL([isUploadCloud], 0) = 0
  AND ISNULL([del_flag], 0) = 0
```

建议给同步账号最小权限：

```sql
USE [yunfu];
GRANT SELECT, UPDATE ON dbo.tbl_weightInfo TO [sync_user];
```

如果 SQL Server 就在本机，安装脚本默认使用 `127.0.0.1:1433`。

## 现场安装

以管理员身份打开 PowerShell，进入交付包目录：

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
.\scripts\install-windows.ps1 `
  -ExePath .\sync-daemon.exe `
  -ApiEndpoint "https://api.xmb.xyz/weighing-data-sync/put" `
  -SqlServerHost "127.0.0.1" `
  -SqlServerPort 1433 `
  -SqlServerDatabase "yunfu" `
  -SqlServerUsername "sa"
```

脚本会提示输入 SQL Server 密码。若云端需要 Bearer Token，可传入：

```powershell
$apiKey = Read-Host "API Key" -AsSecureString
.\scripts\install-windows.ps1 `
  -ExePath .\sync-daemon.exe `
  -ApiEndpoint "https://api.xmb.xyz/weighing-data-sync/put" `
  -ApiKey $apiKey `
  -SqlServerHost "127.0.0.1" `
  -SqlServerUsername "sa"
```

安装脚本会完成：

- 创建程序目录、配置目录、数据目录、日志目录。
- 复制 `sync-daemon.exe`。
- 生成 `production.toml`。
- 生成 `.env` 并写入敏感环境变量。
- 检查 SQL Server TCP 端口。
- 创建 Windows 计划任务 `WeighingDataSync`，以 `SYSTEM` 在开机时启动。
- 立即启动计划任务。

## 手动验证

安装后可以手动执行一次同步：

```powershell
cd "C:\Program Files\WeighingDataSync"
.\sync-daemon.exe sync-now --config "C:\ProgramData\WeighingDataSync\config\production.toml"
```

也可以安装时直接带上 `-RunSyncNow`：

```powershell
.\scripts\install-windows.ps1 -ExePath .\sync-daemon.exe -RunSyncNow
```

查看计划任务状态：

```powershell
Get-ScheduledTask -TaskName WeighingDataSync
Get-ScheduledTaskInfo -TaskName WeighingDataSync
```

查看日志：

```powershell
Get-Content "C:\ProgramData\WeighingDataSync\logs\sync-daemon.log" -Tail 100 -Wait
```

成功同步时日志里应出现：

```text
stage = "sqlserver.upload.done"
stage = "sqlserver.mark_uploaded"
```

## 升级

升级只需要替换 exe，并重新启动计划任务：

```powershell
Stop-ScheduledTask -TaskName WeighingDataSync
Copy-Item .\sync-daemon.exe "C:\Program Files\WeighingDataSync\sync-daemon.exe" -Force
Start-ScheduledTask -TaskName WeighingDataSync
```

如果配置项有变化，重新运行安装脚本即可，它会覆盖程序和配置，并重建计划任务。

## 卸载

只移除程序和计划任务，保留数据与日志：

```powershell
.\scripts\uninstall-windows.ps1
```

连同 `C:\ProgramData\WeighingDataSync` 一起删除：

```powershell
.\scripts\uninstall-windows.ps1 -RemoveData
```

## Windows 服务说明

当前仓库保留了 `sync-daemon.exe install-service` / `uninstall-service` 入口，但生产部署推荐使用
本文的计划任务方式。原因是计划任务能直接运行普通 exe，不依赖额外服务包装器；而原生 Windows
Service 方式还需要程序实现完整的 Service Control Manager 生命周期后再作为默认路径。

## 常见问题

### SQL Server 端口不通

安装脚本报：

```text
cannot reach SQL Server at 127.0.0.1:1433
```

检查：

- SQL Server 服务是否启动。
- SQL Server Configuration Manager 是否启用 TCP/IP。
- 固定端口是否为 1433。
- Windows 防火墙是否允许本机或局域网连接。

### 启动报 SQL Server password is not configured

说明 `.env` 未被加载或没有写入 `WDS__SQLSERVER__PASSWORD`。确认计划任务的工作目录是：

```text
C:\Program Files\WeighingDataSync
```

并确认文件存在：

```text
C:\Program Files\WeighingDataSync\.env
```

### 云端 401

云端要求 Bearer Token，但安装时没有传 `-ApiKey`，或 Token 不正确。重新运行安装脚本并传入
`-ApiKey`。

### 需要先看将同步哪些数据

在 SQL Server 上执行：

```sql
USE [yunfu];
SELECT TOP (100) *
FROM dbo.tbl_weightInfo
WHERE ISNULL([isUploadCloud], 0) = 0
  AND ISNULL([del_flag], 0) = 0
ORDER BY ISNULL([updateTime], ISNULL([secondTime], ISNULL([firstTime], [grossTime]))) ASC;
```

这与程序读取条件一致。
