#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$ExePath = ".\sync-daemon.exe",
    [string]$InstallDir = "$env:ProgramFiles\WeighingDataSync",
    [string]$ProgramDataDir = "$env:ProgramData\WeighingDataSync",
    [string]$TaskName = "WeighingDataSync",
    [string]$ApiEndpoint = "https://api.xmb.xyz/weighing-data-sync/put",
    [securestring]$ApiKey,
    [string]$SqlServerHost = "127.0.0.1",
    [int]$SqlServerPort = 1433,
    [string]$SqlServerDatabase = "yunfu",
    [string]$SqlServerUsername = "sa",
    [securestring]$SqlServerPassword,
    [string]$SqlServerTable = "tbl_weightInfo",
    [ValidateSet("off", "none", "plaintext", "not_supported", "required", "true", "on")]
    [string]$SqlServerEncryption = "off",
    [int]$BatchSize = 100,
    [string]$Cron = "0 */5 * * * *",
    [switch]$SkipTask,
    [switch]$SkipNetworkCheck,
    [switch]$RunSyncNow
)

$ErrorActionPreference = "Stop"

function Assert-Administrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    if (!$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw "please run PowerShell as Administrator"
    }
}

function ConvertFrom-SecureStringPlain {
    param([securestring]$Value)

    if ($null -eq $Value) {
        return ""
    }

    $bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($Value)
    try {
        return [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr)
    }
    finally {
        [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr)
    }
}

