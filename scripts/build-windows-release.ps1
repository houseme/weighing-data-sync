#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$Target = "x86_64-pc-windows-msvc",
    [string]$OutputDir = "dist\weighing-data-sync-windows-x64",
    [switch]$StaticCrt,
    [switch]$SkipArchive
)

$ErrorActionPreference = "Stop"
$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$OutputPath = Join-Path $RepoRoot $OutputDir

function Invoke-Step {
    param(
        [string]$Name,
        [scriptblock]$Body
    )

    Write-Host "==> $Name"
    & $Body
}

Invoke-Step "check Rust toolchain" {
    cargo --version | Out-Host
    rustc --version | Out-Host
}

Invoke-Step "ensure Windows target $Target" {
    rustup target add $Target | Out-Host
}

$previousRustFlags = $env:RUSTFLAGS
if ($StaticCrt) {
    $env:RUSTFLAGS = "-C target-feature=+crt-static"
}

try {
    Invoke-Step "build sync-daemon.exe" {
        cargo build --locked --release --target $Target --features windows | Out-Host
    }
}
finally {
    $env:RUSTFLAGS = $previousRustFlags
}

$ExePath = Join-Path $RepoRoot "target\$Target\release\sync-daemon.exe"
if (!(Test-Path $ExePath)) {
    throw "build output not found: $ExePath"
}

Invoke-Step "prepare deployment package" {
    New-Item -ItemType Directory -Force -Path $OutputPath | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $OutputPath "config") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $OutputPath "scripts") | Out-Null

    Copy-Item -Force $ExePath (Join-Path $OutputPath "sync-daemon.exe")
    Copy-Item -Force (Join-Path $RepoRoot "config\windows-production.example.toml") (Join-Path $OutputPath "config\production.toml")
    Copy-Item -Force (Join-Path $RepoRoot "scripts\install-windows.ps1") (Join-Path $OutputPath "scripts\install-windows.ps1")
    Copy-Item -Force (Join-Path $RepoRoot "scripts\uninstall-windows.ps1") (Join-Path $OutputPath "scripts\uninstall-windows.ps1")
    Copy-Item -Force (Join-Path $RepoRoot "docs\deployment-windows.md") (Join-Path $OutputPath "deployment-windows.md")
}

if (!$SkipArchive) {
    Invoke-Step "create zip archive" {
        $ArchivePath = "$OutputPath.zip"
        if (Test-Path $ArchivePath) {
            Remove-Item -Force $ArchivePath
        }
        Compress-Archive -Force -Path (Join-Path $OutputPath "*") -DestinationPath $ArchivePath
        Write-Host "archive: $ArchivePath"
    }
}

Write-Host "Windows deployment package ready: $OutputPath"
