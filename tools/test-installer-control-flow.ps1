param(
    [string]$Path = (Join-Path (
        Split-Path $PSScriptRoot -Parent) "packaging\Install.ps1"),
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$source = [IO.File]::ReadAllText((Resolve-Path -LiteralPath $Path))
$tokens = $null
$parseErrors = $null
$installerAst = [Management.Automation.Language.Parser]::ParseInput(
    $source,
    [ref]$tokens,
    [ref]$parseErrors)
if ($parseErrors.Count -ne 0) {
    throw "Installer control-flow harness could not parse Install.ps1."
}

function Get-InstallerFunction([string]$Name) {
    $matches = @($installerAst.FindAll({
        param($node)
        $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
            $node.Name -eq $Name
    }, $true))
    if ($matches.Count -ne 1) {
        throw "Expected one installer function named $Name."
    }
    return $matches[0]
}

$replacementItems = [Collections.Generic.List[object]]::new()
function Add-Replacement($Ast, [string]$Text) {
    $replacementItems.Add([pscustomobject]@{
        Start = $Ast.Extent.StartOffset
        Length = $Ast.Extent.EndOffset - $Ast.Extent.StartOffset
        Text = $Text
    })
}

Add-Replacement (Get-InstallerFunction "Test-MachineCertificateTrust") @'
function Test-MachineCertificateTrust([string]$Thumbprint) {
    $global:QuickLookInstallerHarnessEvents.Add("TrustProbe")
    return [bool]$global:QuickLookInstallerHarnessTrustPresent
}
'@
Add-Replacement (Get-InstallerFunction "Add-MachineCertificateTrust") @'
function Add-MachineCertificateTrust($Certificate) {
    $global:QuickLookInstallerHarnessEvents.Add("TrustAddDirect")
    if ($global:QuickLookInstallerHarnessTrustPresent) { return $false }
    $global:QuickLookInstallerHarnessTrustPresent = $true
    return $true
}
'@
Add-Replacement (Get-InstallerFunction "Remove-MachineCertificateTrust") @'
function Remove-MachineCertificateTrust($Certificate) {
    $global:QuickLookInstallerHarnessEvents.Add("TrustRemoveDirect")
    $global:QuickLookInstallerHarnessTrustPresent = $false
}
'@

$administratorAssignments = @($installerAst.FindAll({
    param($node)
    $node -is [Management.Automation.Language.AssignmentStatementAst] -and
        $node.Left -is [Management.Automation.Language.VariableExpressionAst] -and
        $node.Left.VariablePath.UserPath -eq "isAdministrator"
}, $true))
if ($administratorAssignments.Count -ne 1) {
    throw "Expected one isAdministrator assignment in Install.ps1."
}
Add-Replacement $administratorAssignments[0] `
    '$isAdministrator = [bool]$global:QuickLookInstallerHarnessIsAdministrator'

$instrumentedSource = $source
foreach ($replacement in @($replacementItems |
        Sort-Object Start -Descending)) {
    $instrumentedSource = $instrumentedSource.Remove(
        $replacement.Start,
        $replacement.Length).Insert(
        $replacement.Start,
        $replacement.Text)
}

$releaseCertificatePath = Join-Path (
    $Root) "packaging\QuickLook.Next-Release.cer"
$releaseCertificate = [Security.Cryptography.X509Certificates.X509Certificate2]::new(
    (Resolve-Path -LiteralPath $releaseCertificatePath).Path)
$packageName = "SherlockChiang.QuickLookNext"
$packageVersion = "1.2.3.4"
$tempBase = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd(
    [IO.Path]::DirectorySeparatorChar,
    [IO.Path]::AltDirectorySeparatorChar)
$tempRoot = [IO.Path]::GetFullPath((Join-Path $tempBase (
    "quicklook-installer-flow-" + [guid]::NewGuid().ToString("N"))))
$requiredTempPrefix = $tempBase + [IO.Path]::DirectorySeparatorChar
if (-not $tempRoot.StartsWith(
        $requiredTempPrefix,
        [StringComparison]::OrdinalIgnoreCase) -or
    [IO.Path]::GetFileName($tempRoot) -notmatch
        '^quicklook-installer-flow-[0-9a-f]{32}$') {
    throw "Installer harness temporary path escaped the system temp directory."
}
[IO.Directory]::CreateDirectory($tempRoot) | Out-Null
$instrumentedPath = Join-Path $tempRoot "Install.ps1"
$testCertificatePath = Join-Path $tempRoot "QuickLook.Next-Release.cer"
$testMsixPath = Join-Path $tempRoot (
    "QuickLook.Next-$packageVersion-win-x64.msix")

function Get-AuthenticodeSignature {
    [CmdletBinding()]
    param([string]$LiteralPath)

    $global:QuickLookInstallerHarnessEvents.Add("Signature")
    return [pscustomobject]@{
        Status = "Valid"
        SignerCertificate = $releaseCertificate
    }
}

function Start-Process {
    [CmdletBinding()]
    param(
        [string]$FilePath,
        [string[]]$ArgumentList,
        [string]$Verb,
        [switch]$Wait,
        [switch]$PassThru
    )

    if ($FilePath -ne "powershell.exe" -or $Verb -ne "RunAs" -or
        -not $Wait -or -not $PassThru -or
        $ArgumentList -notcontains "-TrustOnly") {
        throw "Installer invoked an unexpected elevated process."
    }
    if ($ArgumentList -contains "-RemoveTrust") {
        $global:QuickLookInstallerHarnessEvents.Add("ElevateRemoveTrust")
        $global:QuickLookInstallerHarnessTrustPresent = $false
        return [pscustomobject]@{ ExitCode = 0 }
    }

    $global:QuickLookInstallerHarnessEvents.Add("ElevateAddTrust")
    if ($global:QuickLookInstallerHarnessTrustPresent) {
        return [pscustomobject]@{ ExitCode = 10 }
    }
    $global:QuickLookInstallerHarnessTrustPresent = $true
    return [pscustomobject]@{ ExitCode = 0 }
}

function Add-AppxPackage {
    [CmdletBinding()]
    param(
        [string]$Path,
        [switch]$ForceApplicationShutdown
    )

    if (-not $ForceApplicationShutdown -or $Path -ne $testMsixPath) {
        throw "Installer invoked Add-AppxPackage with unexpected arguments."
    }
    $global:QuickLookInstallerHarnessEvents.Add("AddAppx")
    if ($global:QuickLookInstallerHarnessScenario -eq "RegistrationFailure") {
        throw "mock registration failed"
    }
    $global:QuickLookInstallerHarnessRegistered = $true
}

function Get-AppxPackage {
    [CmdletBinding()]
    param([string]$Name)

    if ($Name -ne $packageName) {
        throw "Installer queried an unexpected package name."
    }
    $global:QuickLookInstallerHarnessEvents.Add("GetAppx")
    if ($global:QuickLookInstallerHarnessScenario -eq
        "PostconditionFailure") {
        return
    }
    return [pscustomobject]@{
        Name = $packageName
        Publisher = $releaseCertificate.Subject
        Version = [version]$packageVersion
    }
}

function Write-Host {
    param(
        [Parameter(Position = 0, ValueFromRemainingArguments = $true)]
        [object[]]$Object,
        [ConsoleColor]$ForegroundColor
    )
}

function Assert-Scenario {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][bool]$InitialTrust,
        [Parameter(Mandatory = $true)][string[]]$ExpectedEvents,
        [Parameter(Mandatory = $true)][bool]$ExpectedTrust,
        [Parameter(Mandatory = $true)][bool]$ExpectFailure,
        [string]$ExpectedError = ""
    )

    $global:QuickLookInstallerHarnessScenario = $Name
    $global:QuickLookInstallerHarnessTrustPresent = $InitialTrust
    $global:QuickLookInstallerHarnessIsAdministrator = $false
    $global:QuickLookInstallerHarnessRegistered = $false
    $global:QuickLookInstallerHarnessEvents =
        [Collections.Generic.List[string]]::new()
    $failure = $null
    try {
        & $instrumentedPath
    }
    catch {
        $failure = $_
    }

    if (($null -ne $failure) -ne $ExpectFailure) {
        throw "$Name failure state was unexpected: $failure"
    }
    if ($ExpectedError -and
        $failure.Exception.Message -notmatch [regex]::Escape($ExpectedError)) {
        throw "$Name returned an unexpected error: $($failure.Exception.Message)"
    }
    $actualEvents = @($global:QuickLookInstallerHarnessEvents)
    if (($actualEvents -join "|") -ne ($ExpectedEvents -join "|")) {
        throw ("$Name events were '$($actualEvents -join '|')', expected " +
            "'$($ExpectedEvents -join '|')'.")
    }
    if ([bool]$global:QuickLookInstallerHarnessTrustPresent -ne
        $ExpectedTrust) {
        throw "$Name ended with an unexpected trust state."
    }
}

try {
    [IO.File]::WriteAllText(
        $instrumentedPath,
        $instrumentedSource,
        [Text.Encoding]::ASCII)
    [IO.File]::Copy(
        (Resolve-Path -LiteralPath $releaseCertificatePath).Path,
        $testCertificatePath)

    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::Open(
        $testMsixPath,
        [IO.Compression.ZipArchiveMode]::Create)
    try {
        $manifestEntry = $archive.CreateEntry("AppxManifest.xml")
        $manifestStream = $manifestEntry.Open()
        try {
            $writer = [IO.StreamWriter]::new(
                $manifestStream,
                [Text.UTF8Encoding]::new($false))
            try {
                $publisher = [Security.SecurityElement]::Escape(
                    $releaseCertificate.Subject)
                $writer.Write(
                    '<?xml version="1.0" encoding="utf-8"?>' +
                    '<Package xmlns="http://schemas.microsoft.com/appx/' +
                    'manifest/foundation/windows10">' +
                    '<Identity Name="' + $packageName +
                    '" Publisher="' + $publisher +
                    '" Version="' + $packageVersion +
                    '" ProcessorArchitecture="x64" />' +
                    '</Package>')
            }
            finally {
                $writer.Dispose()
            }
        }
        finally {
            $manifestStream.Dispose()
        }
    }
    finally {
        $archive.Dispose()
    }

    Assert-Scenario `
        -Name "FirstTrustSuccess" `
        -InitialTrust $false `
        -ExpectedEvents @(
            "Signature", "TrustProbe", "ElevateAddTrust", "TrustProbe",
            "Signature", "AddAppx", "GetAppx") `
        -ExpectedTrust $true `
        -ExpectFailure $false

    Assert-Scenario `
        -Name "RegistrationFailure" `
        -InitialTrust $false `
        -ExpectedEvents @(
            "Signature", "TrustProbe", "ElevateAddTrust", "TrustProbe",
            "Signature", "AddAppx", "ElevateRemoveTrust") `
        -ExpectedTrust $false `
        -ExpectFailure $true `
        -ExpectedError "mock registration failed"

    Assert-Scenario `
        -Name "PostconditionFailure" `
        -InitialTrust $false `
        -ExpectedEvents @(
            "Signature", "TrustProbe", "ElevateAddTrust", "TrustProbe",
            "Signature", "AddAppx", "GetAppx") `
        -ExpectedTrust $true `
        -ExpectFailure $true `
        -ExpectedError "Certificate trust was retained"

    Assert-Scenario `
        -Name "ExistingTrustSuccess" `
        -InitialTrust $true `
        -ExpectedEvents @(
            "Signature", "TrustProbe", "Signature", "AddAppx", "GetAppx") `
        -ExpectedTrust $true `
        -ExpectFailure $false
}
finally {
    if (Test-Path -LiteralPath $tempRoot -PathType Container) {
        $tempDirectory = Get-Item -LiteralPath $tempRoot -Force
        if (($tempDirectory.Attributes -band
                [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "Installer harness temporary directory became a reparse point."
        }
        foreach ($filePath in @(
                $instrumentedPath,
                $testCertificatePath,
                $testMsixPath)) {
            if (Test-Path -LiteralPath $filePath -PathType Leaf) {
                [IO.File]::Delete($filePath)
            }
        }
        [IO.Directory]::Delete($tempRoot, $false)
    }
    foreach ($name in @(
            "QuickLookInstallerHarnessEvents",
            "QuickLookInstallerHarnessIsAdministrator",
            "QuickLookInstallerHarnessRegistered",
            "QuickLookInstallerHarnessScenario",
            "QuickLookInstallerHarnessTrustPresent")) {
        Remove-Variable -Name $name -Scope Global -ErrorAction SilentlyContinue
    }
}

Microsoft.PowerShell.Utility\Write-Host `
    "installer executable control-flow test passed" -ForegroundColor Green