function ConvertTo-DotEnvValue {
    param([string]$Value)

    if ($null -eq $Value) {
        $Value = ""
    }

    $escaped = $Value.Replace("\", "\\").Replace('"', '\"')
    return '"' + $escaped + '"'
}

function ConvertTo-SqliteUrlPath {
    param([string]$Path)

    $full = [IO.Path]::GetFullPath($Path)
    return "sqlite://$($full.Replace('\', '/'))"
}

function Write-TextFile {
    param(
        [string]$Path,
        [string]$Content
    )

    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [IO.File]::WriteAllText($Path, $Content, $utf8NoBom)
}

function Protect-SecretFile {
    param([string]$Path)

    icacls.exe $Path /inheritance:r /grant:r "*S-1-5-18:(R)" "*S-1-5-32-544:(R)" | Out-Null
}

Assert-Administrator

$ResolvedExePath = (Resolve-Path $ExePath).Path
if ([IO.Path]::GetExtension($ResolvedExePath) -ne ".exe") {
    throw "ExePath must point to sync-daemon.exe"
}

if ($null -eq $SqlServerPassword) {
    $SqlServerPassword = Read-Host "SQL Server password for $SqlServerUsername@$SqlServerHost" -AsSecureString
}

$SqlPasswordPlain = ConvertFrom-SecureStringPlain $SqlServerPassword
if ([string]::IsNullOrWhiteSpace($SqlPasswordPlain)) {
    throw "SQL Server password is required"
}

$ApiKeyPlain = ConvertFrom-SecureStringPlain $ApiKey

if (!$SkipNetworkCheck) {
    Write-Host "checking SQL Server TCP connectivity: ${SqlServerHost}:${SqlServerPort}"
    $reachable = Test-NetConnection -ComputerName $SqlServerHost -Port $SqlServerPort -InformationLevel Quiet
    if (!$reachable) {
        throw "cannot reach SQL Server at ${SqlServerHost}:${SqlServerPort}; check SQL Server service, TCP/IP, port, and firewall"
    }
}

$ConfigDir = Join-Path $ProgramDataDir "config"
$DataDir = Join-Path $ProgramDataDir "data"
$LogDir = Join-Path $ProgramDataDir "logs"
$ConfigPath = Join-Path $ConfigDir "production.toml"
$EnvPath = Join-Path $InstallDir ".env"
$RunScriptPath = Join-Path $InstallDir "run-weighing-data-sync.ps1"
$InstalledExePath = Join-Path $InstallDir "sync-daemon.exe"
$SqliteUrl = ConvertTo-SqliteUrlPath (Join-Path $DataDir "weighing_sync.db")

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null
New-Item -ItemType Directory -Force -Path $DataDir | Out-Null
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null

Copy-Item -Force $ResolvedExePath $InstalledExePath

$ConfigContent = @"
[api]
endpoint = "$ApiEndpoint"
timeout_seconds = 30

[database]
url = "$SqliteUrl"
max_connections = 5

[server]
enabled = false
bind = "0.0.0.0:8080"
route = "/weighing-data-sync/put"
max_body_bytes = 16777216
persist = false

[sqlserver]
host = "$SqlServerHost"
port = $SqlServerPort
database = "$SqlServerDatabase"
username = "$SqlServerUsername"
password = "CHANGE_ME_VIA_ENV"
table = "$SqlServerTable"
trust_cert = true
encryption = "$SqlServerEncryption"
mark_uploaded = true

[sync]
source = "sqlserver"
batch_size = $BatchSize
cron = "$Cron"
sync_on_start = true
retry_initial_delay_ms = 1000
retry_max_delay_ms = 60000
retry_max_elapsed_seconds = 300
"@
Write-TextFile -Path $ConfigPath -Content $ConfigContent

$EnvLines = New-Object System.Collections.Generic.List[string]
[void]$EnvLines.Add("WDS_CONFIG=$(ConvertTo-DotEnvValue $ConfigPath)")
[void]$EnvLines.Add("WDS__SQLSERVER__PASSWORD=$(ConvertTo-DotEnvValue $SqlPasswordPlain)")
if (![string]::IsNullOrWhiteSpace($ApiKeyPlain)) {
    [void]$EnvLines.Add("WDS__API__API_KEY=$(ConvertTo-DotEnvValue $ApiKeyPlain)")
}
Write-TextFile -Path $EnvPath -Content (($EnvLines -join "`r`n") + "`r`n")
Protect-SecretFile -Path $EnvPath

$RunScriptContent = @"
`$ErrorActionPreference = "Stop"
Set-Location "$InstallDir"
`$exe = "$InstalledExePath"
`$config = "$ConfigPath"
`$log = Join-Path "$LogDir" "sync-daemon.log"
& `$exe run --config `$config *>&1 | Tee-Object -FilePath `$log -Append
exit `$LASTEXITCODE
"@
Write-TextFile -Path $RunScriptPath -Content $RunScriptContent

if (!$SkipTask) {
    if (Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue) {
        Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false
    }

    $Action = New-ScheduledTaskAction `
        -Execute "powershell.exe" `
        -Argument "-NoProfile -ExecutionPolicy Bypass -File `"$RunScriptPath`"" `
        -WorkingDirectory $InstallDir
    $Trigger = New-ScheduledTaskTrigger -AtStartup
    $Settings = New-ScheduledTaskSettingsSet `
        -MultipleInstances IgnoreNew `
        -RestartCount 3 `
        -RestartInterval (New-TimeSpan -Minutes 1) `
        -ExecutionTimeLimit (New-TimeSpan -Days 0)

    Register-ScheduledTask `
        -TaskName $TaskName `
        -Action $Action `
        -Trigger $Trigger `
        -Settings $Settings `
        -Description "Weighing data SQL Server to cloud sync daemon" `
        -User "SYSTEM" `
        -RunLevel Highest `
        -Force | Out-Null

    Start-ScheduledTask -TaskName $TaskName
}

if ($RunSyncNow) {
    Push-Location $InstallDir
    try {
        & $InstalledExePath sync-now --config $ConfigPath
        if ($LASTEXITCODE -ne 0) {
            throw "sync-now failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
    }
}

Write-Host "installed sync-daemon.exe to $InstalledExePath"
Write-Host "config: $ConfigPath"
Write-Host "data: $DataDir"
Write-Host "logs: $LogDir"
if (!$SkipTask) {
    Write-Host "scheduled task: $TaskName"
}
