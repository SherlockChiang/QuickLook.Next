param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$resolver = Join-Path $Root "tools\resolve-local-msix-version.ps1"
$defaultPublisher = "CN=QuickLook Next Development"

function Assert-ResolvedVersion {
    param(
        [Parameter(Mandatory = $true)]
        [hashtable]$Parameters,

        [Parameter(Mandatory = $true)]
        [string]$Expected,

        [Parameter(Mandatory = $true)]
        [string]$Scenario
    )

    $output = @(& $resolver @Parameters)
    if ($output.Count -ne 1 -or $output[0] -ne $Expected) {
        throw "$Scenario resolved to '$($output -join ', ')', expected '$Expected'."
    }
}

function Assert-Rejected {
    param(
        [Parameter(Mandatory = $true)]
        [scriptblock]$Action,

        [Parameter(Mandatory = $true)]
        [string]$Scenario
    )

    $rejected = $false
    try {
        & $Action | Out-Null
    }
    catch {
        $rejected = $true
    }
    if (-not $rejected) {
        throw "$Scenario must be rejected."
    }
}

Assert-ResolvedVersion `
    -Parameters @{ VersionPrefix = "1.2.3" } `
    -Expected "1.2.3.1" `
    -Scenario "A package that is not installed"

Assert-ResolvedVersion `
    -Parameters @{
        VersionPrefix = "1.2.3"
        KnownVersions = @("1.2.3.2", "1.2.3.7", "9.9.9.65535")
    } `
    -Expected "1.2.3.8" `
    -Scenario "Known artifacts without an installed package"

Assert-ResolvedVersion `
    -Parameters @{
        VersionPrefix = "2.0.0"
        InstalledVersion = "1.65535.65535.65535"
        InstalledPublisher = $defaultPublisher
    } `
    -Expected "2.0.0.1" `
    -Scenario "An installed package with an older base"

Assert-ResolvedVersion `
    -Parameters @{
        VersionPrefix = "2.0.0"
        InstalledVersion = "1.9.9.4"
        InstalledPublisher = $defaultPublisher
        KnownVersions = @("2.0.0.0", "2.0.0.3", "1.9.9.65535")
    } `
    -Expected "2.0.0.4" `
    -Scenario "Known artifacts for a new source base"

Assert-ResolvedVersion `
    -Parameters @{
        VersionPrefix = "3.4.5"
        InstalledVersion = "3.4.5.12"
        InstalledPublisher = $defaultPublisher
    } `
    -Expected "3.4.5.13" `
    -Scenario "An installed package with the same base"

Assert-ResolvedVersion `
    -Parameters @{
        VersionPrefix = "3.4.5"
        InstalledVersion = "3.4.5.12"
        InstalledPublisher = $defaultPublisher
        KnownVersions = @("3.4.5.4", "3.4.5.20", "3.4.6.65535")
    } `
    -Expected "3.4.5.21" `
    -Scenario "A known artifact newer than the installed revision"

Assert-ResolvedVersion `
    -Parameters @{
        VersionPrefix = "65535.65535.65535"
    } `
    -Expected "65535.65535.65535.1" `
    -Scenario "Maximum source version components"

Assert-ResolvedVersion `
    -Parameters @{
        VersionPrefix = "4.0.0"
        InstalledVersion = "4.0.0.1"
        InstalledPublisher = "CN=Custom Development"
        ExpectedPublisher = "CN=Custom Development"
    } `
    -Expected "4.0.0.2" `
    -Scenario "A matching custom publisher"

Assert-Rejected `
    -Action {
        & $resolver `
            -VersionPrefix "1.2.3" `
            -InstalledVersion "2.0.0.0" `
            -InstalledPublisher $defaultPublisher
    } `
    -Scenario "An installed package with a newer base"

Assert-Rejected `
    -Action {
        & $resolver `
            -VersionPrefix "1.2.3" `
            -InstalledVersion "1.2.3.0" `
            -InstalledPublisher "CN=Unexpected Publisher"
    } `
    -Scenario "A different installed publisher"

Assert-Rejected `
    -Action {
        & $resolver `
            -VersionPrefix "1.2.3" `
            -InstalledVersion "1.2.3.0"
    } `
    -Scenario "An installed version without its publisher"

Assert-Rejected `
    -Action {
        & $resolver `
            -VersionPrefix "1.2.3" `
            -InstalledPublisher $defaultPublisher
    } `
    -Scenario "An installed publisher without its version"

Assert-Rejected `
    -Action {
        & $resolver `
            -VersionPrefix "1.2.3" `
            -InstalledVersion "1.2.3.32767" `
            -InstalledPublisher $defaultPublisher
    } `
    -Scenario "An exhausted installed revision"

Assert-Rejected `
    -Action {
        & $resolver `
            -VersionPrefix "1.2.3" `
            -KnownVersions @("1.2.3.32767")
    } `
    -Scenario "An exhausted known revision"

Assert-Rejected `
    -Action {
        & $resolver `
            -VersionPrefix "1.2.3" `
            -InstalledVersion "1.2.3.32768" `
            -InstalledPublisher $defaultPublisher
    } `
    -Scenario "An installed beta revision"

Assert-Rejected `
    -Action {
        & $resolver `
            -VersionPrefix "1.2.3" `
            -KnownVersions @("1.2.3.65535")
    } `
    -Scenario "A known stable revision"

foreach ($invalidPrefix in @(
        "",
        "1.2",
        "1.2.3.4",
        "1.2.-3",
        "1.2.65536",
        " 1.2.3",
        "1.2.3 "))
{
    Assert-Rejected `
        -Action { & $resolver -VersionPrefix $invalidPrefix } `
        -Scenario "Invalid VersionPrefix '$invalidPrefix'"
}

foreach ($invalidInstalledVersion in @(
        "1.2.3",
        "1.2.3.4.5",
        "1.2.3.-1",
        "1.2.3.65536",
        "1.2.x.0"))
{
    Assert-Rejected `
        -Action {
            & $resolver `
                -VersionPrefix "1.2.3" `
                -InstalledVersion $invalidInstalledVersion `
                -InstalledPublisher $defaultPublisher
        } `
        -Scenario "Invalid InstalledVersion '$invalidInstalledVersion'"
}

foreach ($invalidKnownVersion in @(
        "1.2.3",
        "1.2.3.4.5",
        "1.2.3.65536",
        "other"))
{
    Assert-Rejected `
        -Action {
            & $resolver `
                -VersionPrefix "1.2.3" `
                -KnownVersions @($invalidKnownVersion)
        } `
        -Scenario "Invalid KnownVersions entry '$invalidKnownVersion'"
}

Assert-Rejected `
    -Action {
        & $resolver `
            -VersionPrefix "1.2.3" `
            -ExpectedPublisher ""
    } `
    -Scenario "An empty expected publisher"

Write-Host "local MSIX version resolver test passed" -ForegroundColor Green
