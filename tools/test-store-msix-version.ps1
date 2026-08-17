param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$resolver = Join-Path $Root "tools\resolve-store-msix-version.ps1"

function Assert-ResolvedVersion {
    param(
        [Parameter(Mandatory = $true)]
        [string]$InputVersion,
        [Parameter(Mandatory = $true)]
        [string]$Expected
    )

    $actual = @(& $resolver -VersionPrefix $InputVersion)
    if ($actual.Count -ne 1 -or $actual[0] -ne $Expected) {
        throw "Resolved '$($actual -join ', ')', expected '$Expected'."
    }
}

function Assert-Rejected {
    param(
        [Parameter(Mandatory = $true)]
        [string]$InputVersion
    )

    try {
        & $resolver -VersionPrefix $InputVersion | Out-Null
    }
    catch {
        return
    }
    throw "The invalid Store MSIX version was accepted: $InputVersion"
}

Assert-ResolvedVersion -InputVersion "1.0.0" -Expected "1.0.0.0"
Assert-ResolvedVersion -InputVersion "12.34.56" -Expected "12.34.56.0"
Assert-ResolvedVersion -InputVersion "65535.65535.65535" -Expected "65535.65535.65535.0"

foreach ($inputVersion in @(
        "0.3.7",
        "1.2",
        "1.2.65536",
        "65536.1.1",
        "1.2.3.4",
        "1.2.3-beta.1"))
{
    Assert-Rejected -InputVersion $inputVersion
}

Write-Host "Store MSIX version resolver test passed" -ForegroundColor Green
