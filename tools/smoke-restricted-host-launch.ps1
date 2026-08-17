param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent),
    [string]$Configuration = "Release"
)

$ErrorActionPreference = "Stop"
function ConvertTo-WindowsCommandLineArgument([string]$Argument) {
    if ($Argument.Length -gt 0 -and $Argument -notmatch '[\s"]') {
        return $Argument
    }

    $quoted = [Text.StringBuilder]::new()
    [void]$quoted.Append('"')
    $backslashCount = 0
    foreach ($character in $Argument.ToCharArray()) {
        if ($character -eq [char]0x5C) {
            $backslashCount++
            continue
        }

        if ($character -eq '"') {
            [void]$quoted.Append([char]0x5C, (($backslashCount * 2) + 1))
            [void]$quoted.Append('"')
        }
        else {
            [void]$quoted.Append([char]0x5C, $backslashCount)
            [void]$quoted.Append($character)
        }
        $backslashCount = 0
    }
    [void]$quoted.Append([char]0x5C, ($backslashCount * 2))
    [void]$quoted.Append('"')
    return $quoted.ToString()
}

function Start-AppSmoke([string[]]$Arguments) {
    $startInfo = [Diagnostics.ProcessStartInfo]::new($app)
    $startInfo.UseShellExecute = $false
    if ($null -ne $startInfo.PSObject.Properties['ArgumentList']) {
        foreach ($argument in $Arguments) { $startInfo.ArgumentList.Add($argument) }
    }
    else {
        # Windows PowerShell 5.1 targets .NET Framework, which does not expose
        # ProcessStartInfo.ArgumentList. Preserve exact argv boundaries there,
        # including host paths below a workspace whose name contains spaces.
        $startInfo.Arguments = ($Arguments | ForEach-Object {
            ConvertTo-WindowsCommandLineArgument $_
        }) -join ' '
    }
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
