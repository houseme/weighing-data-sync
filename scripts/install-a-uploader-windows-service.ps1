#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$ExePath = ".\a-uploader.exe",
    [string]$InstallDir = "$env:ProgramFiles\WeighingAUploader",
    [string]$ProgramDataDir = "$env:ProgramData\WeighingAUploader",
    [string]$TaskName = "WeighingAUploader",
    [string]$CEndpoint = "http://c-server/weighing-data-sync/put",
    [securestring]$IngestApiToken,
    [securestring]$IngestSignSecret,
    [string]$SqlServerHost = "127.0.0.1",
    [int]$SqlServerPort = 1433,
    [string]$SqlServerDatabase = "shweight",
    [string]$SqlServerUsername = "sa",
    [securestring]$SqlServerPassword,
    [string]$SqlServerSchema = "dbo",
    [string]$SqlServerInfoTable = "tbl_weightInfo",
    [string]$SqlServerPhotoTable = "tbl_weightPhoto",
    [string]$SqlServerInfoPendingWhere = "ISNULL([isUploadCloud], 0) = 0 AND ISNULL([del_flag], 0) = 0",
    [string]$SqlServerPhotoPendingWhere = "ISNULL([isUploadCloud], 0) = 0 AND ISNULL([delFlag], 0) = 0",
    [int]$BatchSize = 100,
    [int]$PollIntervalSeconds = 30,
    [int]$HttpTimeoutSeconds = 30,
    [switch]$SkipNetworkCheck,
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
    throw "ExePath must point to a-uploader.exe"
}

if ($null -eq $IngestApiToken) {
    $IngestApiToken = Read-Host "C ingest API token" -AsSecureString
}
if ($null -eq $IngestSignSecret) {
    $IngestSignSecret = Read-Host "C ingest HMAC secret" -AsSecureString
}
if ($null -eq $SqlServerPassword) {
    $SqlServerPassword = Read-Host "SQL Server password for $SqlServerUsername@$SqlServerHost" -AsSecureString
}

$IngestApiTokenPlain = ConvertFrom-SecureStringPlain $IngestApiToken
$IngestSignSecretPlain = ConvertFrom-SecureStringPlain $IngestSignSecret
$SqlServerPasswordPlain = ConvertFrom-SecureStringPlain $SqlServerPassword
if ([string]::IsNullOrWhiteSpace($IngestApiTokenPlain) -or [string]::IsNullOrWhiteSpace($IngestSignSecretPlain)) {
    throw "ingest token and sign secret are required"
}
if ([string]::IsNullOrWhiteSpace($SqlServerPasswordPlain)) {
    throw "SQL Server password is required"
}

if (!$SkipNetworkCheck) {
    Write-Host "checking SQL Server TCP connectivity: ${SqlServerHost}:${SqlServerPort}"
    $reachable = Test-NetConnection -ComputerName $SqlServerHost -Port $SqlServerPort -InformationLevel Quiet
    if (!$reachable) {
        throw "cannot reach SQL Server at ${SqlServerHost}:${SqlServerPort}; check SQL Server service, TCP/IP, port, and firewall"
    }
}

$DataDir = Join-Path $ProgramDataDir "data"
$LogDir = Join-Path $ProgramDataDir "logs"
$EnvPath = Join-Path $InstallDir ".env"
$RunScriptPath = Join-Path $InstallDir "run-a-uploader.ps1"
$InstalledExePath = Join-Path $InstallDir "a-uploader.exe"
$StateFile = Join-Path $DataDir "a-uploader-state.jsonl"

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
New-Item -ItemType Directory -Force -Path $DataDir | Out-Null
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
Copy-Item -Force $ResolvedExePath $InstalledExePath

$EnvLines = New-Object System.Collections.Generic.List[string]
[void]$EnvLines.Add("C_ENDPOINT=$(ConvertTo-DotEnvValue $CEndpoint)")
[void]$EnvLines.Add("INGEST_API_TOKEN=$(ConvertTo-DotEnvValue $IngestApiTokenPlain)")
[void]$EnvLines.Add("INGEST_SIGN_SECRET=$(ConvertTo-DotEnvValue $IngestSignSecretPlain)")
[void]$EnvLines.Add("SQLSERVER_HOST=$(ConvertTo-DotEnvValue $SqlServerHost)")
[void]$EnvLines.Add("SQLSERVER_PORT=$(ConvertTo-DotEnvValue $SqlServerPort)")
[void]$EnvLines.Add("SQLSERVER_DATABASE=$(ConvertTo-DotEnvValue $SqlServerDatabase)")
[void]$EnvLines.Add("SQLSERVER_USERNAME=$(ConvertTo-DotEnvValue $SqlServerUsername)")
[void]$EnvLines.Add("SQLSERVER_PASSWORD=$(ConvertTo-DotEnvValue $SqlServerPasswordPlain)")
[void]$EnvLines.Add("SQLSERVER_SCHEMA=$(ConvertTo-DotEnvValue $SqlServerSchema)")
[void]$EnvLines.Add("SQLSERVER_INFO_TABLE=$(ConvertTo-DotEnvValue $SqlServerInfoTable)")
[void]$EnvLines.Add("SQLSERVER_PHOTO_TABLE=$(ConvertTo-DotEnvValue $SqlServerPhotoTable)")
[void]$EnvLines.Add("SQLSERVER_INFO_PRIMARY_KEY=""serialNo""")
[void]$EnvLines.Add("SQLSERVER_PHOTO_PRIMARY_KEY=""id""")
[void]$EnvLines.Add("SQLSERVER_INFO_SERIAL_COLUMN=""serialNo""")
[void]$EnvLines.Add("SQLSERVER_PHOTO_SERIAL_COLUMN=""serialNo""")
[void]$EnvLines.Add("SQLSERVER_INFO_PENDING_WHERE=$(ConvertTo-DotEnvValue $SqlServerInfoPendingWhere)")
[void]$EnvLines.Add("SQLSERVER_PHOTO_PENDING_WHERE=$(ConvertTo-DotEnvValue $SqlServerPhotoPendingWhere)")
[void]$EnvLines.Add("STATE_FILE=$(ConvertTo-DotEnvValue $StateFile)")
[void]$EnvLines.Add("BATCH_SIZE=$(ConvertTo-DotEnvValue $BatchSize)")
[void]$EnvLines.Add("POLL_INTERVAL_SECONDS=$(ConvertTo-DotEnvValue $PollIntervalSeconds)")
[void]$EnvLines.Add("HTTP_TIMEOUT_SECONDS=$(ConvertTo-DotEnvValue $HttpTimeoutSeconds)")
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
`$log = Join-Path "$LogDir" "a-uploader.log"
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
    -Description "A machine SQL Server weighing data uploader" `
    -User "SYSTEM" `
    -RunLevel Highest `
    -Force | Out-Null

if (!$SkipStart) {
    Start-ScheduledTask -TaskName $TaskName
}

Write-Host "installed a-uploader to $InstalledExePath"
Write-Host "env: $EnvPath"
Write-Host "state: $StateFile"
Write-Host "logs: $LogDir"
Write-Host "scheduled task: $TaskName"
