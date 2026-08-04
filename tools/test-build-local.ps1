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
    "Package",
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
    @('Invoke-CheckedScript[\s\S]{0,180}tools\\build-local\.ps1[\s\S]{0,120}Arguments\s+\$PSBoundParameters', "The root build entry must fail closed while forwarding parameters to the focused workflow."),
    @('set-version\.ps1[\s\S]*test-release-version\.ps1', "Local builds must synchronize and verify the version before compiling."),
    @('dotnet\s+msbuild\s+\$nativeProject[\s\S]{0,120}-target:Build', "Local builds must produce the pinned native Release DLL through its shared MSBuild project."),
    @('VersionPrefix=\$resolvedVersion[\s\S]*dotnet\s+build\s+\$solution[\s\S]*--no-restore', "Local builds must compile the solution with the synchronized version."),
    @('dotnet\s+restore[\s\S]*--disable-build-servers[\s\S]*dotnet\s+build[\s\S]*--disable-build-servers', "Local builds must bypass stale persistent build servers."),
    @('if\s*\(\$Test\)[\s\S]*cargo\s+test[\s\S]*if\s*\(\$Test\)[\s\S]*dotnet\s+test', "The Test switch must cover both Rust and .NET."),
    @('dotnet\s+test[\s\S]{0,220}--maxcpucount:1', "Host-launching integration test projects must run serially to preserve their hard timeout signal."),
    @('\$packageRequested\s*=\s*\$Package\s+-or\s+\$Install', "Install must imply the shared local packaging path."),
    @('\$packageRequested\s+-and\s+-not\s+\$Test[\s\S]*\$Test\s*=\s*\$true', "Packaging and installing must automatically enable the full test path."),
    @('if\s*\(\$packageRequested\)[\s\S]*write-tested-release-proof\.ps1[\s\S]*update-local-msix\.ps1[\s\S]*PackageOnly\s*=\s*-not\s+\[bool\]\$Install', "Packaging must reuse tested outputs without requiring installation."),
    @('checked-invocation\.ps1[\s\S]*Invoke-CheckedScript[\s\S]*update-local-msix\.ps1', "Local build orchestration must fail closed on child scripts."),
    @('elseif\s*\(\$Package\)[\s\S]*MSIX version:[\s\S]*Installer:', "Package-only builds must print both artifact paths and the four-part MSIX version."),
    @('No MSIX was created; pass -Package to package or -Install', "Normal local builds must state that packaging and installation are opt-in.")
)) {
    if (($entryText + "`n" + $workflowText) -notmatch $rule[0]) {
        throw $rule[1]
    }
}

Write-Host "local build workflow test passed" -ForegroundColor Green
