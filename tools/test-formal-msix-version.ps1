param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$resolver = Join-Path $Root "tools\resolve-formal-msix-version.ps1"

function Assert-ResolvedVersion {
    param(
        [Parameter(Mandatory = $true)]
        [hashtable]$Parameters,
        [Parameter(Mandatory = $true)]
        [string]$Expected
    )

    $actual = @(& $resolver @Parameters)
    if ($actual.Count -ne 1 -or $actual[0] -ne $Expected) {
        throw "Resolved '$($actual -join ', ')', expected '$Expected'."
    }
}

function Assert-Rejected {
    param([Parameter(Mandatory = $true)][scriptblock]$Action)

    try {
        & $Action | Out-Null
    }
    catch {
        return
    }
    throw "The invalid formal MSIX version was accepted."
}

Assert-ResolvedVersion `
    -Parameters @{ VersionPrefix = "1.2.3" } `
    -Expected "1.2.3.65535"
Assert-ResolvedVersion `
    -Parameters @{ VersionPrefix = "1.2.3"; VersionSuffix = "beta.1" } `
    -Expected "1.2.3.32768"
Assert-ResolvedVersion `
    -Parameters @{
        VersionPrefix = "65535.65535.65535"
        VersionSuffix = "beta.32767"
    } `
    -Expected "65535.65535.65535.65534"

foreach ($parameters in @(
        @{ VersionPrefix = "1.2" },
        @{ VersionPrefix = "1.2.65536" },
        @{ VersionPrefix = "1.2.3"; VersionSuffix = "../victim" },
        @{ VersionPrefix = "1.2.3"; VersionSuffix = "beta.0" },
        @{ VersionPrefix = "1.2.3"; VersionSuffix = "beta.32768" },
        @{ VersionPrefix = "1.2.3"; VersionSuffix = "rc.1" }))
{
    Assert-Rejected { & $resolver @parameters }
}

Write-Host "formal MSIX version resolver test passed" -ForegroundColor Green
