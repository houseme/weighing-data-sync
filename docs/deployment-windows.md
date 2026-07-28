# Windows 部署指南

## 推荐目录

```text
C:\Program Files\WeighingDataSync\sync-daemon.exe
C:\ProgramData\WeighingDataSync\config\default.toml
C:\ProgramData\WeighingDataSync\data\weighing_sync.db
C:\ProgramData\WeighingDataSync\logs\
```

## 编译 Windows 版本

在 Windows 本机：

```powershell
cargo build --release --features windows
```

在 macOS / Linux 交叉准备目标：

```bash
rustup target add x86_64-pc-windows-msvc
cargo build --release --target x86_64-pc-windows-msvc --features windows
```

> 说明：`windows` 特性启用 `windows-service` 与 `winreg`，它们位于
> `target.'cfg(windows)'.dependencies`，在非 Windows 上不会被编译。Windows 专用代码
> （`src/windows/*`）整体被 `#[cfg(all(target_os = "windows", feature = "windows"))]` 包裹，
> 非 Windows 平台返回明确错误。

## 凭据管理（生产环境）

现场凭据**不写入磁盘配置**，推荐任选其一：

- **Windows 凭据管理器**：服务启动脚本从中读取后再设置 `WDS__*` 环境变量。
- **服务账号环境变量**：专用低权限服务账号的环境变量。
- **组策略 / 配置管理**统一下发环境变量。

最小化集（其余可写进 `default.toml`）：

```powershell
$env:WDS__SQLSERVER__HOST     = "现场主机"
$env:WDS__SQLSERVER__PASSWORD = "现场密码"
$env:WDS__API__API_KEY        = "云端 Token"
```

## 前台运行

```powershell
.\target\release\sync-daemon.exe run --config C:\ProgramData\WeighingDataSync\config\default.toml
```

## 手动同步一次

```powershell
.\target\release\sync-daemon.exe sync-now --config C:\ProgramData\WeighingDataSync\config\default.toml
```

## 用户登录自启动（注册表 Run 项）

```powershell
.\target\release\sync-daemon.exe enable-autostart      # 写入
.\target\release\sync-daemon.exe autostart-status      # 查询
.\target\release\sync-daemon.exe disable-autostart     # 移除
```

注册表位置：

```text
HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run
  WeighingDataSync = "<exe-path>" run
```

## Windows 服务安装 / 卸载

以管理员身份运行 PowerShell：

```powershell
.\target\release\sync-daemon.exe install-service
.\target\release\sync-daemon.exe uninstall-service
```

服务以 `ServiceStartType::AutoStart`、`OWN_PROCESS` 注册，启动参数为 `run`。

## 生产环境加固建议

- 服务账号使用专用低权限账号，最小权限原则。
- 数据目录放在 `C:\ProgramData\WeighingDataSync\data`，定期备份。
- 云端 API Key 与 SQL Server 密码通过环境变量或凭据管理器注入，不进源码与磁盘配置。
- 现场部署前用真实云端沙箱环境跑一次 `sync-now`。
- Windows 服务控制分发与事件日志上报可作为下一步生产化加固项单独验证。

## 下一步

- 上报报文格式：[HTTP 协议](api-protocol.md)。
- 日志字段与排障：[日志与排障](logging.md)。
