param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent),
    [string]$Configuration = "Release"
)

$ErrorActionPreference = "Stop"
$app = Join-Path $Root "src\QuickLook.Next.App\bin\$Configuration\net10.0-windows10.0.19041.0\win-x64\QuickLook.Next.App.exe"
if (-not (Test-Path -LiteralPath $app -PathType Leaf)) {
    throw "Restricted host launch probe requires a built App: $app"
}

Write-Host "== restricted host launch smoke ==" -ForegroundColor Cyan
$process = Start-Process -FilePath $app -ArgumentList "--smoke-restricted-host-launch" -Wait -PassThru
if ($process.ExitCode -ne 0) {
    throw "Restricted host launch smoke failed with exit code $($process.ExitCode)."
}
Write-Host "restricted host launch smoke passed" -ForegroundColor Green
