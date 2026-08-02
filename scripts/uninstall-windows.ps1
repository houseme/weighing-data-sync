#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$TaskName = "WeighingDataSync",
    [string]$InstallDir = "$env:ProgramFiles\WeighingDataSync",
    [string]$ProgramDataDir = "$env:ProgramData\WeighingDataSync",
    [switch]$RemoveData
)

$ErrorActionPreference = "Stop"

function Assert-Administrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    if (!$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw "please run PowerShell as Administrator"
    }
}

Assert-Administrator

$task = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
if ($null -ne $task) {
    Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false
    Write-Host "removed scheduled task: $TaskName"
}

if (Test-Path $InstallDir) {
    Remove-Item -Recurse -Force $InstallDir
    Write-Host "removed install directory: $InstallDir"
}

if ($RemoveData -and (Test-Path $ProgramDataDir)) {
    Remove-Item -Recurse -Force $ProgramDataDir
    Write-Host "removed program data directory: $ProgramDataDir"
}
elseif (Test-Path $ProgramDataDir) {
    Write-Host "kept program data directory: $ProgramDataDir"
}
