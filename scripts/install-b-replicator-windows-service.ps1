#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$ExePath = ".\b-replicator.exe",
    [string]$InstallDir = "$env:ProgramFiles\WeighingBReplicator",
    [string]$ProgramDataDir = "$env:ProgramData\WeighingBReplicator",
    [string]$TaskName = "WeighingBReplicator",
    [securestring]$MysqlDsn,
    [string]$CBaseUrl = "http://c-server",
    [securestring]$QueryApiToken,
    [securestring]$QuerySignSecret,
    [securestring]$CleanupApiToken,
    [securestring]$CleanupSignSecret,
    [int]$FetchBatchSize = 100,
    [int]$FetchIntervalSeconds = 5,
    [int]$DeleteIntervalSeconds = 2,
    [int]$HttpTimeoutSeconds = 30,
    [int]$DbTimeoutSeconds = 30,
    [switch]$DisableAutoMigrate,
    [switch]$SkipStart
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

function Write-TextFile {
    param([string]$Path, [string]$Content)
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
    throw "ExePath must point to b-replicator.exe"
}

if ($null -eq $MysqlDsn) {
    $MysqlDsn = Read-Host "MySQL DSN, for example user:password@tcp(127.0.0.1:3306)/weighing?charset=utf8mb4&parseTime=true&loc=Local" -AsSecureString
}
if ($null -eq $QueryApiToken) {
    $QueryApiToken = Read-Host "C query API token" -AsSecureString
}
if ($null -eq $QuerySignSecret) {
    $QuerySignSecret = Read-Host "C query HMAC secret" -AsSecureString
}
if ($null -eq $CleanupApiToken) {
    $CleanupApiToken = Read-Host "C cleanup API token" -AsSecureString
}
if ($null -eq $CleanupSignSecret) {
    $CleanupSignSecret = Read-Host "C cleanup HMAC secret" -AsSecureString
}

$MysqlDsnPlain = ConvertFrom-SecureStringPlain $MysqlDsn
$QueryApiTokenPlain = ConvertFrom-SecureStringPlain $QueryApiToken
$QuerySignSecretPlain = ConvertFrom-SecureStringPlain $QuerySignSecret
$CleanupApiTokenPlain = ConvertFrom-SecureStringPlain $CleanupApiToken
$CleanupSignSecretPlain = ConvertFrom-SecureStringPlain $CleanupSignSecret

if ([string]::IsNullOrWhiteSpace($MysqlDsnPlain)) {
    throw "MYSQL_DSN is required"
}
if ([string]::IsNullOrWhiteSpace($QueryApiTokenPlain) -or [string]::IsNullOrWhiteSpace($QuerySignSecretPlain)) {
    throw "query token and sign secret are required"
}
if ([string]::IsNullOrWhiteSpace($CleanupApiTokenPlain) -or [string]::IsNullOrWhiteSpace($CleanupSignSecretPlain)) {
    throw "cleanup token and sign secret are required"
}

$LogDir = Join-Path $ProgramDataDir "logs"
$EnvPath = Join-Path $InstallDir ".env"
$RunScriptPath = Join-Path $InstallDir "run-b-replicator.ps1"
$InstalledExePath = Join-Path $InstallDir "b-replicator.exe"

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
Copy-Item -Force $ResolvedExePath $InstalledExePath

$EnvLines = New-Object System.Collections.Generic.List[string]
[void]$EnvLines.Add("MYSQL_DSN=$(ConvertTo-DotEnvValue $MysqlDsnPlain)")
[void]$EnvLines.Add("C_BASE_URL=$(ConvertTo-DotEnvValue $CBaseUrl)")
[void]$EnvLines.Add("QUERY_API_TOKEN=$(ConvertTo-DotEnvValue $QueryApiTokenPlain)")
[void]$EnvLines.Add("QUERY_SIGN_SECRET=$(ConvertTo-DotEnvValue $QuerySignSecretPlain)")
[void]$EnvLines.Add("CLEANUP_API_TOKEN=$(ConvertTo-DotEnvValue $CleanupApiTokenPlain)")
[void]$EnvLines.Add("CLEANUP_SIGN_SECRET=$(ConvertTo-DotEnvValue $CleanupSignSecretPlain)")
[void]$EnvLines.Add("FETCH_BATCH_SIZE=$(ConvertTo-DotEnvValue $FetchBatchSize)")
[void]$EnvLines.Add("FETCH_INTERVAL_SECONDS=$(ConvertTo-DotEnvValue $FetchIntervalSeconds)")
[void]$EnvLines.Add("DELETE_INTERVAL_SECONDS=$(ConvertTo-DotEnvValue $DeleteIntervalSeconds)")
[void]$EnvLines.Add("HTTP_TIMEOUT_SECONDS=$(ConvertTo-DotEnvValue $HttpTimeoutSeconds)")
[void]$EnvLines.Add("DB_TIMEOUT_SECONDS=$(ConvertTo-DotEnvValue $DbTimeoutSeconds)")
[void]$EnvLines.Add("AUTO_MIGRATE=$(ConvertTo-DotEnvValue (!$DisableAutoMigrate))")
Write-TextFile -Path $EnvPath -Content (($EnvLines -join "`r`n") + "`r`n")
Protect-SecretFile -Path $EnvPath

$RunScriptContent = @"
`$ErrorActionPreference = "Stop"
Set-Location "$InstallDir"
`$envFile = "$EnvPath"
Get-Content `$envFile | ForEach-Object {
    if (!(`$_ -match '^\s*$' -or `$_.TrimStart().StartsWith('#'))) {
        `$name, `$value = `$_.Split('=', 2)
        if (![string]::IsNullOrWhiteSpace(`$name)) {
            `$value = `$value.Trim()
            if (`$value.StartsWith('"') -and `$value.EndsWith('"')) {
                `$value = `$value.Substring(1, `$value.Length - 2).Replace('\"', '"').Replace('\\', '\')
            }
            [Environment]::SetEnvironmentVariable(`$name.Trim(), `$value, 'Process')
        }
    }
}
`$log = Join-Path "$LogDir" "b-replicator.log"
& "$InstalledExePath" *>&1 | Tee-Object -FilePath `$log -Append
exit `$LASTEXITCODE
"@
Write-TextFile -Path $RunScriptPath -Content $RunScriptContent

if (Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue) {
    Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
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
    -Description "B machine C receiver to local MySQL replicator" `
    -User "SYSTEM" `
    -RunLevel Highest `
    -Force | Out-Null

if (!$SkipStart) {
    Start-ScheduledTask -TaskName $TaskName
}

Write-Host "installed b-replicator to $InstalledExePath"
Write-Host "env: $EnvPath"
Write-Host "logs: $LogDir"
Write-Host "scheduled task: $TaskName"
