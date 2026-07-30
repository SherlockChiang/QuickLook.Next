param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$entryPath = Join-Path $Root "build.ps1"
$workflowPath = Join-Path $Root "tools\build-local.ps1"

foreach ($path in @($entryPath, $workflowPath)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Local build entry point is missing: $path"
    }
}

$entry = Get-Command -Name $entryPath
$workflow = Get-Command -Name $workflowPath
foreach ($name in @(
    "Version",
    "Bump",
    "Configuration",
    "VersionSuffix",
    "NoRestore",
    "Test",
    "Install"))
{
    if (-not $entry.Parameters.ContainsKey($name) -or
        -not $workflow.Parameters.ContainsKey($name))
    {
        throw "Local build entry points are missing the $name parameter."
    }
}

$entryText = Get-Content -LiteralPath $entryPath -Raw
$workflowText = Get-Content -LiteralPath $workflowPath -Raw
foreach ($rule in @(
    @('tools\\build-local\.ps1"\)\s+@PSBoundParameters', "The root build entry must forward parameters to the focused workflow."),
    @('set-version\.ps1[\s\S]*test-release-version\.ps1', "Local builds must synchronize and verify the version before compiling."),
    @('cargo\s+build\s+--workspace\s+--release\s+--locked', "Local builds must produce the native Release DLL used by every .NET configuration."),
    @('VersionPrefix=\$resolvedVersion[\s\S]*dotnet\s+build\s+\$solution[\s\S]*--no-restore', "Local builds must compile the solution with the synchronized version."),
    @('dotnet\s+restore[\s\S]*--disable-build-servers[\s\S]*dotnet\s+build[\s\S]*--disable-build-servers', "Local builds must bypass stale persistent build servers."),
    @('if\s*\(\$Test\)[\s\S]*cargo\s+test[\s\S]*if\s*\(\$Test\)[\s\S]*dotnet\s+test', "The Test switch must cover both Rust and .NET."),
    @('dotnet\s+test[\s\S]{0,220}--maxcpucount:1', "Host-launching integration test projects must run serially to preserve their hard timeout signal."),
    @('\$Install\s+-and\s+-not\s+\$Test[\s\S]*\$Test\s*=\s*\$true', "Installing must automatically enable the full test path."),
    @('if\s*\(\$Install\)[\s\S]*write-tested-release-proof\.ps1[\s\S]*update-local-msix\.ps1', "Installing must package only the build that just passed all tests."),
    @('installed MSIX was not changed; pass -Install', "Normal local builds must explicitly state that installation is unchanged.")
)) {
    if (($entryText + "`n" + $workflowText) -notmatch $rule[0]) {
        throw $rule[1]
    }
}

Write-Host "local build workflow test passed" -ForegroundColor Green
