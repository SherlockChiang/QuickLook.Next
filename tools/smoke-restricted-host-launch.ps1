param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent),
    [string]$Configuration = "Release"
)

$ErrorActionPreference = "Stop"
function Start-AppSmoke([string[]]$Arguments) {
    $startInfo = [Diagnostics.ProcessStartInfo]::new($app)
    $startInfo.UseShellExecute = $false
    foreach ($argument in $Arguments) { $startInfo.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::Start($startInfo)
    $process.WaitForExit()
    return $process
}
$app = Join-Path $Root "src\QuickLook.Next.App\bin\$Configuration\net10.0-windows10.0.19041.0\win-x64\QuickLook.Next.App.exe"
if (-not (Test-Path -LiteralPath $app -PathType Leaf)) {
    throw "Restricted host launch probe requires a built App: $app"
}

Write-Host "== restricted host launch smoke ==" -ForegroundColor Cyan
$process = Start-Process -FilePath $app -ArgumentList "--smoke-restricted-host-launch" -Wait -PassThru
if ($process.ExitCode -ne 0) {
    throw "Restricted host launch smoke failed with exit code $($process.ExitCode)."
}
$parserHost = Join-Path $Root "src\QuickLook.Next.ParserHost\bin\$Configuration\net10.0-windows10.0.19041.0\win-x64\QuickLook.Next.ParserHost.exe"
if (-not (Test-Path -LiteralPath $parserHost -PathType Leaf)) {
    throw "Write-restricted ParserHost smoke requires a built host: $parserHost"
}
$parserProcess = Start-AppSmoke @("--smoke-write-restricted-parser-host", $parserHost)
if ($parserProcess.ExitCode -ne 0) {
    throw "Write-restricted ParserHost smoke failed with exit code $($parserProcess.ExitCode)."
}
$shellBroker = Join-Path $Root "src\QuickLook.Next.ShellBroker\bin\$Configuration\net10.0-windows10.0.19041.0\win-x64\QuickLook.Next.ShellBroker.exe"
$shellFixture = Join-Path $Root "src\QuickLook.Next.App\Assets\QuickLookNext.ico"
if (-not (Test-Path -LiteralPath $shellBroker -PathType Leaf)) {
    throw "ShellBroker smoke requires a built broker: $shellBroker"
}
$shellProcess = Start-AppSmoke @("--smoke-shell-broker", $shellBroker, $shellFixture)
if ($shellProcess.ExitCode -ne 0) {
    throw "ShellBroker smoke failed with exit code $($shellProcess.ExitCode)."
}
Write-Host "restricted host launch smoke passed" -ForegroundColor Green
