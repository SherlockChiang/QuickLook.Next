param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent),
    [ValidateSet("quick", "full")]
    [string]$Mode = "quick",
    [string]$Focus = "",
    [switch]$SkipBuild,
    [switch]$SkipGuard,
    [switch]$AllowDirty
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "checked-invocation.ps1")

function Invoke-Step([string]$Name, [scriptblock]$Script) {
    Write-Host "== $Name ==" -ForegroundColor Cyan
    $started = Get-Date
    Invoke-CheckedScriptBlock -Script $Script -FailureMessage "$Name failed"
    $elapsed = (Get-Date) - $started
    Write-Host ("passed in {0:n1}s" -f $elapsed.TotalSeconds) -ForegroundColor Green
}

function Test-CommandExists([string]$Name) {
    return $null -ne (Get-Command $Name -ErrorAction SilentlyContinue)
}

function Invoke-GitStatus {
    if (-not (Test-CommandExists "git")) {
        throw "git was not found on PATH"
    }
    git -C $Root status --short --branch
}

Write-Host "== long-cycle harness ==" -ForegroundColor Cyan
Write-Host "root: $Root"
Write-Host "mode: $Mode"
if ($Focus) { Write-Host "focus: $Focus" }

Invoke-Step "preflight" {
    if ($PSVersionTable.PSVersion.Major -lt 7) {
        throw "PowerShell 7 or newer is required. Run this harness with pwsh."
    }
    if (-not (Test-Path -LiteralPath $Root)) {
        throw "Root not found: $Root"
    }
    if (-not (Test-Path -LiteralPath (Join-Path $Root "QuickLook.Next.slnx"))) {
        throw "Solution not found under root: $Root"
    }
    if (-not (Test-Path -LiteralPath (Join-Path $Root "native\quicklook_next_native\Cargo.toml"))) {
        throw "Native Cargo.toml not found under root: $Root"
    }
    $status = @(git -C $Root status --short)
    if ($status.Count -gt 0 -and -not $AllowDirty) {
        Write-Host "Worktree has uncommitted changes:" -ForegroundColor Yellow
        $status | ForEach-Object { Write-Host $_ }
        throw "Commit or intentionally pass -AllowDirty before running the harness"
    }
}

Invoke-Step "native tests" {
    cargo test --manifest-path (Join-Path $Root "native\quicklook_next_native\Cargo.toml")
}

if (-not $SkipBuild) {
    Invoke-Step "dotnet build" {
        dotnet build (Join-Path $Root "QuickLook.Next.slnx") -c Debug --no-restore
    }
}

if (-not $SkipGuard) {
    Invoke-Step "architecture guard" {
        Invoke-CheckedScript -Path (Join-Path $Root "tools\guard-architecture.ps1") `
            -Arguments @{ Root = $Root; SkipDist = $true } `
            -FailureMessage "Architecture guard failed"
    }
}

if ($Mode -eq "full") {
    Invoke-Step "native release build" {
        dotnet msbuild (Join-Path $Root "native\QuickLook.Next.Native.proj") `
            -target:Build -verbosity:minimal
    }
    Invoke-Step "native smoke" {
        Invoke-CheckedScript -Path (Join-Path $Root "tools\smoke-native.ps1") `
            -Arguments @{ Root = $Root } `
            -FailureMessage "Native smoke failed"
    }
}

Invoke-Step "final status" {
    Invoke-GitStatus
}

Write-Host "Long-cycle harness completed." -ForegroundColor Green
